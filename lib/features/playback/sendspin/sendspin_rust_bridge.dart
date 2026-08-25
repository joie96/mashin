import 'dart:async';

import '../../../data/music_assistant/ma_models.dart';
import 'sendspin_player_models.dart';

abstract class SendspinRustBridge {
  Stream<SendspinPlayerState> get playbackStates;
  Stream<Map<String, dynamic>> get events;

  Future<void> engineInit();

  Future<void> engineConnect();

  Future<void> engineReconnect();

  Future<void> engineDisconnect({required String reason});

  Future<void> engineTerminate();

  Future<void> transportPlay();

  Future<void> transportPause();

  Future<void> transportStop();

  Future<void> transportNext();

  Future<void> transportPrevious();

  Future<void> transportSeek(double seconds);

  Future<void> transportSeekRelativeMs(int offsetMs);

  Future<void> transportRepeatMode(RepeatMode mode);

  Future<void> transportShuffle(bool enabled);

  Future<void> transportSwitchGroup();

  Future<void> setVolume(int volume);

  Future<void> setMuted(bool muted);

  Future<void> senderSendSyncState(String state);

  Future<void> senderEnterExternalSource();

  Future<void> senderExitExternalSource({Map<String, dynamic>? playerState});

  Future<void> senderRequestStreamFormatsJson(String requestJson);

  Future<void> senderSendMessageJson(String messageJson);

  Future<SendspinPlayerState> getPlaybackState();

  Future<void> dispose();
}
