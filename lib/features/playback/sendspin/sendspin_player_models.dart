import 'dart:convert';

import '../../../data/music_assistant/ma_models.dart';
import '../playback_models.dart';

enum SendspinConnectionState {
  offline,
  connecting,
  online,
  reconnecting,
}

class SendspinPlayerState {
  final SendspinConnectionState connectionState;
  final PlaybackStateType playbackState;
  final int volume;
  final bool muted;
  final RepeatMode? repeatMode;
  final bool? shuffleEnabled;
  final int? seekMaxMillis;

  final int? metadataTimestampMicros;
  final String? metadataTitle;
  final String? metadataArtist;
  final String? metadataAlbumArtist;
  final String? metadataAlbum;
  final String? metadataArtworkUrl;
  final int? metadataYear;
  final int? metadataTrackNumber;
  final int? metadataTrackProgressMillis;
  final int? metadataTrackDurationMillis;
  final int? metadataPlaybackSpeedMillis;
  final String? activeCodec;
  final int? activeSampleRateHz;
  final int? activeChannels;
  final int? activeBitDepth;
  final String? groupId;
  final String? groupName;
  final String? serverName;

  const SendspinPlayerState({
    this.connectionState = SendspinConnectionState.offline,
    this.playbackState = PlaybackStateType.uninitialized,
    this.volume = 50,
    this.muted = false,
    this.repeatMode,
    this.shuffleEnabled,
    this.seekMaxMillis,
    this.metadataTimestampMicros,
    this.metadataTitle,
    this.metadataArtist,
    this.metadataAlbumArtist,
    this.metadataAlbum,
    this.metadataArtworkUrl,
    this.metadataYear,
    this.metadataTrackNumber,
    this.metadataTrackProgressMillis,
    this.metadataTrackDurationMillis,
    this.metadataPlaybackSpeedMillis,
    this.activeCodec,
    this.activeSampleRateHz,
    this.activeChannels,
    this.activeBitDepth,
    this.groupId,
    this.groupName,
    this.serverName,
  });

  PlaybackState toPlaybackState() {
    return PlaybackState(
      playbackState: playbackState,
      volume: volume,
      muted: muted,
      repeatMode: repeatMode,
      shuffleEnabled: shuffleEnabled,
      seekMaxMillis: seekMaxMillis,
      positionMillis: metadataTrackProgressMillis,
      durationMillis: metadataTrackDurationMillis,
    );
  }

  SendspinPlayerState copyWith({
    SendspinConnectionState? connectionState,
    PlaybackStateType? playbackState,
    int? volume,
    bool? muted,
    RepeatMode? repeatMode,
    bool clearRepeatMode = false,
    bool? shuffleEnabled,
    bool clearShuffleEnabled = false,
    int? seekMaxMillis,
    bool clearSeekMaxMillis = false,
    int? metadataTimestampMicros,
    bool clearMetadataTimestampMicros = false,
    String? metadataTitle,
    bool clearMetadataTitle = false,
    String? metadataArtist,
    bool clearMetadataArtist = false,
    String? metadataAlbumArtist,
    bool clearMetadataAlbumArtist = false,
    String? metadataAlbum,
    bool clearMetadataAlbum = false,
    String? metadataArtworkUrl,
    bool clearMetadataArtworkUrl = false,
    int? metadataYear,
    bool clearMetadataYear = false,
    int? metadataTrackNumber,
    bool clearMetadataTrackNumber = false,
    int? metadataTrackProgressMillis,
    bool clearMetadataTrackProgressMillis = false,
    int? metadataTrackDurationMillis,
    bool clearMetadataTrackDurationMillis = false,
    int? metadataPlaybackSpeedMillis,
    bool clearMetadataPlaybackSpeedMillis = false,
    String? activeCodec,
    bool clearActiveCodec = false,
    int? activeSampleRateHz,
    bool clearActiveSampleRateHz = false,
    int? activeChannels,
    bool clearActiveChannels = false,
    int? activeBitDepth,
    bool clearActiveBitDepth = false,
    String? groupId,
    bool clearGroupId = false,
    String? groupName,
    bool clearGroupName = false,
    String? serverName,
    bool clearServerName = false,
  }) {
    return SendspinPlayerState(
      connectionState: connectionState ?? this.connectionState,
      playbackState: playbackState ?? this.playbackState,
      volume: volume ?? this.volume,
      muted: muted ?? this.muted,
      repeatMode: clearRepeatMode ? null : (repeatMode ?? this.repeatMode),
      shuffleEnabled: clearShuffleEnabled ? null : (shuffleEnabled ?? this.shuffleEnabled),
      seekMaxMillis: clearSeekMaxMillis ? null : (seekMaxMillis ?? this.seekMaxMillis),
      metadataTimestampMicros: clearMetadataTimestampMicros
          ? null
          : (metadataTimestampMicros ?? this.metadataTimestampMicros),
      metadataTitle: clearMetadataTitle ? null : (metadataTitle ?? this.metadataTitle),
      metadataArtist: clearMetadataArtist ? null : (metadataArtist ?? this.metadataArtist),
      metadataAlbumArtist: clearMetadataAlbumArtist
          ? null
          : (metadataAlbumArtist ?? this.metadataAlbumArtist),
      metadataAlbum: clearMetadataAlbum ? null : (metadataAlbum ?? this.metadataAlbum),
      metadataArtworkUrl: clearMetadataArtworkUrl
          ? null
          : (metadataArtworkUrl ?? this.metadataArtworkUrl),
      metadataYear: clearMetadataYear ? null : (metadataYear ?? this.metadataYear),
      metadataTrackNumber: clearMetadataTrackNumber
          ? null
          : (metadataTrackNumber ?? this.metadataTrackNumber),
      metadataTrackProgressMillis: clearMetadataTrackProgressMillis
          ? null
          : (metadataTrackProgressMillis ?? this.metadataTrackProgressMillis),
      metadataTrackDurationMillis: clearMetadataTrackDurationMillis
          ? null
          : (metadataTrackDurationMillis ?? this.metadataTrackDurationMillis),
      metadataPlaybackSpeedMillis: clearMetadataPlaybackSpeedMillis
          ? null
          : (metadataPlaybackSpeedMillis ?? this.metadataPlaybackSpeedMillis),
      activeCodec: clearActiveCodec ? null : (activeCodec ?? this.activeCodec),
      activeSampleRateHz: clearActiveSampleRateHz
          ? null
          : (activeSampleRateHz ?? this.activeSampleRateHz),
      activeChannels: clearActiveChannels ? null : (activeChannels ?? this.activeChannels),
      activeBitDepth: clearActiveBitDepth ? null : (activeBitDepth ?? this.activeBitDepth),
      groupId: clearGroupId ? null : (groupId ?? this.groupId),
      groupName: clearGroupName ? null : (groupName ?? this.groupName),
      serverName: clearServerName ? null : (serverName ?? this.serverName),
    );
  }

  bool get isEmpty =>
      metadataTimestampMicros == null &&
      metadataTitle == null &&
      metadataArtist == null &&
      metadataAlbumArtist == null &&
      metadataAlbum == null &&
      metadataArtworkUrl == null &&
      metadataYear == null &&
      metadataTrackNumber == null &&
      metadataTrackProgressMillis == null &&
      metadataTrackDurationMillis == null &&
      metadataPlaybackSpeedMillis == null &&
      activeCodec == null &&
      activeSampleRateHz == null &&
      activeChannels == null &&
      activeBitDepth == null &&
      groupId == null &&
      groupName == null &&
      serverName == null;

  Map<String, dynamic> toFlatJson() {
    return <String, dynamic>{
      'connectionState': connectionState.name,
      'playbackState': playbackState.name,
      'volume': volume,
      'muted': muted,
      'repeatMode': repeatMode?.name,
      'shuffleEnabled': shuffleEnabled,
      'seekMaxMillis': seekMaxMillis,
      'metadataTimestampMicros': metadataTimestampMicros,
      'metadataTitle': metadataTitle,
      'metadataArtist': metadataArtist,
      'metadataAlbumArtist': metadataAlbumArtist,
      'metadataAlbum': metadataAlbum,
      'metadataArtworkUrl': metadataArtworkUrl,
      'metadataYear': metadataYear,
      'metadataTrackNumber': metadataTrackNumber,
      'metadataTrackProgressMillis': metadataTrackProgressMillis,
      'metadataTrackDurationMillis': metadataTrackDurationMillis,
      'metadataPlaybackSpeedMillis': metadataPlaybackSpeedMillis,
      'activeCodec': activeCodec,
      'activeSampleRateHz': activeSampleRateHz,
      'activeChannels': activeChannels,
      'activeBitDepth': activeBitDepth,
      'groupId': groupId,
      'groupName': groupName,
      'serverName': serverName,
    };
  }

  Map<String, dynamic> toJson() => toFlatJson();

  String toJsonString() => jsonEncode(toFlatJson());

  factory SendspinPlayerState.fromJson(Map<String, dynamic> json) {
    final metadataTrackProgressMillis =
        (json['metadataTrackProgressMillis'] as num?)?.toInt() ??
            (json['metadataTrackProgressMilliseconds'] as num?)?.toInt() ??
            (json['trackProgressMilliseconds'] as num?)?.toInt();
    final metadataTrackDurationMillis =
        (json['metadataTrackDurationMillis'] as num?)?.toInt() ??
            (json['metadataTrackDurationMilliseconds'] as num?)?.toInt() ??
            (json['trackDurationMilliseconds'] as num?)?.toInt();
    final metadataPlaybackSpeedMillis =
        (json['metadataPlaybackSpeedMillis'] as num?)?.toInt() ??
            (json['metadataPlaybackSpeedMilli'] as num?)?.toInt() ??
            (json['playbackSpeedMilli'] as num?)?.toInt();
    final seekMaxMillis =
        (json['seekMaxMillis'] as num?)?.toInt() ??
            (json['seekMaxMilliseconds'] as num?)?.toInt();

    return SendspinPlayerState(
      connectionState: _parseSendspinConnectionState(json['connectionState']),
      playbackState: _parsePlaybackState(json['playbackState']),
      volume: (json['volume'] as num?)?.toInt().clamp(0, 100) ?? 50,
      muted: json['muted'] == true,
      repeatMode: repeatModeFromJson((json['repeatMode'] as String?)?.trim()),
      shuffleEnabled: json['shuffleEnabled'] as bool?,
      seekMaxMillis: seekMaxMillis,
      metadataTimestampMicros: (json['metadataTimestampMicros'] as num?)?.toInt(),
      metadataTitle: (json['metadataTitle'] as String?)?.trim(),
      metadataArtist: (json['metadataArtist'] as String?)?.trim(),
      metadataAlbumArtist: (json['metadataAlbumArtist'] as String?)?.trim(),
      metadataAlbum: (json['metadataAlbum'] as String?)?.trim(),
      metadataArtworkUrl: (json['metadataArtworkUrl'] as String?)?.trim(),
      metadataYear: (json['metadataYear'] as num?)?.toInt(),
      metadataTrackNumber: (json['metadataTrackNumber'] as num?)?.toInt(),
      metadataTrackProgressMillis: metadataTrackProgressMillis,
      metadataTrackDurationMillis: metadataTrackDurationMillis,
      metadataPlaybackSpeedMillis: metadataPlaybackSpeedMillis,
      activeCodec: (json['activeCodec'] as String?)?.trim(),
      activeSampleRateHz: (json['activeSampleRateHz'] as num?)?.toInt(),
      activeChannels: (json['activeChannels'] as num?)?.toInt(),
      activeBitDepth: (json['activeBitDepth'] as num?)?.toInt(),
      groupId: (json['groupId'] as String?)?.trim(),
      groupName: (json['groupName'] as String?)?.trim(),
      serverName: (json['serverName'] as String?)?.trim(),
    );
  }
}


// Helper functions to parse enums from strings
SendspinConnectionState _parseSendspinConnectionState(dynamic value) {
  final normalized = (value as String?)?.trim().toLowerCase();
  switch (normalized) {
    case 'connecting':
      return SendspinConnectionState.connecting;
    case 'online':
      return SendspinConnectionState.online;
    case 'reconnecting':
      return SendspinConnectionState.reconnecting;
    default:
      return SendspinConnectionState.offline;
  }
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

