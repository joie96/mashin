import 'dart:async';

import '../playback_backend.dart';
import '../playback_models.dart';
import 'sendspin_player_models.dart';
import 'sendspin_rust_bridge.dart';

class SendspinRustBackend implements PlaybackBackend {
  SendspinRustBackend({
    required this.bridge,
    required this.musicAssistantBaseUrlResolver,
  });

  final SendspinRustBridge bridge;
  final String Function() musicAssistantBaseUrlResolver;

  @override
  PlaybackOutputMode get outputMode => PlaybackOutputMode.localSendspin;

  @override
  Stream<PlaybackState> get states =>
      bridge.states.map((SendspinPlayerState state) => state.toPlaybackState());

  @override
  Future<void> activate({String? targetPlayerId}) async {
    await bridge.engineInit(
      musicAssistantBaseUrl: musicAssistantBaseUrlResolver(),
    );
  }

  @override
  Future<void> deactivate() => bridge.engineDisconnect(reason: 'deactivate');

  @override
  Future<void> connect() => bridge.engineConnect();

  @override
  Future<void> reconnect() => bridge.engineReconnect();

  @override
  Future<void> disconnect({String reason = 'client_disconnect'}) =>
      bridge.engineDisconnect(reason: reason);

  @override
  Future<void> terminate() async {
    try {
      await bridge.engineTerminate();
    } finally {
      await bridge.dispose();
    }
  }

  @override
  Future<void> transportPlay() => bridge.transportPlay();

  @override
  Future<void> transportPause() => bridge.transportPause();

  @override
  Future<void> transportNext() => bridge.transportNext();

  @override
  Future<void> transportPrevious() => bridge.transportPrevious();

  @override
  Future<void> transportSeek(double seconds) => bridge.transportSeek(seconds);

  @override
  Future<void> setVolume(int volume) => bridge.setVolume(volume);

  @override
  Future<void> setMuted(bool muted) => bridge.setMuted(muted);

  @override
  Future<PlaybackState> getState() async {
    final state = await bridge.getState();
    return state.toPlaybackState();
  }
}
