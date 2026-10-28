param(
    [Parameter(Mandatory = $true)]
    [string]$InputDirectory,
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [Parameter(Mandatory = $true)]
    [int]$BuildNumber,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-f]{40}$')]
    [string]$Commit,
    [string]$OutputDirectory = ''
)

$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$metadata = & (Join-Path $PSScriptRoot 'get_release_metadata.ps1') -Version $Version -BuildNumber $BuildNumber
$InputDirectory = (Resolve-Path $InputDirectory).Path
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $workspaceRoot "build/releases/$($metadata.Tag)/all" }
if ((Test-Path $OutputDirectory) -and (Get-ChildItem $OutputDirectory -Force)) { throw 'Collection output directory must be empty.' }
$artifacts = @()
foreach ($target in @('windows-x64', 'windows-arm64', 'android-arm64')) {
    $directory = Join-Path $InputDirectory $target
    $manifestPath = Join-Path $directory 'artifact.json'
    if (-not (Test-Path $manifestPath -PathType Leaf)) { throw "Missing platform artifact: $target" }
    $manifest = Get-Content $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.version -cne $metadata.Version -or $manifest.buildNumber -ne $metadata.BuildNumber -or
        $manifest.channel -cne $metadata.Channel -or $manifest.commit -cne $Commit -or
        $manifest.target -cne $target -or $manifest.isDirty -ne $false) {
        throw "Artifact identity does not match the shared clean build: $target"
    }
    $extension = if ($target -eq 'android-arm64') { 'apk' } else { 'msi' }
    $fileName = "Mashin-$($metadata.Version)-$target.$extension"
    if ($manifest.file -cne $fileName) { throw "Unexpected artifact filename: $target" }
    if ($target -eq 'android-arm64' -and $manifest.isSigned -ne $true) { throw 'Android artifact must be signed.' }
    $package = Join-Path $directory $fileName
    $checksum = (Get-FileHash $package -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($manifest.sha256 -cne $checksum -or $manifest.size -ne (Get-Item $package).Length) { throw "Artifact integrity check failed: $target" }
    $checksumPath = "$package.sha256"
    if ([IO.File]::ReadAllText($checksumPath).Trim() -cne "$checksum  $fileName") { throw "Checksum file mismatch: $target" }
    $artifacts += $manifest
}
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
foreach ($artifact in $artifacts) {
    $source = Join-Path $InputDirectory "$($artifact.target)/$($artifact.file)"
    Copy-Item $source, "$source.sha256" $OutputDirectory
    Copy-Item (Join-Path $InputDirectory "$($artifact.target)/artifact.json") (Join-Path $OutputDirectory "$($artifact.target).json")
}
$build = [ordered]@{
    version = $metadata.Version
    buildNumber = $metadata.BuildNumber
    channel = $metadata.Channel
    commit = $Commit
    artifacts = $artifacts
}
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'build.json'), ($build | ConvertTo-Json -Depth 5))
Write-Host "Collected all three platforms for $($metadata.Version)+$($metadata.BuildNumber) in $OutputDirectory."