import 'dart:convert';

import '../playback_models.dart';
import '../../../data/music_assistant/ma_models.dart';

enum SendspinConnectionState { offline, connecting, online, reconnecting }

class SendspinPlayerState {
  final SendspinConnectionState connectionState;
  final PlaybackStateType playbackState;
  final int volume;
  final bool muted;
  final RepeatMode? repeatMode;
  final bool shuffleEnabled;
  final int? positionMillis;
  final List<PlaybackQueueItem> queue;
  final int? currentIndex;

  const SendspinPlayerState({
    this.connectionState = SendspinConnectionState.offline,
    this.playbackState = PlaybackStateType.uninitialized,
    this.volume = 50,
    this.muted = false,
    this.repeatMode,
    this.shuffleEnabled = false,
    this.positionMillis,
    this.queue = const <PlaybackQueueItem>[],
    this.currentIndex,
  });

  PlaybackState toPlaybackState() => PlaybackState(
    playbackState: playbackState,
    volume: volume,
    muted: muted,
    repeatMode: repeatMode,
    shuffleEnabled: shuffleEnabled,
    positionMillis: positionMillis,
  );

  Map<String, dynamic> toJson() => <String, dynamic>{
    'connectionState': connectionState.name,
    'playbackState': playbackState.name,
    'volume': volume,
    'muted': muted,
    'repeatMode': repeatMode?.name,
    'shuffleEnabled': shuffleEnabled,
    'positionMillis': positionMillis,
    'queue': queue.map(_queueItemToJson).toList(growable: false),
    'currentIndex': currentIndex,
  };

  String toJsonString() => jsonEncode(toJson());

  factory SendspinPlayerState.fromJson(Map<String, dynamic> json) {
    final queue = (json['queue'] as List?)
            ?.whereType<Map<String, dynamic>>()
            .map(_queueItemFromRustJson)
            .toList(growable: false) ??
        const <PlaybackQueueItem>[];
    return SendspinPlayerState(
      connectionState: _parseConnectionState(json['connectionState']),
      playbackState: _parsePlaybackState(json['playbackState']),
      volume: (json['volume'] as num?)?.toInt().clamp(0, 100) ?? 50,
      muted: json['muted'] == true,
      repeatMode: repeatModeFromJson((json['repeatMode'] as String?)?.trim()),
      shuffleEnabled: json['shuffleEnabled'] == true,
      positionMillis: (json['localPlaybackPositionMillis'] as num?)?.toInt(),
      queue: queue,
      currentIndex: (json['currentIndex'] as num?)?.toInt(),
    );
  }
}

PlaybackQueueItem _queueItemFromRustJson(Map<String, dynamic> json) {
  final mediaItemJson = json['mediaItem'];
  final Track? track = mediaItemJson is Map<String, dynamic>
      ? Track.fromJson(mediaItemJson)
      : null;
  return PlaybackQueueItem(
    queueId: 'local-sendspin',
    queueItemId: (json['queueItemId'] as String?)?.trim() ?? '',
    name: track?.name ?? (json['name'] as String?)?.trim() ?? '',
    duration: track?.duration,
    sortIndex: (json['originalIndex'] as num?)?.toInt(),
    available: true,
    mediaItem: track,
    cache: json['cache'] is Map<String, dynamic>
      ? PlaybackQueueItemCache.fromJson(json['cache'] as Map<String, dynamic>)
      : null,
  );
}

Map<String, dynamic> _queueItemToJson(PlaybackQueueItem item) => <String, dynamic>{
  'queueItemId': item.queueItemId,
  'name': item.name,
  'duration': item.duration,
  'sortIndex': item.sortIndex,
  'available': item.available,
  'cache': item.cache?.toJson(),
  'mediaItem': item.mediaItem == null
      ? null
      : <String, dynamic>{
          'item_id': item.mediaItem!.itemId,
          'provider': item.mediaItem!.provider,
          'name': item.mediaItem!.name,
          'duration': item.mediaItem!.duration,
        },
};

SendspinConnectionState _parseConnectionState(Object? value) {
  return switch ((value as String?)?.trim().toLowerCase()) {
    'connecting' => SendspinConnectionState.connecting,
    'online' => SendspinConnectionState.online,
    'reconnecting' => SendspinConnectionState.reconnecting,
    _ => SendspinConnectionState.offline,
  };
}

PlaybackStateType _parsePlaybackState(Object? value) {
  return switch ((value as String?)?.trim().toLowerCase()) {
    'idle' => PlaybackStateType.idle,
    'buffering' => PlaybackStateType.buffering,
    'playing' => PlaybackStateType.playing,
    'paused' => PlaybackStateType.paused,
    'error' => PlaybackStateType.error,
    _ => PlaybackStateType.uninitialized,
  };
}
