import 'dart:async';

import 'playback_models.dart';

abstract class PlaybackBackend {
  PlaybackOutputMode get outputMode;

  Stream<PlaybackState> get states;

  Future<void> activate({String? targetPlayerId});

  Future<void> deactivate();

  Future<void> connect();

  Future<void> reconnect();

  Future<void> disconnect({String reason = 'client_disconnect'});

  Future<void> terminate();

  Future<void> transportPlay();

  Future<void> transportPause();

  Future<void> transportNext();

  Future<void> transportPrevious();

  Future<void> transportSeek(double seconds);

  Future<void> setVolume(int volume);

  Future<void> setMuted(bool muted);

  Future<PlaybackState> getState();
}
