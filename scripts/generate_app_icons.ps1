param(
    [string]$InkscapePath = 'C:\Program Files\Inkscape\bin\inkscape.com'
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $InkscapePath -PathType Leaf)) {
    throw "Inkscape not found: $InkscapePath. Pass -InkscapePath with its executable path."
}

$projectRoot = Split-Path -Parent $PSScriptRoot
$sourcePath = Join-Path $projectRoot 'assets\branding\appicon.svg'
$outputDirectory = Join-Path $projectRoot 'build\app_icons'
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null

$exports = @(
    @{ Size = 1024; Path = (Join-Path $projectRoot 'assets\branding\appicon.png') }
)
$androidSizes = [ordered]@{
    mdpi = 48
    hdpi = 72
    xhdpi = 96
    xxhdpi = 144
    xxxhdpi = 192
}
foreach ($density in $androidSizes.Keys) {
    $exports += @{
        Size = $androidSizes[$density]
        Path = (Join-Path $projectRoot "android\app\src\main\res\mipmap-$density\ic_launcher.png")
    }
}

$windowsSizes = @(16, 20, 24, 32, 40, 48, 64, 128, 256)
foreach ($size in $windowsSizes) {
    $exports += @{ Size = $size; Path = (Join-Path $outputDirectory "windows-$size.png") }
}

foreach ($export in $exports) {
    & $InkscapePath $sourcePath '--export-area-page' '--export-type=png' `
        "--export-width=$($export.Size)" "--export-height=$($export.Size)" `
        "--export-filename=$($export.Path)"
    if ($LASTEXITCODE -ne 0) {
        throw "Inkscape failed for $($export.Path) with exit code $LASTEXITCODE."
    }
    if (-not (Test-Path -LiteralPath $export.Path -PathType Leaf)) {
        throw "Inkscape did not create $($export.Path). On Windows, use inkscape.com."
    }
}

$iconPath = Join-Path $projectRoot 'windows\runner\resources\app_icon.ico'
$frames = foreach ($size in $windowsSizes) {
    ,([System.IO.File]::ReadAllBytes((Join-Path $outputDirectory "windows-$size.png")))
}
$writer = [System.IO.BinaryWriter]::new([System.IO.File]::Create($iconPath))
try {
    $writer.Write([uint16]0)
    $writer.Write([uint16]1)
    $writer.Write([uint16]$windowsSizes.Count)
    $offset = 6 + 16 * $windowsSizes.Count
    for ($index = 0; $index -lt $windowsSizes.Count; $index++) {
        $dimension = $windowsSizes[$index] % 256
        $writer.Write([byte]$dimension)
        $writer.Write([byte]$dimension)
        $writer.Write([byte]0)
        $writer.Write([byte]0)
        $writer.Write([uint16]1)
        $writer.Write([uint16]32)
        $writer.Write([uint32]$frames[$index].Length)
        $writer.Write([uint32]$offset)
        $offset += $frames[$index].Length
    }
    foreach ($frame in $frames) {
        $writer.Write([byte[]]$frame)
    }
}
finally {
    $writer.Dispose()
}

Write-Output 'Generated the shared PNG, Android launcher icons and Windows ICO.'