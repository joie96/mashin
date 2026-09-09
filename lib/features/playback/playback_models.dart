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

enum PlaybackStateReason { playMedia, codecSwitch, seek }

class PlaybackContentKey {
  const PlaybackContentKey._();

  static String fromMediaItem(
    MediaItem item, {
    required String codec,
    required int bitDepth,
    required int sampleRate,
    required int channels,
  }) {
    final String provider = item.provider.trim().toLowerCase();
    final String mediaType = item.mediaType.name.trim().toLowerCase();
    final String itemId = item.itemId.trim();
    if (provider.isEmpty || itemId.isEmpty) {
      throw ArgumentError.value(
        item,
        'item',
        'A media item needs a provider and item id for local playback.',
      );
    }

    final normalizedCodec = codec.trim().toLowerCase();
    if (normalizedCodec.isEmpty ||
        bitDepth <= 0 ||
        sampleRate <= 0 ||
        channels <= 0) {
      throw ArgumentError.value(
        codec,
        'codec',
        'A complete audio quality profile is required.',
      );
    }

    // This cache key describes the requested quality profile; the negotiated stream may differ.
    return '$provider:$mediaType:$itemId'
        ':$normalizedCodec'
        ':$sampleRate'
        ':$channels'
        ':$bitDepth';
  }
}

// Queue State
class PlaybackQueue {
  final String? queueId;
  final List<QueueItem> items;
  final int? currentIndex;
  final bool? shuffleEnabled;
  final RepeatMode? repeatMode;
  final bool? autoplayEnabled;

  const PlaybackQueue({
    this.queueId,
    this.items = const <QueueItem>[],
    this.currentIndex,
    this.shuffleEnabled,
    this.repeatMode,
    this.autoplayEnabled,
  });

  PlaybackQueue copyWith({
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
          : (dontStopTheMusicEnabled ?? this.autoplayEnabled),
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
  final PlaybackStateReason? reason;
  final DateTime? activeSince;

  const PlaybackState({
    this.playbackState = PlaybackStateType.uninitialized,
    this.volume = 50,
    this.muted = false,
    this.repeatMode,
    this.shuffleEnabled,
    this.seekMaxMillis,
    this.positionMillis,
    this.durationMillis,
    this.reason,
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
    int? seekMaxMillis,
    bool clearSeekMaxMillis = false,
    int? positionMillis,
    bool clearPositionMillis = false,
    int? durationMillis,
    bool clearDurationMillis = false,
    PlaybackStateReason? reason,
    bool clearReason = false,
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
      seekMaxMillis: clearSeekMaxMillis
          ? null
          : (seekMaxMillis ?? this.seekMaxMillis),
      positionMillis: clearPositionMillis
          ? null
          : (positionMillis ?? this.positionMillis),
      durationMillis: clearDurationMillis
          ? null
          : (durationMillis ?? this.durationMillis),
      reason: clearReason ? null : (reason ?? this.reason),
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
      'seekMaxMillis': seekMaxMillis,
      'positionMillis': positionMillis,
      'durationMillis': durationMillis,
      'reason': reason?.name,
      'activeSinceUtc': activeSince?.toUtc().toIso8601String(),
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
      seekMaxMillis: seekMaxMillis,
      positionMillis: positionMillis,
      durationMillis: durationMillis,
      reason: _parsePlaybackStateReason(json['reason']),
      activeSince: _parseDateTime(json['activeSinceUtc']),
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

DateTime? _parseDateTime(Object? value) {
  final String? raw = value as String?;
  return raw == null ? null : DateTime.tryParse(raw)?.toLocal();
}

PlaybackStateReason? _parsePlaybackStateReason(Object? value) {
  final String? normalized = (value as String?)?.trim().toLowerCase();
  return switch (normalized) {
    'playmedia' => PlaybackStateReason.playMedia,
    'codecswitch' => PlaybackStateReason.codecSwitch,
    'seek' => PlaybackStateReason.seek,
    _ => null,
  };
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
