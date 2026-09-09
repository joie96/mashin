import 'dart:convert';

import '../playback_models.dart';
import '../../../data/music_assistant/ma_models.dart';

enum SendspinConnectionState { offline, connecting, online, reconnecting }

class SendspinCacheItemState {
  // This key names the requested quality profile; the negotiated stream may differ.
  final String? cacheContentKey;
  final String? queueItemId;
  final String state;
  final int cachedDurationMillis;

  const SendspinCacheItemState({
    this.cacheContentKey,
    this.queueItemId,
    required this.state,
    required this.cachedDurationMillis,
  });

  factory SendspinCacheItemState.fromJson(Map<String, dynamic> json) {
    return SendspinCacheItemState(
      cacheContentKey: (json['cacheContentKey'] as String?)?.trim(),
      queueItemId: (json['queueItemId'] as String?)?.trim(),
      state: (json['state'] as String?)?.trim().toLowerCase() ?? 'idle',
      cachedDurationMillis:
          (json['cachedDurationMillis'] as num?)?.toInt() ?? 0,
    );
  }

  Map<String, dynamic> toJson() => <String, dynamic>{
    'cacheContentKey': cacheContentKey,
    'queueItemId': queueItemId,
    'state': state,
    'cachedDurationMillis': cachedDurationMillis,
  };
}

class SendspinPlayerState {
  final int downloadPlanVersion;
  final SendspinConnectionState connectionState;
  final PlaybackStateType playbackState;
  final String? currentCacheContentKey;
  final int volume;
  final bool muted;
  final int? positionMillis;
  final List<SendspinCacheItemState> cacheItems;
  final List<QueueItem> queue;
  final int? currentIndex;

  const SendspinPlayerState({
    this.downloadPlanVersion = 0,
    this.connectionState = SendspinConnectionState.offline,
    this.playbackState = PlaybackStateType.uninitialized,
    this.currentCacheContentKey,
    this.volume = 50,
    this.muted = false,
    this.positionMillis,
    this.cacheItems = const <SendspinCacheItemState>[],
    this.queue = const <QueueItem>[],
    this.currentIndex,
  });

  PlaybackState toPlaybackState() => PlaybackState(
    playbackState: playbackState,
    volume: volume,
    muted: muted,
    positionMillis: positionMillis,
  );

  Map<String, dynamic> toJson() => <String, dynamic>{
    'downloadPlanVersion': downloadPlanVersion,
    'connectionState': connectionState.name,
    'playbackState': playbackState.name,
    'currentCacheContentKey': currentCacheContentKey,
    'volume': volume,
    'muted': muted,
    'positionMillis': positionMillis,
    'cacheItems': cacheItems.map((item) => item.toJson()).toList(),
    'queue': queue.map(_queueItemToJson).toList(growable: false),
    'currentIndex': currentIndex,
  };

  String toJsonString() => jsonEncode(toJson());

  factory SendspinPlayerState.fromJson(Map<String, dynamic> json) {
    final cacheItems =
        (json['cacheItems'] as List?)
            ?.whereType<Map<String, dynamic>>()
            .map(SendspinCacheItemState.fromJson)
            .toList(growable: false) ??
        const <SendspinCacheItemState>[];
      final queue = (json['queue'] as List?)
          ?.whereType<Map<String, dynamic>>()
          .map(_queueItemFromRustJson)
          .toList(growable: false) ??
        const <QueueItem>[];
    return SendspinPlayerState(
      downloadPlanVersion: (json['downloadPlanVersion'] as num?)?.toInt() ?? 0,
      connectionState: _parseConnectionState(json['connectionState']),
      playbackState: _parsePlaybackState(json['playbackState']),
      currentCacheContentKey: (json['currentCacheContentKey'] as String?)
          ?.trim(),
      volume: (json['volume'] as num?)?.toInt().clamp(0, 100) ?? 50,
      muted: json['muted'] == true,
      positionMillis: (json['localPlaybackPositionMillis'] as num?)?.toInt(),
      cacheItems: cacheItems,
      queue: queue,
      currentIndex: (json['currentIndex'] as num?)?.toInt(),
    );
  }
}

QueueItem _queueItemFromRustJson(Map<String, dynamic> json) {
  final mediaItemJson = json['mediaItem'];
  final Track? track = mediaItemJson is Map<String, dynamic>
      ? Track.fromJson(mediaItemJson)
      : null;
  return QueueItem(
    queueId: 'local-sendspin',
    queueItemId: (json['queueItemId'] as String?)?.trim() ?? '',
    name: track?.name ?? (json['name'] as String?)?.trim() ?? '',
    duration: track?.duration,
    sortIndex: (json['sortIndex'] as num?)?.toInt(),
    available: true,
    mediaItem: track,
  );
}

Map<String, dynamic> _queueItemToJson(QueueItem item) => <String, dynamic>{
  'queueItemId': item.queueItemId,
  'name': item.name,
  'duration': item.duration,
  'sortIndex': item.sortIndex,
  'available': item.available,
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
