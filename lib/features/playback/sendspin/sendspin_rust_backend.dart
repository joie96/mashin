import 'dart:async';

import '../playback_models.dart';
import 'sendspin_player_models.dart';
import 'sendspin_rust_bridge.dart';

class SendspinRustBackend {
  SendspinRustBackend({required this.bridge});

  final SendspinRustBridge bridge;

  PlaybackOutputMode get outputMode => PlaybackOutputMode.localSendspin;

  Stream<PlaybackState> get playbackStates => bridge.playbackStates.map(
    (SendspinPlayerState playbackState) => playbackState.toPlaybackState(),
  );

  Future<void> activate({String? targetPlayerId}) async {
    await bridge.engineInit();
  }

  Future<void> deactivate() => bridge.engineDisconnect(reason: 'deactivate');

  Future<void> connect() => bridge.engineConnect();

  Future<void> reconnect() => bridge.engineReconnect();

  Future<void> disconnect({String reason = 'client_disconnect'}) =>
      bridge.engineDisconnect(reason: reason);

  Future<void> terminate() async {
    try {
      await bridge.engineTerminate();
    } finally {
      await bridge.dispose();
    }
  }

  Future<void> transportPlay() => bridge.transportPlay();

  Future<void> transportPause() => bridge.transportPause();

  Future<void> transportNext() => bridge.transportNext();

  Future<void> transportPrevious() => bridge.transportPrevious();

  Future<void> transportSeek(double seconds) => bridge.transportSeek(seconds);

  Future<void> setVolume(int volume) => bridge.setVolume(volume);

  Future<void> setMuted(bool muted) => bridge.setMuted(muted);

  Future<PlaybackState> getPlaybackState() async {
    final SendspinPlayerState playbackState = await bridge.getPlaybackState();
    return playbackState.toPlaybackState();
  }
}
