import 'dart:convert';

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

// Play Media options
enum PlaybackQueueInsertOption {
  replace,
  next,
  replaceNext,
  last,
}

// Queue State
class PlaybackQueueSnapshot {
  final String? queueId;
  final List<Map<String, dynamic>> items;
  final int? currentIndex;
  final bool? shuffleEnabled;
  final String? repeatMode;
  final bool? dontStopTheMusicEnabled;

  const PlaybackQueueSnapshot({
    this.queueId,
    this.items = const <Map<String, dynamic>>[],
    this.currentIndex,
    this.shuffleEnabled,
    this.repeatMode,
    this.dontStopTheMusicEnabled,
  });

  PlaybackQueueSnapshot copyWith({
    String? queueId,
    bool clearQueueId = false,
    List<Map<String, dynamic>>? items,
    int? currentIndex,
    bool clearCurrentIndex = false,
    bool? shuffleEnabled,
    bool clearShuffleEnabled = false,
    String? repeatMode,
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
  final String? repeatMode;
  final bool? shuffleEnabled;
  final int? seekMaxMillis;

  const PlaybackState({
    this.playbackState = PlaybackStateType.uninitialized,
    this.volume = 50,
    this.muted = false,
    this.repeatMode,
    this.shuffleEnabled,
    this.seekMaxMillis,
  });

  PlaybackState copyWith({
    PlaybackStateType? playbackState,
    int? volume,
    bool? muted,
    String? repeatMode,
    bool clearRepeatMode = false,
    bool? shuffleEnabled,
    bool clearShuffleEnabled = false,
    int? seekMaxMillis,
    bool clearSeekMaxMillis = false,
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
    );
  }

  Map<String, dynamic> toJson() {
    final payload = <String, dynamic>{
      'playbackState': playbackState.name,
      'volume': volume,
      'muted': muted,
      'repeatMode': repeatMode,
      'shuffleEnabled': shuffleEnabled,
      'seekMaxMillis': seekMaxMillis,
    };
    return payload;
  }

  String toJsonString() => jsonEncode(toJson());

  factory PlaybackState.fromJson(Map<String, dynamic> json) {
    final seekMaxMillis =
        (json['seekMaxMillis'] as num?)?.toInt() ??
            (json['seekMaxMilliseconds'] as num?)?.toInt();

    return PlaybackState(
      playbackState: _parsePlaybackState(json['playbackState']),
      volume: (json['volume'] as num?)?.toInt().clamp(0, 100) ?? 50,
      muted: json['muted'] == true,
      repeatMode: (json['repeatMode'] as String?)?.trim(),
      shuffleEnabled: json['shuffleEnabled'] as bool?,
      seekMaxMillis: seekMaxMillis,
    );
  }
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

