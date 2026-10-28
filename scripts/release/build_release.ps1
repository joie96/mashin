param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('windows-x64', 'windows-arm64', 'android-arm64')]
    [string]$Target,
    [string]$Version = '',
    [int]$BuildNumber = 0
)

$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$metadata = & (Join-Path $PSScriptRoot 'get_release_metadata.ps1') -Version $Version -BuildNumber $BuildNumber
$outputDirectory = Join-Path $workspaceRoot "build/releases/$($metadata.Tag)/$Target"
$nativeTarget = switch ($Target) {
    'windows-x64' { 'WindowsX64' }
    'windows-arm64' { 'WindowsArm64' }
    'android-arm64' { 'AndroidArm64' }
}
if ($Target -eq 'android-arm64') {
    $signingFile = Join-Path $workspaceRoot 'android/key.properties'
    $hasSigningEnvironment = $env:MASHIN_ANDROID_KEYSTORE -and $env:MASHIN_ANDROID_KEYSTORE_PASSWORD -and $env:MASHIN_ANDROID_KEY_ALIAS -and $env:MASHIN_ANDROID_KEY_PASSWORD
    if (-not (Test-Path $signingFile) -and -not $hasSigningEnvironment) {
        throw 'Android release signing is missing. Configure android/key.properties or all MASHIN_ANDROID_KEY* environment variables; see docs/releases.md.'
    }
}
Push-Location $workspaceRoot
try {
    if ($Target.StartsWith('windows-')) {
        & (Join-Path $PSScriptRoot 'setup_release_tools.ps1')
    }
    & (Join-Path $PSScriptRoot '../build/build_mashin_engine.ps1') -Profile Release -PlatformSelection $nativeTarget
    if ($LASTEXITCODE -ne 0) { throw 'Native release build failed.' }
    New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
    if ($Target.StartsWith('windows-')) {
        $architecture = $Target.Substring('windows-'.Length)
        & (Join-Path $PSScriptRoot '../build/build_windows_app.ps1') -Architecture $architecture -Version $metadata.Version -BuildNumber $metadata.BuildNumber
        $artifact = & (Join-Path $PSScriptRoot 'package_windows_app.ps1') -Architecture $architecture -Version $metadata.Version -BuildNumber $metadata.BuildNumber -OutputDirectory $outputDirectory
    } else {
        & flutter build apk --release --target-platform android-arm64 "--build-name=$($metadata.Version)" "--build-number=$($metadata.BuildNumber)"
        if ($LASTEXITCODE -ne 0) { throw 'Android release build failed.' }
        $artifact = Join-Path $outputDirectory "Mashin-$($metadata.Version)-android-arm64.apk"
        Copy-Item (Join-Path $workspaceRoot 'build/app/outputs/flutter-apk/app-release.apk') $artifact -Force
    }
    $artifact = [string]$artifact
    $checksum = (Get-FileHash $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText("$artifact.sha256", "$checksum  $([IO.Path]::GetFileName($artifact))`n")
    $commit = & git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Could not determine release commit.' }
    $changes = & git status --porcelain --untracked-files=normal
    if ($LASTEXITCODE -ne 0) { throw 'Could not determine working tree state.' }
    $manifest = [ordered]@{
        version = $metadata.Version
        buildNumber = $metadata.BuildNumber
        channel = $metadata.Channel
        target = $Target
        commit = $commit
        isDirty = [bool]$changes
        file = [IO.Path]::GetFileName($artifact)
        sha256 = $checksum
        size = (Get-Item $artifact).Length
        isSigned = if ($Target.StartsWith('windows-')) { (Get-AuthenticodeSignature $artifact).Status -eq 'Valid' } else { $true }
    }
    [IO.File]::WriteAllText((Join-Path $outputDirectory 'artifact.json'), ($manifest | ConvertTo-Json))
    Write-Host "Release artifact: $artifact"
} finally {
    Pop-Location
}