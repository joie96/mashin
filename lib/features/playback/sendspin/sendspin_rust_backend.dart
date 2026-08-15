import 'dart:async';

import '../playback_models.dart';
import 'sendspin_player_models.dart';
import 'sendspin_rust_bridge.dart';

class SendspinRustBackend {
  SendspinRustBackend({
    required this.bridge,
    required this.musicAssistantBaseUrlResolver,
  });

  final SendspinRustBridge bridge;
  final String Function() musicAssistantBaseUrlResolver;

  PlaybackOutputMode get outputMode => PlaybackOutputMode.localSendspin;

  Stream<PlaybackState> get states =>
      bridge.states.map((SendspinPlayerState state) => state.toPlaybackState());

  Future<void> activate({String? targetPlayerId}) async {
    await bridge.engineInit(
      musicAssistantBaseUrl: musicAssistantBaseUrlResolver(),
    );
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

  Future<PlaybackState> getState() async {
    final state = await bridge.getState();
    return state.toPlaybackState();
  }
}
