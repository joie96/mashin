# mashin_flutter

Flutter client for Music Assistant, backed by the Rust Mashin playback core.

## Android Playback Ownership

Android playback commands follow one path:

```text
MashinCoreBridge -> AndroidBridge -> FlutterPlaybackChannel
	-> UI MediaController -> PlaybackService MediaSession
	-> RustCoreBridge JNI -> Rust core
```

- `PlaybackService` owns Android session restoration, command ordering, audio focus,
	foreground notification and the playback CPU wake lock. Rust projects the
	server-owned queue and playback state; Media3 only presents that projection.
- Flutter activities (including minimized activities) and Android Auto controllers
	share session ownership. Closing the last activity or removing its task ends
	playback only when no Android Auto controller or reconnect grace remains.
	Configuration changes that recreate the activity do not shut down playback.
- Disconnecting the last Android Auto controller starts a five-second reconnect
	grace, preserving the session and ongoing playback. Reconnecting a controller or
	attaching an activity cancels the pending shutdown. At expiry, the session ends
	only if no activity or Android Auto controller exists and no activity recreation
	is in progress. This follows Media3 controller disconnect callbacks; legacy
	controller disconnects may be delayed.
- Before engine detach, Android synchronously revokes the Flutter FFI listener.
	Registration and native callback invocation share a lock, so no old listener
	can be called after revocation returns. Service-owned JNI events remain active.
- A local playback start establishes foreground eligibility before requesting
	audio focus or executing Rust. Failed startup does not fall back to FFI.
- Local pause suspends the native audio stream without discarding buffered audio.
	Writing silence to a running stream still counts as active music on Android
	and can cause the Bluetooth service to discard headset play commands.
	Native pause/play failures propagate through JNI and FFI before state changes.
- Notification permission is not a prerequisite for media-session playback.
	A CPU wake lock does not guarantee network availability under every Android
	battery restriction or Doze policy.
- Library queries and non-playback operations may use FFI on Android. Desktop
	playback continues to use FFI.

## Playback And Session Ownership

Only two modes exist: `local` plays through the native DAC; `remote` controls an
existing Music Assistant player without owning audio. Both use MA's queue and
stable queue-item IDs. Radio, shuffle, repeat and autoplay are server decisions.
Favorites, client-local playlists, the MA library and recommendations remain
available. Client-local playlists are expanded before sending their media to MA.

One concrete `PlaybackEngine` owns mode selection and delegates to `LocalPlayback`
or `RemotePlayback`. Both use a shared `MusicAssistantPlaybackTarget` for MA
commands and queue projections; neither mode owns or embeds the other.

`CoreCoordinator` owns application transactions, not a second playback API.
Session login and output selection live in `core/session.rs`; persisted settings
live in `core/settings.rs`. Pure playback commands go directly to the engine
through one serialized core entry that collects events and awaits network work
outside the core lock. Local volume persistence remains an application setting;
remote volume never overwrites it.

One `MusicAssistantConnection` shares the authenticated API client and EventHub
between targets. Local configuration contains only device and audio preferences.
`LocalPlayback` prepares and reuses its own receiver session; the engine installs
sessions and retains checkpoints for persistence rollback. Profile changes retain
their original receiver and own native compensation. The engine rejects stale
commits; the application only persists the confirmed preference. Negotiated server
profiles do not replace the user's preference or force receiver recreation.

One native authenticated MA EventHub supplies queue and player changes. Mode
selection does not replace the engine-owned local session or its RAM audio.
The receiver connects to `/mashin/v1/control` and `/mashin/v1/audio`; neither
socket receives the MA API token. Supported profiles are FLAC (default) and
Opus 96, 128, 192 and 256 kb/s.

The bounded RAM cache retains only authorized encoded chunks for the current and
prepared next stream. Connection loss preserves accepted chunks and the receive
cursor; it does not start a downloader, create audio files or reconstruct a queue.
Chunks are acknowledged only after native retention. Position and track changes
follow audible output, not receipt or decoder completion.

At Debug log level, the native buffer reports occupied/capacity KiB, cached chunks
and remaining audio every five seconds while a stream is configured. Prepared-next
audio is reported separately. Desktop and the opened mobile player overlay show
buffered audio ahead of the audible position, limited to the current track.
The compact mobile player keeps its existing playback-progress indicator.

Session storage format 3 requires a fresh login for older or invalid sessions.
Each fresh login creates a persistent Mashin device identity. Selecting Remote
and returning to Local preserves it. This reset does not rewrite favorites or
local playlists stored in MA user preferences. Settings remain drafts until Save;
Cancel discards changes. Failed persistence restores the preceding native session.

### Native Structure

```text
core/                          App settings, secure persistence and user data
core/session.rs                Login, output selection and session transactions
core/settings.rs               Persisted application and playback preferences
playback/engine.rs              Concrete engine, lifecycle and mode dispatch
playback/backend.rs             Shared Local/Remote command contract
playback/mode.rs                Canonical local/remote mode names
playback/local/                 Local presentation and receiver session
playback/local/mashin/          V1 sockets, framing, authorization and audio adapter
playback/remote/                Remote MA presentation, no audio resources
audio/                         Pipeline worker, bounded buffer, codecs and CPAL
music_assistant/                API, EventHub, playback target and projections
music_assistant/connection.rs    Shared API client and EventHub resources
```

The audio layer has no dependency on Mashin wire messages or MA models. The
local adapter validates the protocol and supplies typed audio commands, formats
and chunks. Only the pipeline worker owns native audio resources. The application
core uses typed profile operations and playback checkpoints instead of accessing sockets
or audio internals. Platform snapshots use `activeMode`; local selection uses
`selectLocalOutput`, with no legacy command aliases.

## User Data Version 5

Mashin favorites and local playlists use a deduplicated version-five store in
Music Assistant user preferences. The store preserves all modeled media fields,
provider mappings, metadata and ordered playlist references. Queue playback sends
full media objects, never URI strings. Sparse new entries are completed through
the typed `music/*/get` APIs before they are added.
This completion is limited to ingestion: queue playback and version-five store
reads never repair missing data. Version-five writes and reads validate provider
mappings, media identities and references. Invalid mutation batches leave active
user data unchanged and report an error instead of silently healing the store.

Only version five is supported; user-data loading does not upgrade or rewrite
preferences. Mapping identity strings containing `None` are preserved as returned
by Music Assistant. Null or unknown audio formats remain allowed.

## Scrobbling

Scrobbling is owned by Music Assistant. The client has no scrobbling credentials,
outbox, worker or connection-test command. Recommendations come from MA without
restricting them to one provider.

## Build And Verify

Rebuild the native library after JNI changes, then rebuild Flutter:

```powershell
& ./scripts/build_mashin_engine.ps1 -Profile Debug -PlatformSelection AndroidArm64
flutter build apk --debug
./android/gradlew.bat -p android :app:testDebugUnitTest --console=plain
cargo test --manifest-path rust/mashin_engine/Cargo.toml --lib
flutter analyze
flutter test
```

Kotlin/Rust changes require a full app restart, not just Flutter hot reload.
On a device, verify local playback with a media notification, screen-off playback,
activity closure and task removal while playing, and UI reconnection. Closing the
last activity or removing its task must preserve playback while Android Auto is
connected; without another owner or reconnect grace, it must end playback and
remove the notification. Disconnect Android Auto both without a Flutter activity
and with a minimized Flutter activity: only the former must end the session after
five seconds. Reconnect within five seconds and verify uninterrupted playback.
After pause or stop, verify that
audio focus and the CPU wake lock are released. Host tests do not certify standby
behavior or manufacturer-specific battery policies.
For Bluetooth headsets, repeat pause/play and verify that the native AAudio player
is paused (not `started`) during pause and that headset play reaches the MediaSession.
