import 'dart:convert';

import '../../data/music_assistant/ma_models.dart';

// Output Modes (corresponding to different players)
enum PlaybackOutputMode { localOffline, localSendspin, maRemote }

// Playback State Types
enum PlaybackStateType {
  unknown,
  uninitialized,
  idle,
  buffering,
  playing,
  paused,
  seeking,
  error,
}

// Custom Queue Item with Cache information
class PlaybackQueueItemCache {
  final String state;
  final int cachedDurationMillis;

  const PlaybackQueueItemCache({
    required this.state,
    required this.cachedDurationMillis,
  });

  factory PlaybackQueueItemCache.fromJson(Map<String, dynamic> json) {
    return PlaybackQueueItemCache(
      state: (json['state'] as String?)?.trim().toLowerCase() ?? 'idle',
      cachedDurationMillis:
          (json['cachedDurationMillis'] as num?)?.toInt() ?? 0,
    );
  }

  Map<String, dynamic> toJson() => <String, dynamic>{
    'state': state,
    'cachedDurationMillis': cachedDurationMillis,
  };
}

class PlaybackQueueItem extends QueueItem {
  final PlaybackQueueItemCache? cache;

  const PlaybackQueueItem({
    required super.queueId,
    required super.queueItemId,
    required super.name,
    super.duration,
    super.sortIndex,
    super.available,
    super.mediaItem,
    this.cache,
  });

}


// Queue State
class PlaybackQueue {
  final String? queueId;
  final List<PlaybackQueueItem> items;
  final int? currentIndex;
  final bool? shuffleEnabled;
  final RepeatMode? repeatMode;
  final bool? autoplayEnabled;

  const PlaybackQueue({
    this.queueId,
    this.items = const <PlaybackQueueItem>[],
    this.currentIndex,
    this.shuffleEnabled,
    this.repeatMode,
    this.autoplayEnabled,
  });

  PlaybackQueue copyWith({
    String? queueId,
    bool clearQueueId = false,
    List<PlaybackQueueItem>? items,
    int? currentIndex,
    bool clearCurrentIndex = false,
    bool? shuffleEnabled,
    bool clearShuffleEnabled = false,
    RepeatMode? repeatMode,
    bool clearRepeatMode = false,
    bool? dontStopTheMusicEnabled,
    bool clearDontStopTheMusicEnabled = false,
  }) {
    return PlaybackQueue(
      queueId: clearQueueId ? null : (queueId ?? this.queueId),
      items: items ?? this.items,
      currentIndex: clearCurrentIndex
          ? null
          : (currentIndex ?? this.currentIndex),
      shuffleEnabled: clearShuffleEnabled
          ? null
          : (shuffleEnabled ?? this.shuffleEnabled),
      repeatMode: clearRepeatMode ? null : (repeatMode ?? this.repeatMode),
      autoplayEnabled: clearDontStopTheMusicEnabled
          ? null
          : (dontStopTheMusicEnabled ?? autoplayEnabled),
    );
  }
}


// Playback State
class PlaybackState {
  final PlaybackStateType playbackState;
  final int volume;
  final bool muted;
  final RepeatMode? repeatMode;
  final bool? shuffleEnabled;
  final int? positionMillis;
  final int? durationMillis;
  final DateTime? activeSince;

  const PlaybackState({
    this.playbackState = PlaybackStateType.uninitialized,
    this.volume = 50,
    this.muted = false,
    this.repeatMode,
    this.shuffleEnabled,
    this.positionMillis,
    this.durationMillis,
    this.activeSince,
  });

  PlaybackState copyWith({
    PlaybackStateType? playbackState,
    int? volume,
    bool? muted,
    RepeatMode? repeatMode,
    bool clearRepeatMode = false,
    bool? shuffleEnabled,
    bool clearShuffleEnabled = false,
    int? positionMillis,
    bool clearPositionMillis = false,
    int? durationMillis,
    bool clearDurationMillis = false,
    DateTime? activeSince,
    bool clearActiveSince = false,
  }) {
    return PlaybackState(
      playbackState: playbackState ?? this.playbackState,
      volume: volume ?? this.volume,
      muted: muted ?? this.muted,
      repeatMode: clearRepeatMode ? null : (repeatMode ?? this.repeatMode),
      shuffleEnabled: clearShuffleEnabled
          ? null
          : (shuffleEnabled ?? this.shuffleEnabled),
      positionMillis: clearPositionMillis
          ? null
          : (positionMillis ?? this.positionMillis),
      durationMillis: clearDurationMillis
          ? null
          : (durationMillis ?? this.durationMillis),
      activeSince: clearActiveSince ? null : (activeSince ?? this.activeSince),
    );
  }

  Map<String, dynamic> toJson() {
    final payload = <String, dynamic>{
      'playbackState': playbackState.name,
      'volume': volume,
      'muted': muted,
      'repeatMode': repeatMode?.name,
      'shuffleEnabled': shuffleEnabled,
      'positionMillis': positionMillis,
      'durationMillis': durationMillis,
      'activeSinceUtc': activeSince?.toUtc().toIso8601String(),
    };
    return payload;
  }

  String toJsonString() => jsonEncode(toJson());

  factory PlaybackState.fromJson(Map<String, dynamic> json) {
    final positionMillis =
        _toClampedMillis((json['positionMillis'] as num?)?.toDouble()) ??
        _toClampedMillis((json['positionMs'] as num?)?.toDouble()) ??
        _toClampedMillisFromSeconds(
          (json['positionSecs'] as num?)?.toDouble(),
        ) ??
        _toClampedMillisFromSeconds(
          (json['positionSeconds'] as num?)?.toDouble(),
        );
    final durationMillis =
        _toClampedMillis((json['durationMillis'] as num?)?.toDouble()) ??
        _toClampedMillis((json['durationMs'] as num?)?.toDouble()) ??
        _toClampedMillisFromSeconds(
          (json['durationSecs'] as num?)?.toDouble(),
        ) ??
        _toClampedMillisFromSeconds(
          (json['durationSeconds'] as num?)?.toDouble(),
        );

    return PlaybackState(
      playbackState: _parsePlaybackState(json['playbackState']),
      volume: (json['volume'] as num?)?.toInt().clamp(0, 100) ?? 50,
      muted: json['muted'] == true,
      repeatMode: repeatModeFromJson((json['repeatMode'] as String?)?.trim()),
      shuffleEnabled: json['shuffleEnabled'] as bool?,
      positionMillis: positionMillis,
      durationMillis: durationMillis,
      activeSince: _parseDateTime(json['activeSinceUtc']),
    );
  }
}


// Helper
int? _toClampedMillis(double? value) {
  if (value == null) {
    return null;
  }

  final int millis = value.round();
  return millis < 0 ? 0 : millis;
}

int? _toClampedMillisFromSeconds(double? value) {
  if (value == null) {
    return null;
  }

  return _toClampedMillis(value * 1000);
}

DateTime? _parseDateTime(Object? value) {
  final String? raw = value as String?;
  return raw == null ? null : DateTime.tryParse(raw)?.toLocal();
}

PlaybackStateType _parsePlaybackState(dynamic value) {
  final normalized = (value as String?)?.trim().toLowerCase();
  switch (normalized) {
    case 'uninitialized':
      return PlaybackStateType.uninitialized;
    case 'idle':
      return PlaybackStateType.idle;
    case 'buffering':
      return PlaybackStateType.buffering;
    case 'playing':
      return PlaybackStateType.playing;
    case 'paused':
      return PlaybackStateType.paused;
    case 'seeking':
      return PlaybackStateType.seeking;
    case 'error':
      return PlaybackStateType.error;
    default:
      return PlaybackStateType.unknown;
  }
}
