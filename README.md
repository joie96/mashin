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
- Minimizing the Flutter activity keeps the session alive. Closing the activity or
	removing its task releases the Media3 session, removes its notification and shuts
	down native playback, even when Android Auto is connected. Configuration changes
	that recreate the activity do not shut down playback.
- Disconnecting the last Android Auto controller ends the session only when no
	Flutter activity exists, including a minimized activity. This follows Media3
	controller disconnect callbacks; legacy controller disconnects may be delayed.
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
activity must remove the notification and end playback. Disconnect Android Auto
both without a Flutter activity and with a minimized Flutter activity: only the
former must end the session. After pause or stop, verify that
audio focus and the CPU wake lock are released. Host tests do not certify standby
behavior or manufacturer-specific battery policies.
For Bluetooth headsets, repeat pause/play and verify that the native AAudio player
is paused (not `started`) during pause and that headset play reaches the MediaSession.
