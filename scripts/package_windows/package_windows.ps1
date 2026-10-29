param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('x64', 'arm64')]
    [string]$Architecture,
    [Parameter(Mandatory = $true)]
    [pscustomobject]$Release,
    [string]$BundleDirectory = '',
    [string]$OutputDirectory = '',
    [string]$WixPath = ''
)

$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$metadata = $Release
$msiVersion = $metadata.MsiVersion
if (-not $BundleDirectory) { $BundleDirectory = Join-Path $workspaceRoot "build/windows/$Architecture/runner/Release" }
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $workspaceRoot "build/releases/$($metadata.Tag)/windows-$Architecture" }
$BundleDirectory = (Resolve-Path $BundleDirectory).Path
if (-not $WixPath) {
    $localWix = Join-Path $workspaceRoot 'build/tools/wix/wix.exe'
    $WixPath = if (Test-Path $localWix) { $localWix } else { (Get-Command wix -ErrorAction Stop).Source }
    if (Test-Path $localWix) {
        $env:DOTNET_ROOT = Join-Path $workspaceRoot 'build/tools/dotnet'
        $env:DOTNET_ROLL_FORWARD = 'Major'
    }
}
foreach ($required in @('mashin.exe', 'mashin_engine.dll', 'flutter_windows.dll', 'msvcp140.dll', 'vcruntime140.dll', 'data/icudtl.dat', 'data/app.so', 'data/flutter_assets', 'data/licenses/ffmpeg', 'data/release.json')) {
    if (-not (Test-Path (Join-Path $BundleDirectory $required))) { throw "Incomplete Windows bundle: $required is missing." }
}
$bundleMetadata = Get-Content (Join-Path $BundleDirectory 'data/release.json') -Raw | ConvertFrom-Json
if ($bundleMetadata.version -cne $metadata.Version -or $bundleMetadata.buildNumber -ne $metadata.BuildNumber -or $bundleMetadata.architecture -cne $Architecture) {
    throw 'Bundle metadata does not match the requested release. Rebuild the app first.'
}
$files = @(Get-ChildItem $BundleDirectory -Recurse -File | Where-Object { $_.Extension -notin @('.pdb', '.exp', '.lib') } | Sort-Object FullName)
foreach ($binary in $files | Where-Object { $_.Extension -in @('.exe', '.dll') }) {
    $reader = [IO.BinaryReader]::new([IO.File]::OpenRead($binary.FullName))
    try {
        if ($reader.ReadUInt16() -ne 0x5A4D) { throw "Invalid Windows binary: $($binary.Name)" }
        $reader.BaseStream.Position = 0x3C
        $headerOffset = $reader.ReadInt32()
        $reader.BaseStream.Position = $headerOffset
        if ($reader.ReadUInt32() -ne 0x4550) { throw "Invalid PE header: $($binary.Name)" }
        $expectedMachine = if ($Architecture -eq 'arm64') { 0xAA64 } else { 0x8664 }
        if ($reader.ReadUInt16() -ne $expectedMachine) { throw "Architecture mismatch: $($binary.Name) is not $Architecture." }
    } finally { $reader.Dispose() }
}
if ($files | Where-Object { $_.Name -eq 'config.json' -or $_.FullName.Substring($BundleDirectory.Length) -match '[/\\]Logs[/\\]' }) {
    throw 'The program bundle must not contain user configuration or logs.'
}
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$OutputDirectory = (Resolve-Path $OutputDirectory).Path
$hash = [Security.Cryptography.SHA256]::Create()
try {
    $identity = $hash.ComputeHash([Text.Encoding]::UTF8.GetBytes("$($metadata.UpgradeCode)/$($metadata.Version)/$Architecture"))
    $productCode = [guid]::new([byte[]]$identity[0..15]).ToString('D')
    $document = [xml]'<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs"><Fragment><DirectoryRef Id="INSTALLFOLDER"/><ComponentGroup Id="AppFiles"/></Fragment></Wix>'
    $namespace = $document.DocumentElement.NamespaceURI
    $directories = @{ '' = $document.DocumentElement.FirstChild.FirstChild }
    $group = $document.DocumentElement.FirstChild.LastChild
    foreach ($file in $files) {
        $relative = $file.FullName.Substring($BundleDirectory.Length + 1)
        $parent = Split-Path -Parent $relative
        $directoryKey = ''
        foreach ($segment in @($parent -split '\\' | Where-Object { $_ })) {
            $nextKey = if ($directoryKey) { "$directoryKey\$segment" } else { $segment }
            if (-not $directories.ContainsKey($nextKey)) {
                $directory = $document.CreateElement('Directory', $namespace)
                $directory.SetAttribute('Id', "Directory$($directories.Count)")
                $directory.SetAttribute('Name', $segment)
                $directories[$directoryKey].AppendChild($directory) | Out-Null
                $directories[$nextKey] = $directory
            }
            $directoryKey = $nextKey
        }
        $fileHash = [BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($relative.ToLowerInvariant()))).Replace('-', '').Substring(0, 24)
        $component = $document.CreateElement('Component', $namespace)
        $component.SetAttribute('Id', "Component$fileHash")
        $componentIdentity = $hash.ComputeHash([Text.Encoding]::UTF8.GetBytes("$($metadata.UpgradeCode)/$Architecture/$($relative.ToLowerInvariant())"))
        $component.SetAttribute('Guid', [guid]::new([byte[]]$componentIdentity[0..15]).ToString('D'))
        $directories[$directoryKey].AppendChild($component) | Out-Null
        $payload = $document.CreateElement('File', $namespace)
        $payload.SetAttribute('Id', "File$fileHash")
        $payload.SetAttribute('Source', $file.FullName)
        $component.AppendChild($payload) | Out-Null
        $registry = $document.CreateElement('RegistryValue', $namespace)
        foreach ($attribute in @{ Root = 'HKCU'; Key = 'Software\joie96\Mashin\Installer\Files'; Name = $fileHash; Type = 'integer'; Value = '1'; KeyPath = 'yes' }.GetEnumerator()) {
            $registry.SetAttribute($attribute.Key, $attribute.Value)
        }
        $component.AppendChild($registry) | Out-Null
        $reference = $document.CreateElement('ComponentRef', $namespace)
        $reference.SetAttribute('Id', "Component$fileHash")
        $group.AppendChild($reference) | Out-Null
    }
    foreach ($directory in $directories.Values) {
        $component = $directory.SelectSingleNode('*[local-name()="Component"]')
        if ($null -eq $component) { continue }
        $remove = $document.CreateElement('RemoveFolder', $namespace)
        $remove.SetAttribute('Id', "Remove$($directory.GetAttribute('Id'))")
        $remove.SetAttribute('On', 'uninstall')
        $component.AppendChild($remove) | Out-Null
    }
    $payloadSource = Join-Path $OutputDirectory 'AppFiles.wxs'
    $document.Save($payloadSource)
} finally {
    $hash.Dispose()
}
$output = Join-Path $OutputDirectory "Mashin-$($metadata.Version)-windows-$Architecture.msi"
$arguments = @('build', (Join-Path $PSScriptRoot 'Mashin.wxs'), $payloadSource,
    '-arch', $Architecture, '-culture', 'de-DE', '-ext', 'WixToolset.UI.wixext/5.0.2',
    '-loc', (Join-Path $PSScriptRoot 'Mashin.de-DE.wxl'),
    '-d', "ProductName=$($metadata.ProductName)", '-d', "Manufacturer=$($metadata.Manufacturer)",
    '-d', "MsiVersion=$msiVersion", '-d', "UpgradeCode=$($metadata.UpgradeCode)",
    '-d', "ProductCode=$productCode", '-d', "Repository=$($metadata.Repository)",
    '-d', "IconPath=$(Join-Path $workspaceRoot 'windows/runner/resources/app_icon.ico')",
    '-intermediatefolder', (Join-Path $OutputDirectory 'obj'), '-o', $output)
$WixPath = (Get-Command $WixPath -ErrorAction Stop).Source
Push-Location $workspaceRoot
try {
    & $WixPath @arguments | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'MSI packaging failed.' }
} finally { Pop-Location }
Write-Host "Created $output"
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$portableOutput = Join-Path $OutputDirectory "Mashin-$($metadata.Version)-windows-$Architecture-portable.zip"
$archiveStream = [IO.File]::Open($portableOutput, [IO.FileMode]::Create)
try {
    $archive = [IO.Compression.ZipArchive]::new($archiveStream, [IO.Compression.ZipArchiveMode]::Create, $true)
    try {
        foreach ($file in $files) {
            $relative = $file.FullName.Substring($BundleDirectory.Length + 1).Replace('\', '/')
            [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $file.FullName, $relative, [IO.Compression.CompressionLevel]::Optimal) | Out-Null
        }
    } finally { $archive.Dispose() }
} finally { $archiveStream.Dispose() }
Write-Host "Created $portableOutput"
return @($output, $portableOutput)