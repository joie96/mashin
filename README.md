# Mashin

A [Music Assistant](https://www.music-assistant.io/) client for Windows and Android,
built with Flutter and a Rust playback engine.

Listen locally in lossless FLAC or Opus, or control your Music Assistant players.
Android supports background playback and Android Auto media controls.
Streaming only, no offline downloads.

## Requirements

A Music Assistant server and account. Additional server requirements:

- Local playback: the Mashin provider from my fork's [feature/mashin_stream](https://github.com/joie96/music-assistant-server/tree/feature/mashin_stream) branch.
- Recommendations: my [feature/listenbrainz_recommendations](https://github.com/joie96/music-assistant-server/tree/feature/listenbrainz_recommendations) branch and a configured [ListenBrainz scrobbler](https://github.com/joie96/music-assistant-server/tree/feature/listenbrainz_recommendations/music_assistant/providers/listenbrainz_scrobble).

Supported targets: Windows x64/ARM64 and Android ARM64.

## Building

See the [build guide](scripts/README.md) for setup and release builds.

## Architecture

Catalog browsing requires an online Music Assistant session. Android media controls
and Android Auto use the same online catalog and playback session.
The [cache replacement contract](docs/cache_replacement_contract.md) separates
removal of the previous cache from any future implementation.

## Local Data

Application data lives in `%LOCALAPPDATA%/Mashin` on Windows and
`Android/data/com.joie96.mashin/files` on Android. The cache contains
`Cache/metadata.sqlite3` and `Cache/images/`, with image files and their
`index.sqlite3` database. SQLite can also create temporary WAL and SHM files.
Server bindings remain in `Cache/server.json`, independently of the databases.
Settings and logs remain outside the cache.
For now, the entire cache is cleared before its first use after an app version
change, including images and server bindings. The last cache app version is
stored in the existing configuration, without a separate marker file. The first
run without a recorded version also clears the cache; subsequent starts of the
same version keep it.

## FFmpeg

Mashin dynamically links FFmpeg under LGPL-2.1-or-later. App packages include
the corresponding source, build scripts and license notices; preserve these
when redistributing.
