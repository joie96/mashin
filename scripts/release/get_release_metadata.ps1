param(
    [string]$Version = '',
    [int]$BuildNumber = 0
)

$ErrorActionPreference = 'Stop'
$configuration = Get-Content (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'release.json') -Raw | ConvertFrom-Json
if (-not $Version) { $Version = $configuration.version }
if ($BuildNumber -eq 0) { $BuildNumber = $configuration.buildNumber }
if ($BuildNumber -lt 1 -or $BuildNumber -gt 65535) {
    throw 'BuildNumber must be between 1 and 65535 for Android and Windows file versions.'
}
if ($Version -cnotmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(dev|beta)\.([1-9][0-9]*))?$') {
    throw 'Version must be major.minor.patch, major.minor.patch-beta.N or major.minor.patch-dev.N.'
}
$major = [int]$Matches[1]
$minor = [int]$Matches[2]
$patch = [int]$Matches[3]
$channel = if ($Matches[4]) { $Matches[4] } else { 'stable' }
$sequence = if ($Matches[5]) { [int]$Matches[5] } else { 0 }
if ($major -gt 255 -or $minor -gt 255 -or $patch -gt 64 -or $sequence -gt 399) {
    throw 'MSI version limits: major/minor <= 255, patch <= 64, prerelease sequence <= 399.'
}
$channelOrder = switch ($channel) {
    'dev' { $sequence }
    'beta' { 400 + $sequence }
    'stable' { 999 }
}

[pscustomobject]@{
    Version = $Version
    BuildNumber = $BuildNumber
    Channel = $channel
    Tag = "v$Version"
    MsiVersion = "$major.$minor.$($patch * 1000 + $channelOrder)"
    ProductName = $configuration.productName
    Manufacturer = $configuration.manufacturer
    ApplicationId = $configuration.applicationId
    UpgradeCode = $configuration.upgradeCode
    Repository = $configuration.repository
}