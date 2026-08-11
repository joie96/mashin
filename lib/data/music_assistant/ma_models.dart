// #region Enums

enum MaMediaType {
  artist,
  album,
  track,
  playlist,
  radio,
  audiobook,
  podcast,
  podcastEpisode,
  folder,
  announcement,
  flowStream,
  pluginSource,
  soundEffect,
  genre,
  unknown,
}

enum MaAlbumType {
  album,
  single,
  compilation,
  ep,
  unknown,
}

enum MaQueueOption {
  play,
  replace,
  next,
  replaceNext,
  add,
}

enum MaRepeatMode {
  off,
  one,
  all,
}

enum MaPlaybackState {
  idle,
  playing,
  paused,
  buffering,
  unknown,
}

// #endregion

// #region Core Helpers

MaMediaType maMediaTypeFromJson(String? value) {
  switch ((value ?? '').trim().toLowerCase()) {
    case 'artist':
      return MaMediaType.artist;
    case 'album':
      return MaMediaType.album;
    case 'track':
      return MaMediaType.track;
    case 'playlist':
      return MaMediaType.playlist;
    case 'radio':
      return MaMediaType.radio;
    case 'audiobook':
      return MaMediaType.audiobook;
    case 'podcast':
      return MaMediaType.podcast;
    case 'podcast_episode':
      return MaMediaType.podcastEpisode;
    case 'folder':
      return MaMediaType.folder;
    case 'announcement':
      return MaMediaType.announcement;
    case 'flow_stream':
      return MaMediaType.flowStream;
    case 'plugin_source':
      return MaMediaType.pluginSource;
    case 'sound_effect':
      return MaMediaType.soundEffect;
    case 'genre':
      return MaMediaType.genre;
    default:
      return MaMediaType.unknown;
  }
}

String maMediaTypeToJson(MaMediaType value) {
  switch (value) {
    case MaMediaType.podcastEpisode:
      return 'podcast_episode';
    case MaMediaType.flowStream:
      return 'flow_stream';
    case MaMediaType.pluginSource:
      return 'plugin_source';
    case MaMediaType.soundEffect:
      return 'sound_effect';
    default:
      return value.name;
  }
}

MaRepeatMode? maRepeatModeFromJson(String? value) {
  switch ((value ?? '').trim().toLowerCase()) {
    case 'off':
      return MaRepeatMode.off;
    case 'one':
      return MaRepeatMode.one;
    case 'all':
      return MaRepeatMode.all;
    default:
      return null;
  }
}

// #endregion

// #region Base Models

class MaProviderMapping {
  final String itemId;
  final String providerDomain;
  final String providerInstance;
  final bool available;
  final String? url;

  const MaProviderMapping({
    required this.itemId,
    required this.providerDomain,
    required this.providerInstance,
    required this.available,
    this.url,
  });

  factory MaProviderMapping.fromJson(Map<String, dynamic> json) {
    return MaProviderMapping(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      providerDomain: (json['provider_domain'] as String?)?.trim() ?? '',
      providerInstance: (json['provider_instance'] as String?)?.trim() ?? '',
      available: json['available'] != false,
      url: (json['url'] as String?)?.trim(),
    );
  }
}

class MaMediaItemImage {
  static const List<int> _allowedSizes = <int>[0, 80, 160, 256, 512, 1024];
  static String _proxyBaseUrl = '';

  static String get currentBaseUrl => _proxyBaseUrl;

  final String type;
  final String path;
  final String provider;
  final bool remotelyAccessible;
  final String? proxyId;

  const MaMediaItemImage({
    required this.type,
    required this.path,
    required this.provider,
    required this.remotelyAccessible,
    this.proxyId,
  });

  factory MaMediaItemImage.fromJson(Map<String, dynamic> json) {
    return MaMediaItemImage(
      type: (json['type'] as String?)?.trim() ?? 'thumb',
      path: (json['path'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      remotelyAccessible: json['remotely_accessible'] == true,
      proxyId: (json['proxy_id'] as String?)?.trim(),
    );
  }

  static void configureProxyBaseUrl(String? baseUrl) {
    _proxyBaseUrl = (baseUrl ?? '').trim().replaceAll(RegExp(r'/+$'), '');
  }

  static int mapToAllowedSize(int requestedSize) {
    if (requestedSize <= 0) {
      return 0;
    }

    for (final allowed in _allowedSizes) {
      if (allowed > 0 && requestedSize <= allowed) {
        return allowed;
      }
    }

    return _allowedSizes.last;
  }

  String? getProxyPath({String? checksum, int? size}) {
    final id = proxyId?.trim() ?? '';
    final originalPath = path.trim();

    if (id.isEmpty || _proxyBaseUrl.isEmpty) {
      return originalPath.isEmpty ? null : originalPath;
    }

    final query = <String>[];
    final normalizedChecksum = checksum?.trim() ?? '';
    if (normalizedChecksum.isNotEmpty) {
      query.add('checksum=${Uri.encodeQueryComponent(normalizedChecksum)}');
    }

    if (size != null && size > 0) {
      final mappedSize = mapToAllowedSize(size);
      if (mappedSize > 0) {
        query.add('size=$mappedSize');
      }
    }

    final qs = query.isEmpty ? '' : '?${query.join('&')}';
    return '$_proxyBaseUrl/imageproxy/$id$qs';
  }
}

class MaMediaItemMetadata {
  final String? description;
  final List<MaMediaItemImage> images;
  final List<String> genres;
  final String? label;
  final int? popularity;
  final String? cacheChecksum;

  const MaMediaItemMetadata({
    this.description,
    required this.images,
    required this.genres,
    this.label,
    this.popularity,
    this.cacheChecksum,
  });

  factory MaMediaItemMetadata.fromJson(Map<String, dynamic> json) {
    return MaMediaItemMetadata(
      description: (json['description'] as String?)?.trim(),
      images: (json['images'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(MaMediaItemImage.fromJson)
              .toList() ??
          const <MaMediaItemImage>[],
      genres: (json['genres'] as List?)?.whereType<String>().toList() ?? const <String>[],
      label: (json['label'] as String?)?.trim(),
      popularity: (json['popularity'] as num?)?.toInt(),
      cacheChecksum: (json['cache_checksum'] as String?)?.trim(),
    );
  }
}

class MaMediaItem {
  final String itemId;
  final String provider;
  final String name;
  final String? sortName;
  final String? uri;
  final MaMediaType mediaType;
  final List<MaProviderMapping> providerMappings;
  final MaMediaItemMetadata? metadata;
  final bool favorite;
  final String? artistName;
  final String? imageUri;

  const MaMediaItem({
    required this.itemId,
    required this.provider,
    required this.name,
    required this.mediaType,
    required this.providerMappings,
    this.sortName,
    this.uri,
    this.metadata,
    this.favorite = false,
    this.artistName,
    this.imageUri,
  });

  factory MaMediaItem.fromJson(Map<String, dynamic> json) {
    String? resolvedArtistName;
    final artists = json['artists'];
    if (artists is List && artists.isNotEmpty) {
      final first = artists.first;
      if (first is Map<String, dynamic>) {
        resolvedArtistName = (first['name'] as String?)?.trim();
      }
    }

    final metadataJson = json['metadata'];
    final metadata = metadataJson is Map<String, dynamic> ? MaMediaItemMetadata.fromJson(metadataJson) : null;

    String? resolvedImageUri;
    if (metadata?.images.isNotEmpty == true) {
      resolvedImageUri = metadata!.images.first.getProxyPath(checksum: metadata.cacheChecksum);
    }

    return MaMediaItem(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unbekannter Titel',
      sortName: (json['sort_name'] as String?)?.trim(),
      uri: (json['uri'] as String?)?.trim(),
      mediaType: maMediaTypeFromJson(json['media_type'] as String?),
      providerMappings: (json['provider_mappings'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(MaProviderMapping.fromJson)
              .toList() ??
          const <MaProviderMapping>[],
      metadata: metadata,
      favorite: json['favorite'] == true,
      artistName: resolvedArtistName,
      imageUri: resolvedImageUri,
    );
  }
}

class MaArtist {
  final String itemId;
  final String provider;
  final String name;
  final MaMediaItemMetadata? metadata;

  const MaArtist({
    required this.itemId,
    required this.provider,
    required this.name,
    this.metadata,
  });

  MaMediaItemImage? get primaryImage {
    if (metadata == null || metadata!.images.isEmpty) {
      return null;
    }
    return metadata!.images.first;
  }

  String? get imageUri {
    final image = primaryImage;
    if (image == null) {
      return null;
    }
    return image.getProxyPath(checksum: metadata?.cacheChecksum);
  }

  factory MaArtist.fromJson(Map<String, dynamic> json) {
    final metadataJson = json['metadata'];
    final metadata = metadataJson is Map<String, dynamic> ? MaMediaItemMetadata.fromJson(metadataJson) : null;

    return MaArtist(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unknown Artist',
      metadata: metadata,
    );
  }
}

class MaAlbum {
  final String itemId;
  final String provider;
  final String name;
  final int? year;
  final String? albumType;
  final List<MaArtist> artists;
  final MaMediaItemMetadata? metadata;

  const MaAlbum({
    required this.itemId,
    required this.provider,
    required this.name,
    required this.artists,
    this.year,
    this.albumType,
    this.metadata,
  });

  MaMediaItemImage? get primaryImage {
    if (metadata == null || metadata!.images.isEmpty) {
      return null;
    }
    return metadata!.images.first;
  }

  String? get imageUri {
    final image = primaryImage;
    if (image == null) {
      return null;
    }
    return image.getProxyPath(checksum: metadata?.cacheChecksum);
  }

  factory MaAlbum.fromJson(Map<String, dynamic> json) {
    final metadataJson = json['metadata'];
    final metadata = metadataJson is Map<String, dynamic> ? MaMediaItemMetadata.fromJson(metadataJson) : null;

    return MaAlbum(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unknown Album',
      year: (json['year'] as num?)?.toInt(),
      albumType: (json['album_type'] as String?)?.trim(),
      artists: (json['artists'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(MaArtist.fromJson)
              .toList() ??
          const <MaArtist>[],
      metadata: metadata,
    );
  }
}

class MaTrack {
  final String itemId;
  final String provider;
  final String name;
  final int duration;
  final List<MaArtist> artists;
  final MaAlbum? album;
  final MaMediaItemMetadata? metadata;

  const MaTrack({
    required this.itemId,
    required this.provider,
    required this.name,
    required this.duration,
    required this.artists,
    this.album,
    this.metadata,
  });

  MaMediaItemImage? get primaryImage {
    final albumImage = album?.primaryImage;
    if (albumImage != null) {
      return albumImage;
    }

    if (metadata == null || metadata!.images.isEmpty) {
      return null;
    }

    return metadata!.images.first;
  }

  String? get imageUri {
    final albumImage = album?.primaryImage;
    if (albumImage != null) {
      return albumImage.getProxyPath(checksum: album?.metadata?.cacheChecksum);
    }

    final image = primaryImage;
    if (image == null) {
      return null;
    }

    return image.getProxyPath(checksum: metadata?.cacheChecksum);
  }

  factory MaTrack.fromJson(Map<String, dynamic> json) {
    final albumJson = json['album'];
    final metadataJson = json['metadata'];
    final metadata = metadataJson is Map<String, dynamic> ? MaMediaItemMetadata.fromJson(metadataJson) : null;

    return MaTrack(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unknown Track',
      duration: (json['duration'] as num?)?.toInt() ?? 0,
      artists: (json['artists'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(MaArtist.fromJson)
              .toList() ??
          const <MaArtist>[],
      album: albumJson is Map<String, dynamic> ? MaAlbum.fromJson(albumJson) : null,
      metadata: metadata,
    );
  }
}

class MaPlaylist {
  final String itemId;
  final String provider;
  final String name;
  final String? owner;
  final bool isEditable;
  final MaMediaItemMetadata? metadata;

  const MaPlaylist({
    required this.itemId,
    required this.provider,
    required this.name,
    required this.isEditable,
    this.owner,
    this.metadata,
  });

  MaMediaItemImage? get primaryImage {
    if (metadata == null || metadata!.images.isEmpty) {
      return null;
    }
    return metadata!.images.first;
  }

  String? get imageUri {
    final image = primaryImage;
    if (image == null) {
      return null;
    }
    return image.getProxyPath(checksum: metadata?.cacheChecksum);
  }

  factory MaPlaylist.fromJson(Map<String, dynamic> json) {
    final metadataJson = json['metadata'];
    final metadata = metadataJson is Map<String, dynamic> ? MaMediaItemMetadata.fromJson(metadataJson) : null;

    return MaPlaylist(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unknown Playlist',
      owner: (json['owner'] as String?)?.trim(),
      isEditable: json['is_editable'] == true,
      metadata: metadata,
    );
  }
}

// #endregion

// #region Queue/Player Models

class MaQueueItem {
  final String queueId;
  final String queueItemId;
  final String name;
  final int? duration;
  final int? sortIndex;
  final bool? available;
  final MaTrack? mediaItem;

  const MaQueueItem({
    required this.queueId,
    required this.queueItemId,
    required this.name,
    this.duration,
    this.sortIndex,
    this.available,
    this.mediaItem,
  });

  factory MaQueueItem.fromJson(Map<String, dynamic> json) {
    final mediaItemJson = json['media_item'];
    return MaQueueItem(
      queueId: (json['queue_id'] as String?)?.trim() ?? '',
      queueItemId: (json['queue_item_id'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? '',
      duration: (json['duration'] as num?)?.toInt(),
      sortIndex: (json['sort_index'] as num?)?.toInt(),
      available: json['available'] as bool?,
      mediaItem: mediaItemJson is Map<String, dynamic> ? MaTrack.fromJson(mediaItemJson) : null,
    );
  }
}

class MaPlayerQueue {
  final String queueId;
  final bool active;
  final String displayName;
  final bool available;
  final int itemCount;
  final bool? shuffleEnabled;
  final MaRepeatMode? repeatMode;
  final bool? dontStopTheMusicEnabled;
  final int? currentIndex;
  final double? elapsedTime;
  final MaPlaybackState? state;
  final MaQueueItem? currentItem;

  const MaPlayerQueue({
    required this.queueId,
    required this.active,
    required this.displayName,
    required this.available,
    required this.itemCount,
    this.shuffleEnabled,
    this.repeatMode,
    this.dontStopTheMusicEnabled,
    this.currentIndex,
    this.elapsedTime,
    this.state,
    this.currentItem,
  });

  factory MaPlayerQueue.fromJson(Map<String, dynamic> json) {
    final currentItemJson = json['current_item'];
    return MaPlayerQueue(
      queueId: (json['queue_id'] as String?)?.trim() ?? '',
      active: json['active'] == true,
      displayName: (json['display_name'] as String?)?.trim() ?? '',
      available: json['available'] == true,
      itemCount: (json['items'] as num?)?.toInt() ?? 0,
      shuffleEnabled: json['shuffle_enabled'] as bool?,
      repeatMode: maRepeatModeFromJson((json['repeat_mode'] as String?)),
      dontStopTheMusicEnabled: json['dont_stop_the_music_enabled'] as bool?,
      currentIndex: (json['current_index'] as num?)?.toInt(),
      elapsedTime: (json['elapsed_time'] as num?)?.toDouble(),
      state: _playbackStateFromJson(json['state'] as String?),
      currentItem: currentItemJson is Map<String, dynamic> ? MaQueueItem.fromJson(currentItemJson) : null,
    );
  }
}

MaPlaybackState? _playbackStateFromJson(String? value) {
  switch ((value ?? '').trim().toLowerCase()) {
    case 'idle':
      return MaPlaybackState.idle;
    case 'playing':
      return MaPlaybackState.playing;
    case 'paused':
      return MaPlaybackState.paused;
    case 'buffering':
      return MaPlaybackState.buffering;
    case 'unknown':
      return MaPlaybackState.unknown;
    default:
      return null;
  }
}

class MaPlayer {
  final String playerId;
  final String name;
  final bool available;
  final int? volumeLevel;
  final bool? volumeMuted;
  final String? state;
  final bool? powered;
  final String provider;
  final String type;
  final List<String> supportedFeatures;
  final List<String> groupChilds;
  final String? activeSource;

  const MaPlayer({
    required this.playerId,
    required this.name,
    required this.available,
    required this.provider,
    required this.type,
    required this.supportedFeatures,
    required this.groupChilds,
    this.volumeLevel,
    this.volumeMuted,
    this.state,
    this.powered,
    this.activeSource,
  });

  factory MaPlayer.fromJson(Map<String, dynamic> json) {
    return MaPlayer(
      playerId: (json['player_id'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unbekannter Player',
      available: json['available'] == true,
      volumeLevel: (json['volume_level'] as num?)?.toInt(),
      volumeMuted: json['volume_muted'] as bool?,
      state: (json['state'] as String?)?.trim(),
      powered: json['powered'] as bool?,
      provider: (json['provider'] as String?)?.trim() ?? '',
      type: (json['type'] as String?)?.trim() ?? 'player',
      supportedFeatures: (json['supported_features'] as List?)?.whereType<String>().toList() ?? const <String>[],
      groupChilds: (json['group_childs'] as List?)?.whereType<String>().toList() ?? const <String>[],
      activeSource: (json['active_source'] as String?)?.trim(),
    );
  }
}

// #endregion

// #region Auth/General Models

class MaAuthUser {
  final String? userId;
  final String? username;
  final String? displayName;
  final String? avatarUrl;
  final String? role;
  final Map<String, dynamic>? preferences;

  const MaAuthUser({
    this.userId,
    this.username,
    this.displayName,
    this.avatarUrl,
    this.role,
    this.preferences,
  });

  factory MaAuthUser.fromJson(Map<String, dynamic> json) {
    return MaAuthUser(
      userId: (json['user_id'] as String?)?.trim(),
      username: (json['username'] as String?)?.trim(),
      displayName: (json['display_name'] as String?)?.trim(),
      avatarUrl: (json['avatar_url'] as String?)?.trim(),
      role: (json['role'] as String?)?.trim(),
      preferences: json['preferences'] is Map<String, dynamic>
          ? Map<String, dynamic>.from(json['preferences'] as Map<String, dynamic>)
          : null,
    );
  }
}

class MaServerInfo {
  final String serverId;
  final String name;
  final String? version;
  final String? serverVersion;
  final int? schemaVersion;
  final int? minSupportedSchemaVersion;
  final bool? homeAssistantAddon;

  const MaServerInfo({
    required this.serverId,
    required this.name,
    this.version,
    this.serverVersion,
    this.schemaVersion,
    this.minSupportedSchemaVersion,
    this.homeAssistantAddon,
  });

  factory MaServerInfo.fromJson(Map<String, dynamic> json) {
    return MaServerInfo(
      serverId: (json['server_id'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Music Assistant',
      version: (json['version'] as String?)?.trim(),
      serverVersion: (json['server_version'] as String?)?.trim(),
      schemaVersion: (json['schema_version'] as num?)?.toInt(),
      minSupportedSchemaVersion: (json['min_supported_schema_version'] as num?)?.toInt(),
      homeAssistantAddon: json['homeassistant_addon'] as bool?,
    );
  }
}

class MaProviderManifest {
  final String domain;
  final String? name;
  final String? description;
  final String? icon;
  final String? iconSvg;
  final String? iconSvgDark;
  final String? iconSvgMonochrome;
  final String? type;
  final bool? builtin;
  final Map<String, dynamic> raw;

  const MaProviderManifest({
    required this.domain,
    this.name,
    this.description,
    this.icon,
    this.iconSvg,
    this.iconSvgDark,
    this.iconSvgMonochrome,
    this.type,
    this.builtin,
    required this.raw,
  });

  factory MaProviderManifest.fromJson(Map<String, dynamic> json) {
    return MaProviderManifest(
      domain: (json['domain'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim(),
      description: (json['description'] as String?)?.trim(),
      icon: (json['icon'] as String?)?.trim(),
      iconSvg: (json['icon_svg'] as String?)?.trim(),
      iconSvgDark: (json['icon_svg_dark'] as String?)?.trim(),
      iconSvgMonochrome: (json['icon_svg_monochrome'] as String?)?.trim(),
      type: (json['type'] as String?)?.trim(),
      builtin: json['builtin'] as bool?,
      raw: Map<String, dynamic>.from(json),
    );
  }

  String? get preferredIconSvg => iconSvg?.trim();
}

class MaProviderManifestRegistry {
  static final Map<String, MaProviderManifest> _byDomain = <String, MaProviderManifest>{};

  static void registerAll(Iterable<MaProviderManifest> manifests) {
    for (final manifest in manifests) {
      final key = manifest.domain.trim().toLowerCase();
      if (key.isEmpty) {
        continue;
      }
      _byDomain[key] = manifest;
    }
  }

  static void registerOne(MaProviderManifest manifest) {
    registerAll(<MaProviderManifest>[manifest]);
  }

  static MaProviderManifest? getByProvider(String providerOrDomain) {
    final domain = _providerDomain(providerOrDomain);
    if (domain.isEmpty) {
      return null;
    }
    return _byDomain[domain];
  }

  static String? iconSvgForProvider(String providerOrDomain) {
    final raw = getByProvider(providerOrDomain)?.preferredIconSvg?.trim();
    if (raw == null || raw.isEmpty) {
      return null;
    }

    if (raw.startsWith('http://') || raw.startsWith('https://') || raw.startsWith('data:image/')) {
      return raw;
    }

    if (raw.startsWith('/')) {
      final base = MaMediaItemImage.currentBaseUrl.trim();
      if (base.isNotEmpty) {
        return '$base$raw';
      }
    }

    return raw;
  }

  static String _providerDomain(String providerOrDomain) {
    final value = providerOrDomain.trim().toLowerCase();
    if (value.isEmpty) {
      return '';
    }

    final domain = value.split('--').first.trim();
    return domain;
  }
}

class MaRecommendationFolder {
  final String title;
  final String? subtitle;
  final List<MaMediaItem> items;

  const MaRecommendationFolder({
    required this.title,
    this.subtitle,
    required this.items,
  });

  factory MaRecommendationFolder.fromJson(Map<String, dynamic> json) {
    return MaRecommendationFolder(
      title: (json['title'] as String?)?.trim() ?? (json['name'] as String?)?.trim() ?? 'Empfehlungen',
      subtitle: (json['subtitle'] as String?)?.trim() ?? (json['description'] as String?)?.trim(),
      items: (json['items'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(MaMediaItem.fromJson)
              .toList() ??
          const <MaMediaItem>[],
    );
  }
}

class MaSearchResults {
  final List<MaArtist> artists;
  final List<MaAlbum> albums;
  final List<MaTrack> tracks;
  final List<MaPlaylist> playlists;
  final List<MaMediaItem> radio;

  const MaSearchResults({
    required this.artists,
    required this.albums,
    required this.tracks,
    required this.playlists,
    required this.radio,
  });

  factory MaSearchResults.fromJson(Map<String, dynamic> json) {
    return MaSearchResults(
      artists: (json['artists'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(MaArtist.fromJson)
              .toList() ??
          const <MaArtist>[],
      albums: (json['albums'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(MaAlbum.fromJson)
              .toList() ??
          const <MaAlbum>[],
      tracks: (json['tracks'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(MaTrack.fromJson)
              .toList() ??
          const <MaTrack>[],
      playlists: (json['playlists'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(MaPlaylist.fromJson)
              .toList() ??
          const <MaPlaylist>[],
      radio: (json['radio'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(MaMediaItem.fromJson)
              .toList() ??
          const <MaMediaItem>[],
    );
  }
}

class MaBootstrapData {
  final MaServerInfo serverInfo;
  final List<MaPlayer> players;
  final List<MaMediaItem> recentlyPlayed;

  const MaBootstrapData({
    required this.serverInfo,
    required this.players,
    required this.recentlyPlayed,
  });
}

// #endregion
