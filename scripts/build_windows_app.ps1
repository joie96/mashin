param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('x64', 'arm64')]
    [string]$Architecture
)

$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path -Parent $PSScriptRoot
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$installation = & $vswhere -latest -prerelease -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $installation) { throw 'Visual Studio C++ build tools are required.' }
$cmake = Join-Path $installation 'Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe'
$platform = "windows-$Architecture"
$cmakeArchitecture = if ($Architecture -eq 'arm64') { 'ARM64' } else { 'x64' }
$buildDirectory = Join-Path $workspaceRoot "build/windows/$Architecture"
$flutterRoot = Split-Path -Parent (Split-Path -Parent (Get-Command flutter -ErrorAction Stop).Source)

Push-Location $workspaceRoot
try {
    & flutter precache --windows
    if ($LASTEXITCODE -ne 0) { throw 'Flutter Windows artifact download failed.' }
    & flutter build windows --release --config-only
    if ($LASTEXITCODE -ne 0) { throw 'Flutter Windows configuration failed.' }
    $cache = Get-ChildItem "$workspaceRoot/build/windows/*/CMakeCache.txt" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $cache) { throw 'Flutter CMake configuration is missing.' }
    $generatorEntry = Select-String -Path $cache.FullName -Pattern '^CMAKE_GENERATOR:INTERNAL=(.+)$'
    if (-not $generatorEntry) { throw 'Could not determine the Visual Studio CMake generator.' }
    $generator = $generatorEntry.Matches[0].Groups[1].Value

    $engine = [IO.File]::ReadAllText((Join-Path $flutterRoot 'bin/cache/engine.stamp')).Trim()
    $downloadDirectory = Join-Path $workspaceRoot "build/native/flutter/$engine/$platform"
    $storage = if ($env:FLUTTER_STORAGE_BASE_URL) { $env:FLUTTER_STORAGE_BASE_URL.TrimEnd('/') } else { 'https://storage.googleapis.com' }
    $artifacts = @(
        @{ Directory = $platform; Archive = "$platform-debug/$platform-flutter.zip"; Required = 'flutter_windows.dll' },
        @{ Directory = $platform; Archive = "$platform/flutter-cpp-client-wrapper.zip"; Required = 'cpp_client_wrapper/include/flutter/flutter_engine.h' },
        @{ Directory = "$platform-release"; Archive = "$platform-release/$platform-flutter.zip"; Required = 'gen_snapshot.exe' }
    )
    foreach ($artifact in $artifacts) {
        $destination = Join-Path $flutterRoot "bin/cache/artifacts/engine/$($artifact.Directory)"
        $stamp = Join-Path $downloadDirectory "$($artifact.Archive -replace '/', '_').ready"
        if ((Test-Path $stamp) -and (Test-Path (Join-Path $destination $artifact.Required))) { continue }
        New-Item -ItemType Directory -Path $downloadDirectory -Force | Out-Null
        $archive = Join-Path $downloadDirectory ($artifact.Archive -replace '/', '_')
        Invoke-WebRequest -UseBasicParsing -Uri "$storage/flutter_infra_release/flutter/$engine/$($artifact.Archive)" -OutFile $archive
        Expand-Archive -Path $archive -DestinationPath $destination -Force
        if (-not (Test-Path (Join-Path $destination $artifact.Required))) { throw "Incomplete Flutter artifact: $($artifact.Archive)" }
        New-Item -ItemType File -Path $stamp -Force | Out-Null
    }
    & $cmake -S windows -B $buildDirectory -G $generator -A $cmakeArchitecture "-DFLUTTER_TARGET_PLATFORM=$platform"
    if ($LASTEXITCODE -ne 0) { throw "CMake configuration failed for $platform." }
    & $cmake --build $buildDirectory --config Release --target INSTALL
    if ($LASTEXITCODE -ne 0) { throw "Windows release build failed for $platform." }
} finally {
    Pop-Location
}