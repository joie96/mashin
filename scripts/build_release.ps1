param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('windows-x64', 'windows-arm64', 'android-arm64')]
    [ValidateNotNullOrEmpty()]
    [string[]]$Platforms,
    [switch]$Publish
)

$ErrorActionPreference = 'Stop'
$Platforms = @($Platforms | ForEach-Object { $_.ToLowerInvariant() } | Select-Object -Unique)
if ($Publish -and $Platforms.Count -ne 3) {
    throw 'Publishing requires all platforms: windows-x64, windows-arm64,android-arm64. Use the Build Release & Publish: All Platforms task.'
}
$workspaceRoot = Split-Path -Parent $PSScriptRoot
$metadata = Get-Content (Join-Path $workspaceRoot 'release.json') -Raw | ConvertFrom-Json
if ($metadata.version -cnotmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(dev|beta)\.([1-9][0-9]*))?$') {
    throw 'Version must be major.minor.patch, major.minor.patch-beta.N or major.minor.patch-dev.N.'
}
$major = [long]$Matches[1]
$minor = [long]$Matches[2]
$patch = [long]$Matches[3]
$channel = if ($Matches[4]) { $Matches[4] } else { 'stable' }
$sequence = if ($Matches[5]) { [long]$Matches[5] } else { 0 }
if ($major -gt 255 -or $minor -gt 255 -or $patch -gt 64 -or $sequence -gt 399) {
    throw 'Release version limits: major/minor <= 255, patch <= 64, prerelease sequence <= 399.'
}
$channelOrder = switch ($channel) {
    'dev' { $sequence }
    'beta' { 400 + $sequence }
    'stable' { 999 }
}
$buildNumber = (($major * 256 + $minor) * 65 + $patch) * 1000 + $channelOrder
if ($buildNumber -gt 2100000000) {
    throw 'Version exceeds the Android versionCode limit of 2100000000.'
}
$metadata | Add-Member -NotePropertyName BuildNumber -NotePropertyValue $buildNumber
$metadata | Add-Member -NotePropertyName WindowsBuildNumber -NotePropertyValue $channelOrder
$metadata | Add-Member -NotePropertyName MsiVersion -NotePropertyValue "$major.$minor.$($patch * 1000 + $channelOrder)"
$metadata | Add-Member -NotePropertyName Channel -NotePropertyValue $channel
$metadata | Add-Member -NotePropertyName Tag -NotePropertyValue "v$($metadata.version)"

# Windows compilation owns Flutter artifacts and CRT staging; MSI resources stay in package_windows.
function Invoke-WindowsReleaseBuild([string]$Architecture, [pscustomobject]$Release, [string]$OutputDirectory) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $installation = & $vswhere -latest -prerelease -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $installation) { throw 'Visual Studio C++ build tools are required.' }
    $cmake = Join-Path $installation 'Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe'
    $platform = "windows-$Architecture"
    $cmakeArchitecture = if ($Architecture -eq 'arm64') { 'ARM64' } else { 'x64' }
    $buildDirectory = Join-Path $workspaceRoot "build/windows/$Architecture"
    $flutterRoot = Split-Path -Parent (Split-Path -Parent (Get-Command flutter -ErrorAction Stop).Source)
    & flutter precache --windows | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Flutter Windows artifact download failed.' }
    & flutter build windows --release --config-only "--build-name=$($Release.Version)" "--build-number=$($Release.WindowsBuildNumber)" | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Flutter Windows configuration failed.' }
    $cache = Get-ChildItem "$workspaceRoot/build/windows/*/CMakeCache.txt" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $cache) { throw 'Flutter CMake configuration is missing.' }
    $generatorEntry = Select-String -Path $cache.FullName -Pattern '^CMAKE_GENERATOR:INTERNAL=(.+)$'
    if (-not $generatorEntry) { throw 'Could not determine the Visual Studio CMake generator.' }
    $generator = $generatorEntry.Matches[0].Groups[1].Value
    $engine = [IO.File]::ReadAllText((Join-Path $flutterRoot 'bin/cache/engine.stamp')).Trim()
    $downloadDirectory = Join-Path $workspaceRoot "build/native/flutter/$engine/$platform"
    $storage = if ($env:FLUTTER_STORAGE_BASE_URL) { $env:FLUTTER_STORAGE_BASE_URL.TrimEnd('/') } else { 'https://storage.googleapis.com' }
    $artifacts = @(
        @{ Directory = $platform; Archive = "$platform-debug/$platform-flutter.zip"; Required = 'flutter_windows.dll' },
        @{ Directory = $platform; Archive = "$platform/flutter-cpp-client-wrapper.zip"; Required = 'cpp_client_wrapper/include/flutter/flutter_engine.h' },
        @{ Directory = "$platform-release"; Archive = "$platform-release/$platform-flutter.zip"; Required = 'gen_snapshot.exe' }
    )
    foreach ($artifact in $artifacts) {
        $destination = Join-Path $flutterRoot "bin/cache/artifacts/engine/$($artifact.Directory)"
        $stamp = Join-Path $downloadDirectory "$($artifact.Archive -replace '/', '_').ready"
        if ((Test-Path $stamp) -and (Test-Path (Join-Path $destination $artifact.Required))) { continue }
        New-Item -ItemType Directory -Path $downloadDirectory -Force | Out-Null
        $archive = Join-Path $downloadDirectory ($artifact.Archive -replace '/', '_')
        Invoke-WebRequest -UseBasicParsing -Uri "$storage/flutter_infra_release/flutter/$engine/$($artifact.Archive)" -OutFile $archive
        Expand-Archive -Path $archive -DestinationPath $destination -Force
        if (-not (Test-Path (Join-Path $destination $artifact.Required))) { throw "Incomplete Flutter artifact: $($artifact.Archive)" }
        New-Item -ItemType File -Path $stamp -Force | Out-Null
    }
    & $cmake -S windows -B $buildDirectory -G $generator -A $cmakeArchitecture "-DFLUTTER_TARGET_PLATFORM=$platform" | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "CMake configuration failed for $platform." }
    & $cmake --build $buildDirectory --config Release --target INSTALL | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "Windows release build failed for $platform." }
    $runtimeDirectory = Get-ChildItem "$installation/VC/Redist/MSVC/*/$Architecture/Microsoft.VC*.CRT" -Directory | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $runtimeDirectory) { throw "Visual C++ redistributable DLLs are missing for $Architecture." }
    $runtimeNames = @('msvcp140.dll', 'vcruntime140.dll')
    if ($Architecture -eq 'x64') { $runtimeNames += 'vcruntime140_1.dll' }
    foreach ($runtime in Get-ChildItem $runtimeDirectory.FullName -Filter '*.dll') {
        $stagedRuntime = Join-Path $buildDirectory "runner/Release/$($runtime.Name)"
        if ($runtime.Name -in $runtimeNames) {
            Copy-Item $runtime.FullName $stagedRuntime -Force
        } elseif (Test-Path $stagedRuntime) {
            Remove-Item $stagedRuntime -Force
        }
    }
    foreach ($runtimeName in $runtimeNames) {
        if (-not (Test-Path (Join-Path $runtimeDirectory.FullName $runtimeName))) { throw "Missing Visual C++ runtime: $runtimeName" }
    }
    $bundleMetadata = @{ version = $Release.Version; buildNumber = $Release.BuildNumber; architecture = $Architecture } | ConvertTo-Json
    [IO.File]::WriteAllText((Join-Path $buildDirectory 'runner/Release/data/release.json'), $bundleMetadata)
    & (Join-Path $PSScriptRoot 'package_windows/package_windows.ps1') -Architecture $Architecture -Release $Release -OutputDirectory $OutputDirectory
}

# Publishing must distinguish a missing release from authentication or network failures.
function Assert-GitHubReleaseAbsent([string]$Repository, [string]$Tag) {
    $previousErrorPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $response = @(& gh api --include --silent "repos/$Repository/releases/tags/$Tag" 2>$null)
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousErrorPreference
    }
    if ($exitCode -eq 0) { throw "GitHub release $Tag already exists. Use a new version in release.json." }
    if ($response -match '^HTTP/\S+\s+404(?:\s|$)') {
        $releaseTags = @(& gh api "repos/$Repository/releases" --paginate --jq '.[].tag_name')
        if ($LASTEXITCODE -ne 0) { throw 'Could not check existing GitHub release drafts.' }
        if ($Tag -cin $releaseTags) { throw "GitHub release or draft $Tag already exists. Inspect it before retrying or use a new version." }
        return
    }
    throw "Could not check GitHub release $Tag. Check gh authentication, repository access and network connectivity."
}

# Release prerequisites and sequential platform builds share one metadata snapshot.
if ('android-arm64' -in $Platforms) {
    $keystore = Join-Path $HOME '.keys/mashin-release.jks'
    if (-not (Test-Path $keystore -PathType Leaf)) {
        throw 'Android release keystore is missing: $HOME/.keys/mashin-release.jks.'
    }
}
if ($Platforms -like 'windows-*') {
    if (-not (Test-Path (Join-Path $workspaceRoot 'build/tools/wix/wix.exe')) -and -not (Get-Command wix -ErrorAction SilentlyContinue)) {
        throw 'WiX 5.0.2 and WixToolset.UI.wixext/5.0.2 are required for MSI packaging; see docs/releases.md.'
    }
}
$storePassword = $null
$keyPassword = $null
$releaseAssets = [Collections.Generic.List[string]]::new()
Push-Location $workspaceRoot
try {
    if ($Publish) {
        if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw 'Publishing requires GitHub CLI. Install gh, then run gh auth login.' }
        if ($metadata.repository -cnotmatch '^https://github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+?)(?:\.git)?/?$') {
            throw 'Publishing requires a GitHub repository URL in release.json.'
        }
        $repository = "$($Matches[1])/$($Matches[2])"
        & gh api "repos/$repository" --silent | Out-Host
        if ($LASTEXITCODE -ne 0) { throw 'Cannot access the GitHub repository. Run gh auth login and check repository access.' }
        Assert-GitHubReleaseAbsent -Repository $repository -Tag $metadata.Tag
        $publishCommit = & git rev-parse HEAD
        if ($LASTEXITCODE -ne 0) { throw 'Could not resolve the source commit.' }
        $changes = & git status --porcelain --untracked-files=normal
        if ($LASTEXITCODE -ne 0) { throw 'Could not determine working tree state.' }
        if ($changes) { throw 'Publishing requires a clean working tree. Commit the intended changes before publishing.' }
        $localTag = @(& git tag --list $metadata.Tag)
        if ($LASTEXITCODE -ne 0) { throw 'Could not inspect local tags.' }
        if ($localTag) {
            $tagCommit = & git rev-parse --verify "refs/tags/$($metadata.Tag)^{}"
            if ($LASTEXITCODE -ne 0 -or $tagCommit -cne $publishCommit) { throw "Tag $($metadata.Tag) already points to a different commit. Use a new release version." }
        }
        $remoteTags = @(& git ls-remote --tags $metadata.repository "refs/tags/$($metadata.Tag)" "refs/tags/$($metadata.Tag)^{}")
        if ($LASTEXITCODE -ne 0) { throw 'Could not inspect remote tags.' }
        $hasRemoteTag = $remoteTags.Count -gt 0
        if ($hasRemoteTag) {
            $remoteCommitLine = @($remoteTags | Where-Object { $_.EndsWith('^{}') })
            if (-not $remoteCommitLine) { $remoteCommitLine = $remoteTags }
            $remoteCommit = ($remoteCommitLine[0] -split '\s+')[0]
            if ($remoteCommit -cne $publishCommit) { throw "Remote tag $($metadata.Tag) points to a different commit. Use a new release version." }
            if (-not $localTag) {
                & git fetch --no-tags $metadata.repository "refs/tags/$($metadata.Tag):refs/tags/$($metadata.Tag)" | Out-Host
                if ($LASTEXITCODE -ne 0) { throw 'Could not fetch the matching release tag.' }
            }
        } elseif (-not $localTag) {
            & git tag --annotate $metadata.Tag --message "Mashin $($metadata.Version)" $publishCommit
            if ($LASTEXITCODE -ne 0) { throw 'Could not create the release tag.' }
        }
    }
    if ('android-arm64' -in $Platforms) {
        $storePassword = Read-Host 'Android keystore password' -AsSecureString
        if ($storePassword.Length -eq 0) { throw 'Android keystore password must not be empty.' }
        $keyPassword = Read-Host 'Android key password (Enter uses the keystore password)' -AsSecureString
        if ($keyPassword.Length -eq 0) { $keyPassword.Dispose(); $keyPassword = $storePassword.Copy() }
    }
    & flutter pub get
    if ($LASTEXITCODE -ne 0) { throw 'Flutter dependency resolution failed.' }
    foreach ($platform in $Platforms) {
        Write-Host "Building $($metadata.Version)+$($metadata.BuildNumber) for $platform"
        $outputDirectory = Join-Path $workspaceRoot "build/releases/$($metadata.Tag)/$platform"
        $nativeTarget = switch ($platform) {
            'windows-x64' { 'WindowsX64' }
            'windows-arm64' { 'WindowsArm64' }
            'android-arm64' { 'AndroidArm64' }
        }
        & (Join-Path $PSScriptRoot 'build_mashin_engine/build_mashin_engine.ps1') -Profile Release -PlatformSelection $nativeTarget
        if ($LASTEXITCODE -ne 0) { throw "Native release build failed for $platform." }
        New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
        if ($platform.StartsWith('windows-')) {
            $architecture = $platform.Substring('windows-'.Length)
            $artifacts = @(Invoke-WindowsReleaseBuild -Architecture $architecture -Release $metadata -OutputDirectory $outputDirectory)
        } else {
            $signingEnvironment = @{}
            foreach ($name in @('MASHIN_ANDROID_KEYSTORE', 'MASHIN_ANDROID_KEYSTORE_PASSWORD', 'MASHIN_ANDROID_KEY_ALIAS', 'MASHIN_ANDROID_KEY_PASSWORD')) {
                $signingEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
            }
            $storePointer = [IntPtr]::Zero
            $keyPointer = [IntPtr]::Zero
            try {
                $storePointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($storePassword)
                $keyPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($keyPassword)
                $env:MASHIN_ANDROID_KEYSTORE = $keystore
                $env:MASHIN_ANDROID_KEYSTORE_PASSWORD = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($storePointer)
                $env:MASHIN_ANDROID_KEY_ALIAS = 'mashin'
                $env:MASHIN_ANDROID_KEY_PASSWORD = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($keyPointer)
                & flutter build apk --release --target-platform android-arm64 "--build-name=$($metadata.Version)" "--build-number=$($metadata.BuildNumber)"
                if ($LASTEXITCODE -ne 0) { throw 'Android release build failed.' }
            } finally {
                foreach ($name in $signingEnvironment.Keys) { [Environment]::SetEnvironmentVariable($name, $signingEnvironment[$name]) }
                if ($storePointer -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($storePointer) }
                if ($keyPointer -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($keyPointer) }
            }
            $artifact = Join-Path $outputDirectory "Mashin-$($metadata.Version)-android-arm64.apk"
            Copy-Item (Join-Path $workspaceRoot 'build/app/outputs/flutter-apk/app-release.apk') $artifact -Force
            $artifacts = @($artifact)
        }
        $commit = & git rev-parse HEAD
        if ($LASTEXITCODE -ne 0) { throw 'Could not determine release commit.' }
        $changes = & git status --porcelain --untracked-files=normal
        if ($LASTEXITCODE -ne 0) { throw 'Could not determine working tree state.' }
        foreach ($artifact in $artifacts) {
            $artifact = [string]$artifact
            $checksum = (Get-FileHash $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
            [IO.File]::WriteAllText("$artifact.sha256", "$checksum  $([IO.Path]::GetFileName($artifact))`n")
            $manifest = [ordered]@{
                version = $metadata.Version
                buildNumber = $metadata.BuildNumber
                channel = $metadata.Channel
                target = $platform
                commit = $commit
                isDirty = [bool]$changes
                file = [IO.Path]::GetFileName($artifact)
                sha256 = $checksum
                size = (Get-Item $artifact).Length
                isSigned = if ($platform.StartsWith('windows-')) { (Get-AuthenticodeSignature $artifact).Status -eq 'Valid' } else { $true }
            }
            [IO.File]::WriteAllText("$artifact.json", ($manifest | ConvertTo-Json))
            $releaseAssets.Add($artifact)
            $releaseAssets.Add("$artifact.sha256")
            Write-Host "Release artifact: $artifact"
        }
    }
    if ($Publish) {
        $builtCommit = & git rev-parse HEAD
        if ($LASTEXITCODE -ne 0 -or $builtCommit -cne $publishCommit) { throw 'Source commit changed during the build. Nothing was published.' }
        $changes = & git status --porcelain --untracked-files=normal
        if ($LASTEXITCODE -ne 0 -or $changes) { throw 'Working tree changed during the build. Nothing was published.' }
        Assert-GitHubReleaseAbsent -Repository $repository -Tag $metadata.Tag
        if (-not $hasRemoteTag) {
            & git push $metadata.repository "refs/tags/$($metadata.Tag):refs/tags/$($metadata.Tag)" | Out-Host
            if ($LASTEXITCODE -ne 0) { throw 'Could not push the release tag. Nothing was published.' }
        }
        $releaseArguments = @('release', 'create', $metadata.Tag) + $releaseAssets.ToArray() + @('--repo', $repository, '--verify-tag', '--draft', '--title', "Mashin $($metadata.Version)", '--generate-notes')
        if ($metadata.Channel -ne 'stable') { $releaseArguments += '--prerelease' }
        & gh @releaseArguments | Out-Host
        if ($LASTEXITCODE -ne 0) { throw 'Release upload failed. A draft may remain on GitHub; inspect it before retrying.' }
        & gh release edit $metadata.Tag --repo $repository --draft=false | Out-Host
        if ($LASTEXITCODE -ne 0) { throw 'Could not publish the uploaded draft. Inspect the draft release on GitHub.' }
        Write-Host "Published $($metadata.Tag) to https://github.com/$repository/releases/tag/$($metadata.Tag)"
    }
} finally {
    if ($null -ne $storePassword) { $storePassword.Dispose() }
    if ($null -ne $keyPassword) { $keyPassword.Dispose() }
    Pop-Location
}