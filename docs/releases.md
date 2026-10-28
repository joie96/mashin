# Release Builds

This is the local packaging foundation for GitHub-hosted releases. It does not
publish releases, create tags, or add an in-app update check. Windows packages
are currently unsigned; Android release builds require your own signing key.

## Versions And Identity

`release.json` is authoritative for release builds. The wrapper passes its version
and build number to Flutter explicitly, overriding the development default in
`pubspec.yaml`. Optional `-Version` and `-BuildNumber` arguments support later CI
manual workflow inputs without modifying source files.

- Stable: `0.1.0`, tag `v0.1.0`.
- Beta: `0.1.0-beta.1`, tag `v0.1.0-beta.1`.
- Development: `0.1.0-dev.42`, tag `v0.1.0-dev.42`.
- Android application ID: `com.joie96.mashin`, shared by all channels.
- MSI upgrade code: keep the configured GUID unchanged across releases and architectures.

Increase Android's build number for **every** distributed build across all
channels. Keep the signing identity unchanged. Android refuses lower version
codes and APKs signed with a different key. Back up the private key securely
outside the repository; losing it prevents updates of existing installations.

MSI compares three numeric fields, so `major.minor.patch-channel.N` maps to
`major.minor.(patch * 1000 + channelOrder)`: dev N is N, beta N is 400 + N,
stable is 999. For example, `0.1.0-beta.1` becomes `0.1.401`. This preserves
dev < beta < stable < next patch. Limits are major/minor 0..255, patch 0..64,
prerelease sequence 1..399, build number 1..65535. Other suffixes are rejected.

The MSI product identity is deterministic per version and architecture. Its
version does not include the build number: never distribute changed Windows
contents under an existing version. Use a new prerelease sequence or version.
Lower MSI versions are blocked, including a channel switch to an older version.
Different architectures of the same version cannot be installed side by side.

## Prerequisites

Run the commands below from the repository root in PowerShell. Use the existing
Flutter, Rust, Visual Studio C++/ARM64, FFmpeg and Android NDK setup required by
the native build scripts. Install the matching Visual C++ redistributable files
with the Visual Studio build tools; release builds copy their CRT DLLs beside
the executable. No system-wide VC++ installation is required on the target PC.

The first Windows release build downloads .NET SDK 8.0.427 and WiX 5.0.2 into
the ignored `build/tools` directory. The SDK archive is checked against the
SHA-512 from Microsoft's official release metadata. WiX and its matching UI
extension come from NuGet; WiX caches the extension in the ignored `.wix` directory.
These versions are pinned in `scripts/release/setup_release_tools.ps1`.
The scripts do not request elevation or install the Mashin MSI.

## Build Commands

```powershell
./scripts/release/build_release.ps1 -Target windows-arm64
./scripts/release/build_release.ps1 -Target windows-x64
./scripts/release/build_release.ps1 -Target android-arm64

# Explicit inputs for a manual build:
./scripts/release/build_release.ps1 -Target windows-arm64 -Version 0.1.0-beta.2 -BuildNumber 2
```

Equivalent VS Code tasks are named `Release Build: Windows arm64 MSI`,
`Release Build: Windows x64 MSI`, and `Release Build: Android APK`.
Each entry point builds the native engine, then Flutter, then the package.
Do not build targets concurrently in one checkout: native staging and Flutter
configuration are shared. GitHub build jobs use separate checkouts.

Artifacts are written to `build/releases/v<VERSION>/<TARGET>/`:

- `Mashin-<VERSION>-windows-<ARCH>.msi` or `Mashin-<VERSION>-android-arm64.apk`.
- A matching `.sha256` file.
- `artifact.json` with version, build number, channel, target, commit, dirty-tree
  flag, file name, size, hash and signature status.

`artifact.json` is local build evidence, not a trusted or signed update index.
Local builds may have a dirty tree, recorded explicitly; publish only reviewed
artifacts from a clean tagged commit. Generated WiX sources and intermediates in
the output directory are not release assets.

## Manual GitHub Builds

The workflow `.github/workflows/build.yml` is named **Build All Platforms**.
Commit and push the local changes to the default branch (`main`) before expecting
the Run workflow button to appear. No workflow has been pushed or run by this
local setup, and GitHub repository settings have not been changed.

1. Configure the four signing secrets below in repository **Settings > Secrets
  and variables > Actions > Repository secrets**.
2. Open **Actions > Build All Platforms > Run workflow** and select the source
  branch or tag. It must contain this workflow and the release scripts.
3. Enter one version and one build number, or leave either field empty to use its
  value from `release.json` at the selected source commit.
4. Start the workflow. Every run builds Windows x64, Windows ARM64 and Android
  ARM64. There is no target selector and no per-platform version override.
5. After all jobs succeed, download `Mashin-v<VERSION>-all-platforms` from the
  run's **Artifacts** section. It contains two MSIs, one signed APK, checksums,
  each platform's metadata and a shared `build.json` manifest.

The preparation job resolves a single commit, version, channel and build number.
All target jobs receive those exact values, even if the selected branch advances
while the workflow runs. The collector checks their identities and hashes and
rejects missing, dirty, mismatched or damaged packages. The combined artifact is
kept for 30 days; intermediate per-platform artifacts are kept for 7 days and may
exist even when another platform fails. Those are not a complete shared build.

There are no push, PR, tag or scheduled triggers, including for development
versions. The workflow runs no app tests and creates no release, tag or Pages
deployment. Build-time version, signing and integrity checks still apply.
Windows packages are currently unsigned. The workflow needs `windows-2022` x64
and `windows-11-arm` ARM64 hosted runners to be available for the repository.

### GitHub Android Secrets

| Repository Secret | Value |
| --- | --- |
| `MASHIN_ANDROID_KEYSTORE_BASE64` | Base64 encoding of the existing release keystore |
| `MASHIN_ANDROID_KEYSTORE_PASSWORD` | Keystore password |
| `MASHIN_ANDROID_KEY_ALIAS` | Signing-key alias |
| `MASHIN_ANDROID_KEY_PASSWORD` | Signing-key password |

Use the permanent production identity, including for dev and beta builds. Do not
create a new key for each build. Base64 is only an encoding, not encryption; treat
it as private key material. To transfer an existing key without printing it into
terminal output, run this yourself on your machine, then paste directly into the
GitHub secret field and clear your clipboard:

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes('C:/private/mashin-release.jks')) | Set-Clipboard
```

Never paste secret values into chat. If no permanent key exists yet, create and
back one up before configuring these secrets; the workflow deliberately does not
generate one. Missing secrets stop preparation before any platform build starts.
Incorrect passwords or an invalid keystore fail Android signing and prevent the
combined build artifact. The decoded key exists only in the Android runner's
temporary directory during the build and is deleted in `finally`; it is never
uploaded as an artifact. Secrets are not passed to the Windows build jobs.

## Android Signing

Debug builds continue using the debug key. Release builds never fall back to it.
Configure the ignored `android/key.properties` using the property names from
`android/key.properties.example`, or provide these environment variables:

| Environment Variable | Property |
| --- | --- |
| `MASHIN_ANDROID_KEYSTORE` | `storeFile` |
| `MASHIN_ANDROID_KEYSTORE_PASSWORD` | `storePassword` |
| `MASHIN_ANDROID_KEY_ALIAS` | `keyAlias` |
| `MASHIN_ANDROID_KEY_PASSWORD` | `keyPassword` |

Environment variables take precedence. `storeFile` may be absolute or relative
to the `android` directory; use forward slashes in the properties file. Never
put passwords in build arguments, source files, logs or chat. Keystores and the
real properties file are ignored by Git. Keep private keys outside the checkout.
Gradle validates the release credentials and fails when the key is missing or
invalid. Existing debug-signed APK installations cannot be updated in place
with the production key; changing that identity requires a reinstall and may
remove Android app data.

## Windows Installation And Updates

The MSI installs only for the current user, at `%LOCALAPPDATA%\Mashin\App`,
with a Start menu shortcut. Bundled files include Flutter, the Rust engine,
VC++ runtime, FFmpeg DLLs and their license/source materials.

An older installed release is replaced through MSI MajorUpgrade. Its primary
action is labeled `Aktualisieren`. Removal of the previous release runs inside
the installation transaction so Windows Installer can roll it back if the new
installation fails. Reopening the same MSI offers repair and removal; a newer
installed version blocks downgrades.

Only installer-owned files and empty program directories are removed. The MSI
does not manage sibling `config.json`, `Logs`, secure credentials or future
cache data. Unknown files are not recursively deleted. Close Mashin before
manual installation; an in-app update flow that awaits native shutdown does
not exist yet. Windows Installer may show Files In Use or require a restart
when running processes keep DLLs open.

Before public distribution, add Windows Authenticode signing to executables,
DLLs and finally the MSI, then regenerate hashes and artifact metadata. Keep
certificates and signing secrets in a secure signing service or CI secret store.
Unsigned local packages can trigger Windows security warnings.

## Validation

```powershell
./scripts/tests/test_script_layout.ps1
./scripts/tests/test_release.ps1
./scripts/release/setup_release_tools.ps1
./scripts/tests/test_release.ps1 -PackageFixture
```

The fixture test compiles a deliberately non-runnable ARM64 MSI and inspects its
tables without installing it. It checks version ordering, invalid inputs, bundle
metadata, user-data exclusion, per-user directories, update labels and transactional
upgrade scheduling. Never install anything under `build/release-tests`.

Before release, test real x64 and ARM64 packages in clean disposable Windows
environments: first install, version A to B, downgrade rejection, repair, removal,
running-app handling and preservation of settings/credentials/logs. Test a signed
Android APK update using the same real key and a higher build number. Local
compilation and MSI table checks do not replace these installation tests.

## Remaining GitHub And App Work

The [automation layout](../scripts/README.md) separates builds, release packaging
and script tests. Tests are run locally before committing; no automated GitHub
test workflow is planned. All GitHub builds, including development builds, will
be started manually using `workflow_dispatch` (Run workflow). Pushes, pull requests
and tags must not start builds or tests automatically.

Keep `main` for development and immutable version tags to identify releases, not
to trigger builds. The manual workflow already produces all three platform
artifacts with one shared version. Commit-based notes from the correct preceding
channel tag and an optional draft release are future additions;
publication remains an explicit decision after reviewing the artifacts and notes.
Never embed a GitHub token in the app.

A later signed channel index on GitHub Pages can point to GitHub Release assets.
The app still needs channel settings, startup checking, authenticated metadata
verification, commit-based changelog display and installer handoff. Windows must
await native shutdown before handing over to MSI; Android must use its package
installer with user confirmation and the required permission flow. A SHA-256 file
alone is not authentication. No Pages index, updater UI or publishing workflow is
implemented by this local packaging change.