$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$scriptsRoot = Join-Path $workspaceRoot 'scripts'

foreach ($script in Get-ChildItem $scriptsRoot -Filter '*.ps1' -Recurse) {
    $tokens = $null
    $syntaxErrors = $null
    $syntax = [Management.Automation.Language.Parser]::ParseFile($script.FullName, [ref]$tokens, [ref]$syntaxErrors)
    if ($syntaxErrors.Count -gt 0) { throw ($syntaxErrors | Out-String) }
    $imports = $syntax.FindAll({
        param($node)
        $node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'Join-Path'
    }, $true)
    foreach ($import in $imports) {
        $elements = $import.CommandElements
        if ($elements.Count -ne 3 -or $elements[1].Extent.Text -ne '$PSScriptRoot') { continue }
        $child = $elements[2]
        if ($child -isnot [Management.Automation.Language.StringConstantExpressionAst] -or $child.Value -notmatch '\.(ps1|json)$') { continue }
        $dependency = Join-Path $script.DirectoryName $child.Value
        if (-not (Test-Path $dependency -PathType Leaf)) { throw "Missing script dependency in $($script.Name): $($child.Value)" }
    }
}

$tasks = Get-Content (Join-Path $workspaceRoot '.vscode/tasks.json') -Raw | ConvertFrom-Json
foreach ($task in $tasks.tasks) {
    if ($task.command -match '\$\{workspaceFolder\}/(?<Script>scripts/[^'']+\.ps1)') {
        if (-not (Test-Path (Join-Path $workspaceRoot $Matches.Script) -PathType Leaf)) { throw "Missing script in task: $($task.label)" }
    }
}
$configuration = Get-Content (Join-Path $scriptsRoot 'build/ffmpeg.json') -Raw | ConvertFrom-Json
if (-not $configuration) { throw 'FFmpeg build configuration is missing.' }
if (Get-ChildItem $scriptsRoot -File -Filter '*.ps1') { throw 'PowerShell scripts must live in a responsibility subdirectory.' }

$referenceFiles = @(
    (Join-Path $workspaceRoot 'README.md'),
    (Join-Path $scriptsRoot 'README.md'),
    (Join-Path $workspaceRoot '.vscode/tasks.json')
)
$referenceFiles += @(Get-ChildItem (Join-Path $workspaceRoot 'docs') -Filter '*.md' -Recurse | ForEach-Object { $_.FullName })
$workflowDirectory = Join-Path $workspaceRoot '.github/workflows'
if (Test-Path $workflowDirectory) {
    $referenceFiles += @(Get-ChildItem $workflowDirectory -File | Where-Object { $_.Extension -in @('.yml', '.yaml') } | ForEach-Object { $_.FullName })
}
$staleReferences = Select-String -Path $referenceFiles -Pattern '\bscripts[/\\][^/\\\s''"`]+\.ps1'
if ($staleReferences) { throw "References to the old flat script layout remain:`n$($staleReferences | Out-String)" }
Write-Host 'Script syntax, local dependencies, task paths and documentation references passed.'