param([switch]$PackageFixture)

$ErrorActionPreference = 'Stop'
$metadataScript = Join-Path $PSScriptRoot '../release/get_release_metadata.ps1'

function Assert-Equal($Actual, $Expected) {
    if ($Actual -cne $Expected) { throw "Expected '$Expected', received '$Actual'." }
}

function Assert-Rejected([scriptblock]$Operation) {
    $hasFailed = $false
    try { & $Operation | Out-Null } catch { $hasFailed = $true }
    if (-not $hasFailed) { throw 'Expected release validation to reject this input.' }
}

function Get-MsiValue($Database, [string]$Query) {
    $view = $Database.OpenView($Query)
    try {
        $view.Execute() | Out-Null
        $record = $view.Fetch()
        if ($null -ne $record) { return $record.StringData(1) }
        return $null
    } finally { $view.Close() | Out-Null }
}

$metadata = & $metadataScript
Assert-Equal $metadata.ApplicationId 'com.joie96.mashin'
Assert-Equal $metadata.Tag "v$($metadata.Version)"
Assert-Equal (& $metadataScript -Version '1.2.3' -BuildNumber 12).MsiVersion '1.2.3999'
Assert-Equal (& $metadataScript -Version '1.2.3-beta.2').Channel 'beta'
Assert-Equal (& $metadataScript -Version '1.2.3-dev.42').Channel 'dev'
$versions = @('1.2.3-dev.1', '1.2.3-dev.399', '1.2.3-beta.1', '1.2.3-beta.399', '1.2.3', '1.2.4-dev.1', '1.3.0-dev.1', '2.0.0-dev.1')
$previous = [version]'0.0.0'
foreach ($version in $versions) {
    $current = [version](& $metadataScript -Version $version).MsiVersion
    if ($current -le $previous) { throw "MSI upgrade order failed at $version." }
    $previous = $current
}
foreach ($version in @('v1.2.3', '1.2', '01.2.3', '1.2.3-beta.0', '1.2.3-beta.400', '1.2.3-rc.1', '256.0.0', '1.256.0', '1.2.65', '1.2.3+4')) {
    Assert-Rejected { & $metadataScript -Version $version }
}
Assert-Rejected { & $metadataScript -BuildNumber -1 }
Assert-Rejected { & $metadataScript -BuildNumber 65536 }
Write-Host 'Release metadata tests passed.'

$workspaceRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$collectionFixture = Join-Path $workspaceRoot "build/release-tests/collection-$([guid]::NewGuid().ToString('N'))"
$collectorScript = Join-Path $PSScriptRoot '../release/collect_release_artifacts.ps1'
$fixtureCommit = 'a' * 40
try {
    foreach ($target in @('windows-x64', 'windows-arm64', 'android-arm64')) {
        $directory = Join-Path $collectionFixture $target
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
        $extension = if ($target -eq 'android-arm64') { 'apk' } else { 'msi' }
        $fileName = "Mashin-$($metadata.Version)-$target.$extension"
        $package = Join-Path $directory $fileName
        [IO.File]::WriteAllText($package, "Collection fixture for $target")
        $checksum = (Get-FileHash $package -Algorithm SHA256).Hash.ToLowerInvariant()
        [IO.File]::WriteAllText("$package.sha256", "$checksum  $fileName`n")
        $manifest = @{ version = $metadata.Version; buildNumber = $metadata.BuildNumber; channel = $metadata.Channel;
            target = $target; commit = $fixtureCommit; isDirty = $false; file = $fileName;
            sha256 = $checksum; size = (Get-Item $package).Length; isSigned = $true }
        [IO.File]::WriteAllText((Join-Path $directory 'artifact.json'), ($manifest | ConvertTo-Json))
    }
    $collectionOutput = Join-Path $collectionFixture 'output'
    & $collectorScript -InputDirectory $collectionFixture -Version $metadata.Version -BuildNumber $metadata.BuildNumber -Commit $fixtureCommit -OutputDirectory $collectionOutput
    $collected = Get-Content (Join-Path $collectionOutput 'build.json') -Raw | ConvertFrom-Json
    Assert-Equal $collected.artifacts.Count 3
    Assert-Rejected { & $collectorScript -InputDirectory $collectionFixture -Version $metadata.Version -BuildNumber ($metadata.BuildNumber + 1) -Commit $fixtureCommit -OutputDirectory "$collectionFixture/mismatch" }
    Assert-Rejected { & $collectorScript -InputDirectory $collectionFixture -Version $metadata.Version -BuildNumber $metadata.BuildNumber -Commit ('b' * 40) -OutputDirectory "$collectionFixture/wrong-commit" }
    $windowsPackage = Get-ChildItem (Join-Path $collectionFixture 'windows-x64') -Filter '*.msi' | Select-Object -First 1
    [IO.File]::AppendAllText($windowsPackage.FullName, 'corrupted')
    Assert-Rejected { & $collectorScript -InputDirectory $collectionFixture -Version $metadata.Version -BuildNumber $metadata.BuildNumber -Commit $fixtureCommit -OutputDirectory "$collectionFixture/corrupt" }
    [IO.File]::WriteAllText($windowsPackage.FullName, 'Collection fixture for windows-x64')
    Remove-Item (Join-Path $collectionFixture 'android-arm64') -Recurse -Force
    Assert-Rejected { & $collectorScript -InputDirectory $collectionFixture -Version $metadata.Version -BuildNumber $metadata.BuildNumber -Commit $fixtureCommit -OutputDirectory "$collectionFixture/incomplete" }
    Write-Host 'Shared all-platform artifact collection tests passed.'
} finally {
    if (Test-Path $collectionFixture) { Remove-Item $collectionFixture -Recurse -Force }
}

if ($PackageFixture) {
    $workspaceRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
    $fixture = Join-Path $workspaceRoot 'build/release-tests/bundle'
    foreach ($relative in @('mashin.exe', 'mashin_engine.dll', 'flutter_windows.dll', 'msvcp140.dll', 'vcruntime140.dll', 'data/icudtl.dat', 'data/app.so', 'data/flutter_assets/AssetManifest.bin', 'data/licenses/ffmpeg/COPYING.LGPLv2.1')) {
        $path = Join-Path $fixture $relative
        New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
        if ([IO.Path]::GetExtension($path) -in @('.exe', '.dll')) {
            $bytes = [byte[]]::new(256)
            [BitConverter]::GetBytes([uint16]0x5A4D).CopyTo($bytes, 0)
            [BitConverter]::GetBytes([int]128).CopyTo($bytes, 0x3C)
            [BitConverter]::GetBytes([uint32]0x4550).CopyTo($bytes, 128)
            [BitConverter]::GetBytes([uint16]0xAA64).CopyTo($bytes, 132)
            [IO.File]::WriteAllBytes($path, $bytes)
        } else {
            [IO.File]::WriteAllText($path, 'Packaging fixture, not an executable.')
        }
    }
    $buildMetadata = @{ version = '0.1.0-beta.1'; buildNumber = 1; architecture = 'arm64' } | ConvertTo-Json
    [IO.File]::WriteAllText((Join-Path $fixture 'data/release.json'), $buildMetadata)
    $packageScript = Join-Path $PSScriptRoot '../release/package_windows_app.ps1'
    Assert-Rejected { & $packageScript -Architecture x64 -BundleDirectory $fixture }
    Assert-Rejected { & $packageScript -Architecture arm64 -Version '0.1.0-beta.2' -BundleDirectory $fixture }
    [IO.File]::WriteAllText((Join-Path $fixture 'config.json'), '{}')
    Assert-Rejected { & $packageScript -Architecture arm64 -BundleDirectory $fixture }
    Remove-Item (Join-Path $fixture 'config.json')
    $output = & $packageScript -Architecture arm64 -Version '0.1.0-beta.1' -BuildNumber 1 -BundleDirectory $fixture -OutputDirectory (Join-Path $workspaceRoot 'build/release-tests/output')
    if (-not (Test-Path $output)) { throw 'Fixture MSI was not created.' }
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase($output, 0)
    try {
        Assert-Equal (Get-MsiValue $database 'SELECT `Value` FROM `Property` WHERE `Property` = ''ProductVersion''') '0.1.401'
        Assert-Equal (Get-MsiValue $database 'SELECT `Value` FROM `Property` WHERE `Property` = ''UpgradeCode''') "{$($metadata.UpgradeCode)}"
        Assert-Equal (Get-MsiValue $database 'SELECT `Value` FROM `Property` WHERE `Property` = ''ALLUSERS''') $null
        Assert-Equal (Get-MsiValue $database 'SELECT `Directory_Parent` FROM `Directory` WHERE `Directory` = ''INSTALLFOLDER''') 'MASHINDATA'
        Assert-Equal (Get-MsiValue $database 'SELECT `Directory_Parent` FROM `Directory` WHERE `Directory` = ''MASHINDATA''') 'LocalAppDataFolder'
        Assert-Equal (Get-MsiValue $database 'SELECT `Text` FROM `Control` WHERE `Dialog_` = ''VerifyReadyDlg'' AND `Control` = ''Install''') '&[MASHINACTION]'
        Assert-Equal (Get-MsiValue $database 'SELECT `Target` FROM `CustomAction` WHERE `Source` = ''MASHINACTION''') 'Aktualisieren'
        Assert-Equal (Get-MsiValue $database 'SELECT `ActionProperty` FROM `Upgrade` WHERE `ActionProperty` = ''SAMEVERSIONFOUND''') 'SAMEVERSIONFOUND'
        Assert-Equal (Get-MsiValue $database 'SELECT `FileName` FROM `RemoveFile` WHERE `FileName` IS NOT NULL') $null
        $removeSequence = [int](Get-MsiValue $database 'SELECT `Sequence` FROM `InstallExecuteSequence` WHERE `Action` = ''RemoveExistingProducts''')
        $initializeSequence = [int](Get-MsiValue $database 'SELECT `Sequence` FROM `InstallExecuteSequence` WHERE `Action` = ''InstallInitialize''')
        $filesSequence = [int](Get-MsiValue $database 'SELECT `Sequence` FROM `InstallExecuteSequence` WHERE `Action` = ''InstallFiles''')
        if ($removeSequence -le $initializeSequence -or $removeSequence -ge $filesSequence) { throw 'Upgrade removal must run inside the rollback transaction before file installation.' }
    } finally {
        [Runtime.InteropServices.Marshal]::FinalReleaseComObject($database) | Out-Null
        [Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer) | Out-Null
    }
    Write-Host 'MSI contracts passed. Do not install this fixture package.'
}