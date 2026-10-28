# Project Automation

All entry points are PowerShell 5.1-compatible and resolve project paths from
their own location. Run them locally or call the same scripts from GitHub Actions.
Keep YAML focused on runner/toolchain setup, job dependencies, secrets and artifacts.

```text
.github/workflows/
  build.yml                   Manual build of all three platforms; no publishing
scripts/
  build/
    build_ffmpeg.ps1           FFmpeg build and license/source staging
    ffmpeg.json                FFmpeg build configuration
    build_mashin_engine.ps1    Native build/test and platform staging
    build_windows_app.ps1      Flutter Windows bundle and CRT runtime
  release/
    get_release_metadata.ps1   Version/channel/installer metadata
    setup_release_tools.ps1    Local .NET and WiX setup
    package_windows_app.ps1    Validated bundle to MSI
    build_release.ps1          Native + Flutter + packaging + checksums
    collect_release_artifacts.ps1  Verify and collect the complete shared build
  tests/
    test_script_layout.ps1    Syntax, dependencies and reference checks
    test_release.ps1          Version rules and optional MSI fixture tests
```

Installer source stays in `installer/windows`; release identity stays in the root
`release.json`. Generated tools, bundles, test fixtures and release assets stay
under the ignored `build` directory; WiX's extension cache stays under `.wix`.
Flutter tests remain in `test`, Rust tests next to their modules, and Android
host tests in their standard Gradle source tree. `scripts/tests` is for testing
project automation, not for moving application tests away from their toolchains.

## Local Entry Points

```powershell
./scripts/build/build_mashin_engine.ps1 -Profile Debug -PlatformSelection WindowsArm64
./scripts/build/build_mashin_engine.ps1 -Profile Debug -PlatformSelection WindowsArm64 -Action Test
./scripts/release/build_release.ps1 -Target windows-arm64
./scripts/tests/test_script_layout.ps1
./scripts/tests/test_release.ps1
./scripts/release/setup_release_tools.ps1
./scripts/tests/test_release.ps1 -PackageFixture
```

See [Release Builds](../docs/releases.md) for all targets and signing requirements.
Do not build different targets concurrently in one checkout because native staging
and Flutter configuration are shared. Do not retain forwarding scripts at the old
paths; update tasks, workflows, documentation and callers together.

## GitHub Actions Boundaries

Tests are run locally before committing; no GitHub test workflow is configured.
All GitHub builds, including development builds, must be started manually
through `workflow_dispatch` (Run workflow). Do not add push, pull request, tag or
scheduled triggers.

`.github/workflows/build.yml` accepts one shared version and build number, falling
back to `release.json` when empty. Every run builds Windows x64, Windows ARM64 and
Android ARM64 in separate checkouts at the same resolved commit. There is no target
selector. Toolchain versions are pinned in the workflow; the Android NDK version
comes from `scripts/build/ffmpeg.json`.

`scripts/release/collect_release_artifacts.ps1` verifies that all three manifests
have the expected version, build number, channel and clean source commit. It also
checks file names, sizes and SHA-256 files before producing the combined download.
It is a packaging integrity check, not an app test or an authenticated update index.
Only a completely successful platform matrix produces the combined artifact.

No tests, releases, tags or Pages updates are run by this workflow. Windows remains
unsigned; Android requires the four repository secrets documented in the release
guide. Publishing will be a separate, explicit decision. Restrict future write
permissions and publication secrets to the jobs that actually need them.

Add `scripts/deploy` only when implementing publication or the signed Pages update
index. Put upload/index logic there, not in build scripts or copied YAML commands.
This keeps building and testing usable without GitHub credentials.