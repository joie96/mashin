import 'dart:convert';

import '../../data/music_assistant/ma_models.dart';

// Output Modes (corresponding to different players)
enum PlaybackOutputMode {
  localOffline,
  localSendspin,
  maRemote,
}

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

// Queue State
class PlaybackQueueSnapshot {
  final String? queueId;
  final List<QueueItem> items;
  final int? currentIndex;
  final bool? shuffleEnabled;
  final RepeatMode? repeatMode;
  final bool? dontStopTheMusicEnabled;

  const PlaybackQueueSnapshot({
    this.queueId,
    this.items = const <QueueItem>[],
    this.currentIndex,
    this.shuffleEnabled,
    this.repeatMode,
    this.dontStopTheMusicEnabled,
  });

  PlaybackQueueSnapshot copyWith({
    String? queueId,
    bool clearQueueId = false,
    List<QueueItem>? items,
    int? currentIndex,
    bool clearCurrentIndex = false,
    bool? shuffleEnabled,
    bool clearShuffleEnabled = false,
    RepeatMode? repeatMode,
    bool clearRepeatMode = false,
    bool? dontStopTheMusicEnabled,
    bool clearDontStopTheMusicEnabled = false,
  }) {
    return PlaybackQueueSnapshot(
      queueId: clearQueueId ? null : (queueId ?? this.queueId),
      items: items ?? this.items,
      currentIndex: clearCurrentIndex ? null : (currentIndex ?? this.currentIndex),
      shuffleEnabled: clearShuffleEnabled ? null : (shuffleEnabled ?? this.shuffleEnabled),
      repeatMode: clearRepeatMode ? null : (repeatMode ?? this.repeatMode),
      dontStopTheMusicEnabled: clearDontStopTheMusicEnabled
          ? null
          : (dontStopTheMusicEnabled ?? this.dontStopTheMusicEnabled),
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
  final int? seekMaxMillis;
  final int? positionMillis;
  final int? durationMillis;

  const PlaybackState({
    this.playbackState = PlaybackStateType.uninitialized,
    this.volume = 50,
    this.muted = false,
    this.repeatMode,
    this.shuffleEnabled,
    this.seekMaxMillis,
    this.positionMillis,
    this.durationMillis,
  });

  PlaybackState copyWith({
    PlaybackStateType? playbackState,
    int? volume,
    bool? muted,
    RepeatMode? repeatMode,
    bool clearRepeatMode = false,
    bool? shuffleEnabled,
    bool clearShuffleEnabled = false,
    int? seekMaxMillis,
    bool clearSeekMaxMillis = false,
    int? positionMillis,
    bool clearPositionMillis = false,
    int? durationMillis,
    bool clearDurationMillis = false,
  }) {
    return PlaybackState(
      playbackState: playbackState ?? this.playbackState,
      volume: volume ?? this.volume,
      muted: muted ?? this.muted,
      repeatMode: clearRepeatMode ? null : (repeatMode ?? this.repeatMode),
      shuffleEnabled: clearShuffleEnabled ? null : (shuffleEnabled ?? this.shuffleEnabled),
      seekMaxMillis: clearSeekMaxMillis
          ? null
          : (seekMaxMillis ?? this.seekMaxMillis),
      positionMillis: clearPositionMillis ? null : (positionMillis ?? this.positionMillis),
      durationMillis: clearDurationMillis ? null : (durationMillis ?? this.durationMillis),
    );
  }

  Map<String, dynamic> toJson() {
    final payload = <String, dynamic>{
      'playbackState': playbackState.name,
      'volume': volume,
      'muted': muted,
      'repeatMode': repeatMode?.name,
      'shuffleEnabled': shuffleEnabled,
      'seekMaxMillis': seekMaxMillis,
      'positionMillis': positionMillis,
      'durationMillis': durationMillis,
    };
    return payload;
  }

  String toJsonString() => jsonEncode(toJson());

  factory PlaybackState.fromJson(Map<String, dynamic> json) {
    final seekMaxMillis =
        (json['seekMaxMillis'] as num?)?.toInt() ??
            (json['seekMaxMilliseconds'] as num?)?.toInt();
    final positionMillis =
      _toClampedMillis((json['positionMillis'] as num?)?.toDouble()) ??
        _toClampedMillis((json['positionMs'] as num?)?.toDouble()) ??
        _toClampedMillisFromSeconds((json['positionSecs'] as num?)?.toDouble()) ??
        _toClampedMillisFromSeconds((json['positionSeconds'] as num?)?.toDouble());
    final durationMillis =
      _toClampedMillis((json['durationMillis'] as num?)?.toDouble()) ??
        _toClampedMillis((json['durationMs'] as num?)?.toDouble()) ??
        _toClampedMillisFromSeconds((json['durationSecs'] as num?)?.toDouble()) ??
        _toClampedMillisFromSeconds((json['durationSeconds'] as num?)?.toDouble());

    return PlaybackState(
      playbackState: _parsePlaybackState(json['playbackState']),
      volume: (json['volume'] as num?)?.toInt().clamp(0, 100) ?? 50,
      muted: json['muted'] == true,
      repeatMode: repeatModeFromJson((json['repeatMode'] as String?)?.trim()),
      shuffleEnabled: json['shuffleEnabled'] as bool?,
      seekMaxMillis: seekMaxMillis,
      positionMillis: positionMillis,
      durationMillis: durationMillis,
    );
  }
}

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


// Helper functions to parse enums from strings
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

