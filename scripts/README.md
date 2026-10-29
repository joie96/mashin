# Project Automation

Local builds run on Windows through PowerShell 5.1. For a new release, change only
`version` in [release.json](../release.json); build numbers are derived automatically.

Versions follow `major.minor.patch[-dev.N|-beta.N]`, ordered `dev < beta < stable`.
Limits: major/minor 0-255, patch 0-64, N 1-399. Android's build number is
`((major * 256 + minor) * 65 + patch) * 1000 + channelOrder` (maximum 2100000000).
`channelOrder` is N for dev, 400+N for beta and 999 for stable. Windows uses
`major.minor.patch.channelOrder` for file versions and
`major.minor.(patch * 1000 + channelOrder)` for MSI upgrades.

## Prerequisites

- Flutter, Rust with the selected target toolchains, Git for Windows and LLVM
  (`clang-cl` on `PATH`). Set `FFMPEG_BASH` if using a different MSYS2 Bash.
- Windows: Visual Studio C++ build tools with CMake and the selected architecture's
  toolchain/runtime, plus WiX 5.0.2 and `WixToolset.UI.wixext/5.0.2`.
- Android: Android SDK, a compatible JDK, `cargo-ndk` and the NDK version specified
  in [ffmpeg.json](build_ffmpeg/ffmpeg.json). The SDK is located through
  `ANDROID_HOME`, falling back to `%LOCALAPPDATA%/Android/Sdk`.

Toolchains and signing keys must be installed beforehand. Build dependencies such
as FFmpeg sources and Flutter artifacts are downloaded and cached as needed.

## Build

Run from the repository root, selecting one or more platforms:

```powershell
./scripts/build_release.ps1 -Platforms windows-arm64
./scripts/build_release.ps1 -Platforms windows-x64,android-arm64
./scripts/build_release.ps1 -Platforms windows-x64,windows-arm64,android-arm64
./scripts/build_release.ps1 -Platforms windows-x64,windows-arm64,android-arm64 -Publish
```

Equivalent VS Code tasks: `Build Release: <platform>`, `Build Release: All Platforms`
and `Build Release & Publish: All Platforms`.

Android signing uses `$HOME/.keys/mashin-release.jks`, alias `mashin`. Passwords
are prompted securely; an empty key password reuses the keystore password.
Credentials are passed only through the temporary build environment.

Outputs: `build/releases/v<VERSION>/<PLATFORM>/`, containing Windows MSI/portable
ZIP or Android APK, SHA-256 checksums and JSON metadata. Builds run sequentially
and stop on failure. Do not run builds concurrently in one checkout.

## Publish

`-Publish` requires all three platforms, a clean working tree, GitHub CLI
authentication (`gh auth login`) and Git push access. Existing releases/drafts and
conflicting tags are rejected. The script tags the source commit, builds, pushes
the tag and uploads packages/checksums to a draft before publishing. JSON metadata
stays local. Without `-Publish`, no tags or GitHub releases are created.

## Scripts

- [build_release.ps1](build_release.ps1): builds and optionally publishes releases.
- [build_mashin_engine.ps1](build_mashin_engine/build_mashin_engine.ps1): builds or
  tests the native engine (`-Action Test`, optional `-TestFilter`; Windows targets only).
- [build_ffmpeg.ps1](build_ffmpeg/build_ffmpeg.ps1): builds FFmpeg and stages licenses/sources.
- [package_windows.ps1](package_windows/package_windows.ps1): validates and packages Windows bundles.