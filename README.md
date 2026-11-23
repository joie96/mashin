# Mashin

A [Music Assistant](https://www.music-assistant.io/) client for Windows and Android.

Listen locally in lossless FLAC or Opus, or control your Music Assistant players.
Android supports background playback and Android Auto media controls.
Streaming only, no offline downloads.

## Requirements

A Music Assistant server and account. Additional server requirements:

- Local playback: the [Mashin provider](https://github.com/joie96/music-assistant-server/tree/feature/mashin_stream) from my Music Assistant fork.
- Recommendations: the [Last.fm Recommendations Enhanced provider](https://github.com/joie96/music-assistant-server/tree/feature/lastfm_recommendations_enhanced) from my Music Assistant fork.

I run Music Assistant in Docker and simply mount the two provider folders into
the container.

For recommendations, configure the provider with your own Last.fm API key and
select your Music Assistant user. No Last.fm login or ListenBrainz scrobbler is
required. Personal recommendations appear on Home; charts, genre discovery,
artist radios, moods and decades appear on Explore.

Supported targets: Windows x64/ARM64 and Android ARM64.

## Android Auto

Browse Home and Explore recommendations, access your library playlists and
control playback from Android Auto. Mood playlists have the same recognizable
symbols and colors as in the app. A connection to your Music Assistant server
is required.

## Local Data

Application data lives in `%LOCALAPPDATA%/Mashin` on Windows and
`Android/data/com.joie96.mashin/files` on Android. Cached images and library
information are cleared on the first launch after an app update. Your settings
are kept.

## FFmpeg

Mashin uses FFmpeg under LGPL-2.1-or-later. App packages include the corresponding
source and license notices; preserve these when redistributing.
