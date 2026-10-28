$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$toolsDirectory = Join-Path $workspaceRoot 'build/tools'
$dotnetDirectory = Join-Path $toolsDirectory 'dotnet'
$wixDirectory = Join-Path $toolsDirectory 'wix'
$sdkVersion = '8.0.427'
$wixVersion = '5.0.2'
$env:DOTNET_ROOT = $dotnetDirectory
$env:DOTNET_ROLL_FORWARD = 'Major'
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$env:DOTNET_SKIP_FIRST_TIME_EXPERIENCE = '1'
if (-not (Test-Path (Join-Path $dotnetDirectory "sdk/$sdkVersion"))) {
    $releases = Invoke-RestMethod 'https://builds.dotnet.microsoft.com/dotnet/release-metadata/8.0/releases.json'
    $sdk = @($releases.releases | ForEach-Object { $_.sdks } | Where-Object { $_.version -eq $sdkVersion }) | Select-Object -First 1
    $archive = $sdk.files | Where-Object { $_.rid -eq 'win-x64' -and $_.name -like '*.zip' } | Select-Object -First 1
    if (-not $archive -or $archive.url -notlike 'https://builds.dotnet.microsoft.com/*') { throw 'Official .NET SDK download metadata is missing.' }
    New-Item -ItemType Directory -Path $toolsDirectory -Force | Out-Null
    $download = Join-Path $toolsDirectory "dotnet-sdk-$sdkVersion.zip"
    Invoke-WebRequest -UseBasicParsing $archive.url -OutFile $download
    if ((Get-FileHash $download -Algorithm SHA512).Hash -ine $archive.hash) { throw '.NET SDK SHA512 verification failed.' }
    Expand-Archive $download -DestinationPath $dotnetDirectory -Force
}
$dotnet = Join-Path $dotnetDirectory 'dotnet.exe'
$wix = Join-Path $wixDirectory 'wix.exe'
if (-not (Test-Path $wix)) {
    & $dotnet tool install wix --version $wixVersion --tool-path $wixDirectory | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'WiX installation failed.' }
}
$installedVersion = & $wix --version
if ($LASTEXITCODE -ne 0 -or $installedVersion -notlike "$wixVersion*" ) { throw "WiX $wixVersion is required in $wixDirectory." }
Push-Location $workspaceRoot
try {
    & $wix extension add "WixToolset.UI.wixext/$wixVersion" | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'WiX UI extension installation failed.' }
} finally { Pop-Location }
Write-Host ".NET SDK $sdkVersion and WiX $wixVersion are ready in $toolsDirectory."