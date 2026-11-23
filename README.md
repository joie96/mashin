# Mashin

A [Music Assistant](https://www.music-assistant.io/) client for Windows and Android,
built with Flutter and a Rust playback engine.

Listen locally in lossless FLAC or Opus, or control your Music Assistant players.
Android supports background playback and Android Auto media controls.
Streaming only, no offline downloads.

## Requirements

A Music Assistant server and account. Additional server requirements:

- Local playback: the Mashin provider from my fork's [feature/mashin_stream](https://github.com/joie96/music-assistant-server/tree/feature/mashin_stream) branch.
- Recommendations: a configured Last.fm Recommendations Enhanced provider (`lastfm_recommendations_enhanced`) on the server, with your own Last.fm API key and the Music Assistant user selected in its setup. No Last.fm login or ListenBrainz scrobbler is required. Personal recommendations appear on Home; charts, genre discovery, artist radios, moods and decades appear on Explore.

Supported targets: Windows x64/ARM64 and Android ARM64.

## Android Auto

Home and Explore follow the app's recommendation sections, excluding
genre-specific artist and album rows. Playlists includes all library playlists,
not only owned private playlists. Mood playlists use locally rendered covers
with the app's symbols and dark color palette.

The additional Android Auto drawables are downloaded from Google's
[Material Symbols](https://github.com/google/material-design-icons) under
Apache-2.0: Outlined, weight 400, optical size 48, filled for mood covers only.
Resource names follow Google's symbol names, with `_fill1` for filled variants;
`public` is named `globe` to avoid the reserved Java keyword.
Their fixed foreground color matches the existing Android Auto drawables.

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
