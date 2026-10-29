param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc', 'aarch64-linux-android')]
    [string]$Target
)

$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$configuration = Get-Content (Join-Path $PSScriptRoot 'ffmpeg.json') -Raw | ConvertFrom-Json
$cacheRoot = Join-Path $workspaceRoot 'build/native'
$installRoot = Join-Path $cacheRoot "ffmpeg/$($configuration.version)/$Target"
$fingerprint = (@($PSCommandPath, (Join-Path $PSScriptRoot 'ffmpeg.json')) | ForEach-Object {
    (Get-FileHash -Path $_ -Algorithm SHA256).Hash
}) -join ':'
$stamp = Join-Path $installRoot 'build-stamp.txt'
$runtimeDirectory = if ($Target -like '*-windows-*') { 'bin' } else { 'lib' }
$runtimeNames = if ($Target -like '*-windows-*') {
    $configuration.windowsDlls
} else {
    $configuration.libraries | ForEach-Object { "lib$_.so" }
}
$requiredFiles = @($runtimeNames | ForEach-Object { "$runtimeDirectory/$_" })
$requiredFiles += 'include/libavcodec/avcodec.h', 'licenses/COPYING.LGPLv2.1', "licenses/ffmpeg-$($configuration.version).tar.xz"
if ($Target -like '*-windows-*') {
    $requiredFiles += @($configuration.libraries | ForEach-Object { "lib/$_.lib" })
}
$hasRuntime = @($requiredFiles | Where-Object {
    -not (Test-Path (Join-Path $installRoot $_))
}).Count -eq 0
if ($hasRuntime -and (Test-Path $stamp) -and [IO.File]::ReadAllText($stamp).Trim() -eq $fingerprint) {
    return $installRoot
}

# Acquisition and toolchain setup are shared by Debug, Release and host tests.
function Get-VerifiedArchive([string]$url, [string]$path, [string]$sha256) {
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    if (-not (Test-Path $path)) {
        Write-Host "Downloading $url"
        Invoke-WebRequest $url -OutFile $path -UseBasicParsing
    }
    if ((Get-FileHash $path -Algorithm SHA256).Hash -ne $sha256) {
        throw "SHA256 mismatch for $path. Remove this archive and retry."
    }
}

function ConvertTo-ShellPath([string]$path) {
    return ($path -replace '\\', '/' -replace '^([A-Za-z]):', '/$1')
}

$bash = if ($env:FFMPEG_BASH) { $env:FFMPEG_BASH } else { 'C:/Program Files/Git/bin/bash.exe' }
if (-not (Test-Path $bash)) {
    throw 'Install Git for Windows, or set FFMPEG_BASH to a compatible MSYS2 bash.exe.'
}
$tar = Join-Path $env:SystemRoot 'System32/tar.exe'
$shellBin = Join-Path (Split-Path -Parent (Split-Path -Parent $bash)) 'usr/bin'
$llvmBin = Split-Path -Parent (Get-Command clang-cl -ErrorAction Stop).Source
$originalEnvironment = [Environment]::GetEnvironmentVariables('Process')
try {
    $env:PATH = "$llvmBin;$shellBin;$env:PATH"
    $make = Get-Command make -ErrorAction SilentlyContinue
    if ($make) {
        $env:MAKE = ConvertTo-ShellPath $make.Source
    } else {
        $makeArchive = Join-Path $cacheRoot 'downloads/make-4.4.1-2.pkg.tar.zst'
        Get-VerifiedArchive 'https://repo.msys2.org/msys/x86_64/make-4.4.1-2-x86_64.pkg.tar.zst' $makeArchive '2408af61717dae87b00c855b132769a125c708907fc94a46bb16dae076113e5c'
        $tools = Join-Path $cacheRoot 'tools'
        New-Item -ItemType Directory -Path $tools -Force | Out-Null
        & $tar -xf $makeArchive -C $tools usr/bin/make.exe
        if ($LASTEXITCODE -ne 0) { throw 'Cannot extract GNU Make.' }
        $env:MAKE = ConvertTo-ShellPath (Join-Path $tools 'usr/bin/make.exe')
    }

    $ndkRoot = ''
    if ($Target -like '*-windows-*') {
        $vswhere = "${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
        $visualStudio = & $vswhere -latest -prerelease -products '*' -property installationPath
        if (-not $visualStudio) { throw 'Visual Studio C++ build tools are required.' }
        $architecture = if ($Target -like 'aarch64-*') { 'arm64' } else { 'x64' }
        $hostProcessorArchitecture = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
        $hostArchitecture = switch ($hostProcessorArchitecture) {
            'AMD64' { 'x64' }
            'ARM64' { 'arm64' }
            'x86' { 'x86' }
            default { throw "Unsupported Windows host architecture: $hostProcessorArchitecture" }
        }
        $developerCommand = Join-Path $visualStudio 'Common7/Tools/VsDevCmd.bat'
        $environment = & cmd /d /c "call `"$developerCommand`" -host_arch=$hostArchitecture -arch=$architecture -no_logo >nul & set"
        if ($LASTEXITCODE -ne 0) { throw 'Cannot initialize the Visual Studio toolchain.' }
        foreach ($entry in $environment) {
            if ($entry -match '^([^=]+)=(.*)$') {
                [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process')
            }
        }
        $env:PATH = "$llvmBin;$shellBin;$env:PATH"
    } else {
        $sdkRoot = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { "$env:LOCALAPPDATA/Android/Sdk" }
        $ndkRoot = Join-Path $sdkRoot "ndk/$($configuration.androidNdkVersion)"
        if (-not (Test-Path "$ndkRoot/toolchains/llvm/prebuilt/windows-x86_64/bin/clang.exe")) {
            throw "Install Android NDK $($configuration.androidNdkVersion) in $sdkRoot."
        }
    }

    $archive = Join-Path $cacheRoot "downloads/ffmpeg-$($configuration.version).tar.xz"
    Get-VerifiedArchive "https://ffmpeg.org/releases/ffmpeg-$($configuration.version).tar.xz" $archive $configuration.sha256
    $sourceParent = Join-Path $cacheRoot 'sources'
    $sourceRoot = Join-Path $sourceParent "ffmpeg-$($configuration.version)"
    if (-not (Test-Path "$sourceRoot/configure")) {
        New-Item -ItemType Directory -Path $sourceParent -Force | Out-Null
        & $tar -xf $archive -C $sourceParent
        if ($LASTEXITCODE -ne 0) { throw 'Cannot extract FFmpeg.' }
    }

    $configureArguments = @(
        "--prefix=$(ConvertTo-ShellPath $installRoot)",
        '--disable-autodetect', '--disable-everything',
        '--disable-programs', '--disable-doc', '--disable-network',
        '--disable-avdevice', '--disable-avfilter', '--disable-swscale',
        '--disable-static', '--enable-shared', '--enable-pic',
        '--enable-avcodec', '--enable-avformat', '--enable-avutil', '--enable-swresample',
        '--enable-decoder=flac,opus', '--enable-demuxer=flac,ogg',
        '--enable-parser=flac', '--disable-debug', '--disable-asm', '--host-cc=clang'
    )
    if ($Target -like '*-windows-*') {
        $architecture = if ($Target -like 'aarch64-*') { 'aarch64' } else { 'x86_64' }
        $configureArguments += @(
            '--target-os=win32', "--arch=$architecture", '--enable-cross-compile',
            '--toolchain=msvc', "--cc=clang-cl --target=$Target",
            '--ld=lld-link', '--ar=llvm-lib', '--nm=llvm-nm'
        )
    } else {
        $toolchain = ConvertTo-ShellPath (Join-Path $ndkRoot 'toolchains/llvm/prebuilt/windows-x86_64/bin')
        $configureArguments += @(
            '--target-os=android', '--arch=aarch64', '--enable-cross-compile',
            "--cc=$toolchain/aarch64-linux-android26-clang.cmd",
            "--ld=$toolchain/aarch64-linux-android26-clang.cmd",
            "--ar=$toolchain/llvm-ar.exe", "--nm=$toolchain/llvm-nm.exe",
            "--ranlib=$toolchain/llvm-ranlib.exe", "--strip=$toolchain/llvm-strip.exe",
            '--extra-ldflags=-Wl,-z,max-page-size=16384'
        )
    }
    $buildRoot = Join-Path $installRoot 'build'
    New-Item -ItemType Directory -Path $buildRoot -Force | Out-Null
    $makeExecutable = if ($make) { $make.Source } else { Join-Path $tools 'usr/bin/make.exe' }
    $jobs = if ($env:NUMBER_OF_PROCESSORS) { $env:NUMBER_OF_PROCESSORS } else { '4' }
    Push-Location $buildRoot
    try {
        & $bash (ConvertTo-ShellPath (Join-Path $sourceRoot 'configure')) @configureArguments | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "FFmpeg configure failed; see $buildRoot/ffbuild/config.log." }
        & $makeExecutable -j $jobs | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "FFmpeg compilation failed for $Target." }
        & $makeExecutable install | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "FFmpeg installation failed for $Target." }
    } finally {
        Pop-Location
    }
    if ($Target -like '*-windows-*') {
        Copy-Item "$installRoot/bin/*.lib" (Join-Path $installRoot 'lib') -Force
    }
    $licenseDirectory = Join-Path $installRoot 'licenses'
    New-Item -ItemType Directory -Path $licenseDirectory -Force | Out-Null
    Remove-Item "$licenseDirectory/*" -Force
    Copy-Item "$sourceRoot/COPYING.LGPLv2.1" $licenseDirectory -Force
    Copy-Item "$sourceRoot/LICENSE.md" $licenseDirectory -Force
    Copy-Item $archive $licenseDirectory -Force
    Copy-Item $PSCommandPath $licenseDirectory -Force
    Copy-Item (Join-Path $PSScriptRoot 'ffmpeg.json') $licenseDirectory -Force
    [IO.File]::WriteAllText($stamp, $fingerprint, [Text.UTF8Encoding]::new($false))
} finally {
    foreach ($name in [Environment]::GetEnvironmentVariables('Process').Keys) {
        if (-not $originalEnvironment.Contains($name)) {
            [Environment]::SetEnvironmentVariable($name, $null, 'Process')
        }
    }
    foreach ($name in $originalEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $originalEnvironment[$name], 'Process')
    }
}
return $installRoot