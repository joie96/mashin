param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Debug', 'Release')]
    [string]$Profile,

    [Parameter(Mandatory = $true)]
    [ValidateSet('CurrentFlutter', 'WindowsX64', 'WindowsArm64', 'AndroidArm64')]
    [string]$PlatformSelection,

    [ValidateSet('Build', 'Test')]
    [string]$Action = 'Build',

    [string]$TestFilter = ''
)

$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$ffmpegDirectory = Join-Path $PSScriptRoot '../build_ffmpeg'
$engineRoot = Join-Path $workspaceRoot 'rust/mashin_engine'
$isRelease = $Profile -eq 'Release'

function Invoke-EngineCargo([string]$target, [string[]]$cargoArguments) {
    $ffmpegRoot = & (Join-Path $ffmpegDirectory 'build_ffmpeg.ps1') -Target $target
    $originalFfmpeg = $env:FFMPEG_DIR
    $originalLibclang = $env:LIBCLANG_PATH
    $originalPath = $env:PATH
    $originalNdk = $env:ANDROID_NDK_HOME
    $originalAndroidClangArgs = ${env:BINDGEN_EXTRA_CLANG_ARGS_aarch64-linux-android}
    Push-Location $engineRoot
    try {
        $env:FFMPEG_DIR = $ffmpegRoot
        $env:LIBCLANG_PATH = Split-Path -Parent (Get-Command clang-cl -ErrorAction Stop).Source
        $cargoPath = ($env:PATH -split ';' | Where-Object { $_ -notmatch '[/\\]Git[/\\]usr[/\\]bin[/\\]?$' }) -join ';'
        $env:PATH = "$(Join-Path $ffmpegRoot 'bin');$cargoPath"
        if ($target -eq 'aarch64-linux-android') {
            $configuration = Get-Content (Join-Path $ffmpegDirectory 'ffmpeg.json') -Raw | ConvertFrom-Json
            $sdkRoot = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { "$env:LOCALAPPDATA/Android/Sdk" }
            $env:ANDROID_NDK_HOME = Join-Path $sdkRoot "ndk/$($configuration.androidNdkVersion)"
            $sysroot = (Join-Path $env:ANDROID_NDK_HOME 'toolchains/llvm/prebuilt/windows-x86_64/sysroot') -replace '\\', '/'
            ${env:BINDGEN_EXTRA_CLANG_ARGS_aarch64-linux-android} = "--target=aarch64-linux-android26 --sysroot=`"$sysroot`" -I`"$sysroot/usr/include/aarch64-linux-android`""
        }
        & cargo @cargoArguments | Out-Host
        if ($LASTEXITCODE -ne 0) {
            throw "cargo $Action failed for $target."
        }
    } finally {
        Pop-Location
        $env:FFMPEG_DIR = $originalFfmpeg
        $env:LIBCLANG_PATH = $originalLibclang
        $env:PATH = $originalPath
        $env:ANDROID_NDK_HOME = $originalNdk
        ${env:BINDGEN_EXTRA_CLANG_ARGS_aarch64-linux-android} = $originalAndroidClangArgs
    }
    return $ffmpegRoot
}

function Build-WindowsEngine([string]$target) {
    $cargoArguments = if ($Action -eq 'Test') {
        @('test', '--target', $target, '--lib')
    } else {
        @('build', '--target', $target)
    }
    if ($isRelease) { $cargoArguments += '--release' }
    if ($TestFilter) { $cargoArguments += $TestFilter }
    $ffmpegRoot = Invoke-EngineCargo $target $cargoArguments
    if ($Action -eq 'Test') { return }
    $profileDirectory = if ($isRelease) { 'release' } else { 'debug' }
    $source = Join-Path $engineRoot "target/$target/$profileDirectory/mashin_engine.dll"
    $destinationDirectory = Join-Path $workspaceRoot 'windows/native'
    New-Item -ItemType Directory -Path $destinationDirectory -Force | Out-Null
    Copy-Item $source (Join-Path $destinationDirectory 'mashin_engine.dll') -Force
    $configuration = Get-Content (Join-Path $ffmpegDirectory 'ffmpeg.json') -Raw | ConvertFrom-Json
    $runtimeDirectory = Join-Path $destinationDirectory 'ffmpeg'
    New-Item -ItemType Directory -Path $runtimeDirectory -Force | Out-Null
    Remove-Item "$runtimeDirectory/*.dll" -Force
    foreach ($name in $configuration.windowsDlls) {
        Copy-Item (Join-Path $ffmpegRoot "bin/$name") $runtimeDirectory -Force
    }
    $licenses = Join-Path $destinationDirectory 'licenses/ffmpeg'
    New-Item -ItemType Directory -Path $licenses -Force | Out-Null
    Remove-Item "$licenses/*" -Force
    Copy-Item "$ffmpegRoot/licenses/*" $licenses -Force
}

function Build-AndroidEngine {
    if ($Action -eq 'Test') { throw 'Run Rust unit tests on a Windows host target, not through cargo ndk.' }
    $cargoArguments = @('ndk', '-t', 'arm64-v8a', '-P', '26', '-o', (Join-Path $workspaceRoot 'android/app/src/main/jniLibs'), 'build')
    if ($isRelease) {
        $cargoArguments += '--release'
    }
    $ffmpegRoot = Invoke-EngineCargo 'aarch64-linux-android' $cargoArguments
    $destinationDirectory = Join-Path $workspaceRoot 'android/app/src/main/jniLibs/arm64-v8a'
    $configuration = Get-Content (Join-Path $ffmpegDirectory 'ffmpeg.json') -Raw | ConvertFrom-Json
    foreach ($name in $configuration.libraries) {
        Copy-Item (Join-Path $ffmpegRoot "lib/lib$name.so") $destinationDirectory -Force
    }
    $licenses = Join-Path $workspaceRoot 'android/app/src/main/assets/licenses/ffmpeg'
    New-Item -ItemType Directory -Path $licenses -Force | Out-Null
    Remove-Item "$licenses/*" -Force
    Copy-Item "$ffmpegRoot/licenses/*" $licenses -Force
}

switch ($PlatformSelection) {
    'WindowsX64' { Build-WindowsEngine 'x86_64-pc-windows-msvc'; break }
    'WindowsArm64' { Build-WindowsEngine 'aarch64-pc-windows-msvc'; break }
    'AndroidArm64' { Build-AndroidEngine; break }
    'CurrentFlutter' {
        $devices = @(flutter devices --machine | ConvertFrom-Json)
        $platforms = $devices.targetPlatform | Where-Object { $_ -match '^(windows|android)-' } | Select-Object -Unique
        if ($platforms.Count -eq 0) {
            $platforms = if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') {
                @('windows-arm64')
            } else {
                @('windows-x64')
            }
        }

        foreach ($platform in $platforms) {
            switch -Regex ($platform) {
                '^windows-arm64$' { Build-WindowsEngine 'aarch64-pc-windows-msvc'; break }
                '^windows-x64$' { Build-WindowsEngine 'x86_64-pc-windows-msvc'; break }
                '^android-arm64$' { Build-AndroidEngine; break }
                default { Write-Warning "Rust playback engine is not configured for Flutter target '$platform'." }
            }
        }
    }
}