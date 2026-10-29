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

## FFmpeg

Mashin dynamically links FFmpeg under LGPL-2.1-or-later. App packages include
the corresponding source, build scripts and license notices; preserve these
when redistributing.
