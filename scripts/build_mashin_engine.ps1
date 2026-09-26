param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Debug', 'Release')]
    [string]$Profile,

    [Parameter(Mandatory = $true)]
    [ValidateSet('CurrentFlutter', 'WindowsX64', 'WindowsArm64', 'AndroidArm64')]
    [string]$PlatformSelection
)

$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path -Parent $PSScriptRoot
$engineRoot = Join-Path $workspaceRoot 'rust/mashin_engine'
$isRelease = $Profile -eq 'Release'

function Build-WindowsEngine([string]$target) {
    $cargoArguments = @('build', '--target', $target)
    if ($isRelease) {
        $cargoArguments += '--release'
    }

    Push-Location $engineRoot
    try {
        & cargo @cargoArguments
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed for $target."
        }
    } finally {
        Pop-Location
    }

    $profileDirectory = if ($isRelease) { 'release' } else { 'debug' }
    $source = Join-Path $engineRoot "target/$target/$profileDirectory/mashin_engine.dll"
    $destinationDirectory = Join-Path $workspaceRoot 'windows/native'
    New-Item -ItemType Directory -Path $destinationDirectory -Force | Out-Null
    Copy-Item $source (Join-Path $destinationDirectory 'mashin_engine.dll') -Force
}

function Build-AndroidEngine {
    $cargoArguments = @('ndk', '-t', 'arm64-v8a', '-P', '26', '-o', (Join-Path $workspaceRoot 'android/app/src/main/jniLibs'), 'build')
    if ($isRelease) {
        $cargoArguments += '--release'
    }

    Push-Location $engineRoot
    try {
        & cargo @cargoArguments
        if ($LASTEXITCODE -ne 0) {
            throw 'cargo ndk build failed for Android arm64-v8a.'
        }
    } finally {
        Pop-Location
    }
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