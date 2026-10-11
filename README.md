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
	foreground notification and the playback CPU wake lock. Rust owns the queue and
	playback state; Media3 only projects that state.
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
	playback continues to use FFI. Persisted session identifiers are unchanged.

## Sendspin Download Recovery

Commands remain on HTTP. A single-track `player_queues/play_media` HTTP 500 is
retried three times in total, with one- and three-second retry delays. Exhaustion
marks that queue entry failed for the current pass and advances past it. HTTP 500
is a skip heuristic, not proof of provider unavailability; other HTTP statuses
and transport failures retain the connection-recovery path. HTTP commands have
a five-second connect timeout and a thirty-second total timeout.

Three consecutive exhausted entries request the existing full downloader
reconnect. Failed entries remain visible and excluded across reconnect and queue
edits; explicit playback of an entry retries it. Newly received track data resets
the streak, not playback of older cached audio. Repeat cannot loop over an entirely
unplayable queue. Known metadata/provider-unavailable entries do not count toward
the streak. Provider-instance snapshots are scoped to the server and session and
cached for thirty seconds; a failed snapshot query leaves provider status unknown.
Completely cached tracks may still play locally when their provider is unavailable.

Incomplete prefetch caches below 30 seconds of retained audio are discarded after
connection loss and downloaded again from zero without a seek. Current playback
always retains its PCM reference for seek restoration, even below this threshold.
The threshold is
`MINIMUM_CACHE_DURATION_FOR_RECONNECT_ALIGNMENT` in the Rust reconnect module.
Completed caches are retained.

Larger caches retain a three-second decoded PCM reference. A replacement stream
seeks at least five seconds before the confirmed cache end and is decoded as soon
as packets arrive, without a seek-confirmation gate. Quiet reference windows may
extend to ten seconds, with a correspondingly earlier seek. Correlation locates
the old audio in the replacement stream; original encoded packets are preserved,
duplicates are dropped, and the transition packet receives a sample-frame skip
marker. Playback and local seeks honor that marker without re-encoding audio.

Matching is bounded to 20 seconds of audio or 8 MiB of encoded packets, with a
20-second elapsed-time deadline. No unique match, a format mismatch, or a decode
failure discards prefetch caches and schedules a fresh download from zero. For the
current title, the cache and its identity are retained and seek restoration is
retried instead. Stop and Play Media, and Play
Media and a resume Seek, are separated by one-second asynchronous delays.

After stream replacement or progress timeout, a verified download within two
seconds of its expected metadata duration is completed at its actual decoded end.
This tolerance is not used to truncate a flowing stream or bypass PCM alignment.
Short interrupted prefetch caches are discarded before capacity admission is
estimated, and capacity-pressure eviction never removes the current title.

This recovery does not change the existing playback decoder behavior at Opus
stream boundaries. Validate real-server PCM, FLAC and Opus reconnects on the
target device in addition to host tests.

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

## ListenBrainz Scrobbling

Enter the ListenBrainz username and user token in Settings under ListenBrainz,
then use the page-wide Save button. All settings, including cache size, theme,
log level and ListenBrainz credentials, remain a local draft until Save; Cancel
restores the last saved values. The ListenBrainz Test button validates the draft
without saving or enabling scrobbling and shows a checkmark on success.
Mashin verifies changed credentials before storing the complete settings update
with the secure native session. Clear both fields and save to disable scrobbling.

The Rust core scrobbles local playback after half the track or four minutes of
actual listening, whichever is shorter. Pauses, buffering, downloads and seek
jumps do not count as listening. Repeats have distinct playback identities.
Pending listens are persisted without tokens, scoped to their ListenBrainz account,
and retried when the API is reachable. Remote-player playback is not scrobbled.

Disable Music Assistant's ListenBrainz scrobbler for the same local playback path
to avoid duplicate submissions. Recommendations and recommendation playlists
continue to come from Music Assistant; its recommendation plugin stays enabled.

## Build And Verify

Rebuild the native library after JNI changes, then rebuild Flutter:

```powershell
& ./scripts/build_mashin_engine.ps1 -Profile Debug -PlatformSelection AndroidArm64
flutter build apk --debug
./android/gradlew.bat -p android :app:testDebugUnitTest --console=plain
cargo test --manifest-path rust/mashin_engine/Cargo.toml jni::commands::tests --lib
flutter test test/android_bridge_test.dart
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
