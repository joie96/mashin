# Mashin

Mashin is a Music Assistant client for Windows and Android. Browse your music,
manage playlists and listen on your device or control another Music Assistant
player.

## Features

- Browse your Music Assistant library and recommendations.
- Save favorites and create your own playlists.
- Play music locally or control an existing Music Assistant player.
- Manage the playback queue: play now, play next, append, reorder and remove tracks.
- Use shuffle, repeat and radio through Music Assistant.
- Choose lossless FLAC or Opus at 96, 128, 192 or 256 kb/s for local playback.
- See playback progress and buffered audio ahead of the current position.

Favorites use Music Assistant's normal per-user favorites. Personal playlists are
private Music Assistant playlists owned by your signed-in account. Favorites and
playlists are managed exclusively through Music Assistant's official library APIs.
Favorite changes appear immediately and synchronize in memory, independently per
item. Pending changes are not persisted and are lost when the app exits.
Scrobbling is handled by Music Assistant and its configured integrations.

## Getting Started

Mashin requires a running Music Assistant server and a Music Assistant account.
Local playback also requires server support for the Mashin streaming endpoints.

Sign in to your server, then choose local playback to listen on your device or
select an existing Music Assistant player for remote control. Your music sources
and available content come from the services configured in Music Assistant.

## Playback

Local playback supports lossless FLAC and bandwidth-saving Opus. FLAC is the
default; the available Opus profiles let you balance bandwidth and audio quality.
The player shows how much audio is buffered ahead of playback.

The audio buffer is temporary and held in memory. Mashin does not download music
for offline listening or save streamed audio files. A connection to Music Assistant
is required; a temporary interruption can only be covered by already buffered audio.

### Android

Android playback supports a media notification, headset controls, background
playback and Android Auto media controls. Playback can continue with the screen
off or while the app is minimized.

Closing the last app activity does not interrupt running or starting playback.
Paused playback follows Media3's foreground grace period, normally ten minutes.
Android Auto controller inactivity does not end the session. After foreground
playback ends, existing service bindings can keep the session available; resources
are released when the service is destroyed. Reconnecting retrieves the queue and
playback state from Music Assistant rather than a separate local playback store.
Device-specific battery and network restrictions can still affect background
playback.

## FFmpeg And Licenses

Mashin includes shared [FFmpeg 9.0.2](https://ffmpeg.org/) libraries for FLAC and
Opus decoding. This FFmpeg build is licensed under the **GNU Lesser General Public
License, version 2.1 or later (LGPL-2.1-or-later)**. It does not include GPL or
nonfree components. The Rust bindings, `ffmpeg-next` and `ffmpeg-sys-next`, are
licensed under the WTFPL.

The app packages include the LGPL license text, FFmpeg's license information,
the complete corresponding FFmpeg source archive and its build scripts and
configuration:

- Windows: `data/licenses/ffmpeg` in the app installation directory.
- Android: `assets/licenses/ffmpeg` inside the APK.

Preserve these materials when redistributing the app. FFmpeg is dynamically linked
and replaceable; distribution terms must not prohibit debugging modifications to
its LGPL components. See the [FFmpeg legal guidance](https://ffmpeg.org/legal.html)
for further information.
