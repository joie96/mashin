# Project Automation

Release builds run locally through one PowerShell 5.1-compatible entry point.
Paths are resolved from the script location, not the current working directory.

```text
scripts/
  build_release.ps1           Build selected platforms; optionally tag and publish
  build_ffmpeg/
    build_ffmpeg.ps1           FFmpeg build and license/source staging
    ffmpeg.json                FFmpeg build configuration
  build_mashin_engine/
    build_mashin_engine.ps1    Native build/test and platform staging
  package_windows/
    package_windows.ps1        Validated bundle to MSI and portable ZIP
    Mashin.wxs                 MSI package definition
    Mashin.de-DE.wxl           German MSI localization
```

`build_release.ps1` reads version and build number once from `release.json`.
It resolves Flutter dependencies and builds the requested platforms sequentially,
ignoring duplicate selections. There are no version overrides or setup scripts.
The release script reads and validates the shared configuration directly and
passes that snapshot to Windows packaging. MSI version mapping and
limits belong to Windows packaging, not a separate metadata helper.

Each build area owns its implementation and adjacent resources: FFmpeg owns its
configuration, the native engine calls FFmpeg through its sibling directory,
and Windows packaging owns its WXS/WXL files. Windows app compilation and CRT
staging live directly in the release script; there is no separate Windows build
script. Debug native tasks and all existing debug launch configurations remain
unchanged.

## Usage

```powershell
./scripts/build_release.ps1 -Platforms windows-arm64
./scripts/build_release.ps1 -Platforms windows-x64,android-arm64
./scripts/build_release.ps1 -Platforms windows-x64,windows-arm64,android-arm64
./scripts/build_release.ps1 -Platforms windows-x64,windows-arm64,android-arm64 -Publish
```

VS Code provides `Build Release: <platform>` for Android arm64, Windows x64,
Windows arm64 and All Platforms. There is only one publishing task:
`Build Release & Publish: All Platforms`. The script rejects `-Publish` unless
all three platforms are selected, before passwords or build/publish actions. The terminal is
focused so Android passwords can be entered directly and invisibly.
Do not run separate builds concurrently in one checkout: native staging and Flutter
configuration are shared. Failure stops the run; already completed packages remain
in their own `build/releases/v<VERSION>/<PLATFORM>` directories.

Each Windows target produces both an MSI and a `-portable.zip` containing the
application files directly, without an installer. Each package has its own
`.sha256` and `.json` sidecars. Both Windows formats are uploaded when publishing;
JSON sidecars remain local build evidence.

See [Release Builds](../docs/releases.md) for local prerequisites and Android signing.
Without `-Publish`, the build does not create tags or contact GitHub Releases.
With `-Publish`, it checks for existing releases/drafts and conflicting tags before
building, requires a clean working tree, creates or reuses the matching version
tag, builds, pushes the tag, uploads packages/checksums to a draft and publishes it
only after all uploads succeed. Publication always includes all three platforms
under one shared version. The same version cannot be published twice.
GitHub CLI (`gh auth login`) and Git push access are required.
No publish command is executed just by configuring the task.

Android builds use `$HOME/.keys/mashin-release.jks`, alias `mashin`, and ask for
both passwords using secure terminal prompts. Empty key password means reuse
the keystore password. Credentials are only passed to the APK build through its
temporary environment, restored in `finally`, and are never saved in arguments
or project files.

The build does not install toolchains, generate signing keys or install the MSI.
FFmpeg and matching Flutter engine artifacts are
downloaded as needed by the compilation process and cached under `build` or the
Flutter SDK cache. Existing locally cached WiX/.NET tools remain usable.

Installer source stays beside Windows packaging in `scripts/package_windows`.
Generated bundles and release assets
stay under ignored `build`; the existing local WiX extension cache stays under `.wix`.
There are no GitHub Actions workflows or script-test entry points. Application tests
remain in their existing Flutter, Rust and Android locations.