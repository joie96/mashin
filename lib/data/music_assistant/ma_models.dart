// #region Enums

enum MediaType {
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

enum AlbumType {
  album,
  single,
  compilation,
  ep,
  unknown,
}

enum QueueOption {
  play,
  replace,
  next,
  replaceNext,
  add,
}

enum RepeatMode {
  off,
  one,
  all,
}

enum PlayerPlaybackState {
  idle,
  playing,
  paused,
  buffering,
  unknown,
}

// #endregion

// #region Core Helpers

MediaType mediaTypeFromJson(String? value) {
  switch ((value ?? '').trim().toLowerCase()) {
    case 'artist':
      return MediaType.artist;
    case 'album':
      return MediaType.album;
    case 'track':
      return MediaType.track;
    case 'playlist':
      return MediaType.playlist;
    case 'radio':
      return MediaType.radio;
    case 'audiobook':
      return MediaType.audiobook;
    case 'podcast':
      return MediaType.podcast;
    case 'podcast_episode':
      return MediaType.podcastEpisode;
    case 'folder':
      return MediaType.folder;
    case 'announcement':
      return MediaType.announcement;
    case 'flow_stream':
      return MediaType.flowStream;
    case 'plugin_source':
      return MediaType.pluginSource;
    case 'sound_effect':
      return MediaType.soundEffect;
    case 'genre':
      return MediaType.genre;
    default:
      return MediaType.unknown;
  }
}

String mediaTypeToJson(MediaType value) {
  switch (value) {
    case MediaType.podcastEpisode:
      return 'podcast_episode';
    case MediaType.flowStream:
      return 'flow_stream';
    case MediaType.pluginSource:
      return 'plugin_source';
    case MediaType.soundEffect:
      return 'sound_effect';
    default:
      return value.name;
  }
}

RepeatMode? repeatModeFromJson(String? value) {
  switch ((value ?? '').trim().toLowerCase()) {
    case 'off':
      return RepeatMode.off;
    case 'one':
      return RepeatMode.one;
    case 'all':
      return RepeatMode.all;
    default:
      return null;
  }
}

MediaItem mediaItemFromJson(Map<String, dynamic> json) {
  switch (mediaTypeFromJson(json['media_type'] as String?)) {
    case MediaType.artist:
      return Artist.fromJson(json);
    case MediaType.album:
      return Album.fromJson(json);
    case MediaType.track:
      return Track.fromJson(json);
    case MediaType.playlist:
      return Playlist.fromJson(json);
    default:
      return MediaItem.fromJson(json);
  }
}

// #endregion

// #region Base Models

class MediaItem {
  final String itemId;
  final String provider;
  final String name;
  final String? sortName;
  final String? uri;
  final MediaType mediaType;
  final List<ProviderMapping> providerMappings;
  final MediaItemMetadata? metadata;
  final bool favorite;

  const MediaItem({
    required this.itemId,
    required this.provider,
    required this.name,
    required this.mediaType,
    required this.providerMappings,
    this.sortName,
    this.uri,
    this.metadata,
    this.favorite = false,
  });

  factory MediaItem.fromJson(Map<String, dynamic> json) {
    final metadataJson = json['metadata'];
    final metadata = metadataJson is Map<String, dynamic> ? MediaItemMetadata.fromJson(metadataJson) : null;

    return MediaItem(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unbekannter Titel',
      sortName: (json['sort_name'] as String?)?.trim(),
      uri: (json['uri'] as String?)?.trim(),
      mediaType: mediaTypeFromJson(json['media_type'] as String?),
      providerMappings: (json['provider_mappings'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(ProviderMapping.fromJson)
              .toList() ??
          const <ProviderMapping>[],
      metadata: metadata,
      favorite: false,
    );
  }
}

class ProviderMapping {
  final String itemId;
  final String providerDomain;
  final String providerInstance;
  final bool available;
  final String? url;

  const ProviderMapping({
    required this.itemId,
    required this.providerDomain,
    required this.providerInstance,
    required this.available,
    this.url,
  });

  factory ProviderMapping.fromJson(Map<String, dynamic> json) {
    return ProviderMapping(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      providerDomain: (json['provider_domain'] as String?)?.trim() ?? '',
      providerInstance: (json['provider_instance'] as String?)?.trim() ?? '',
      available: json['available'] != false,
      url: (json['url'] as String?)?.trim(),
    );
  }
}

class MediaItemImage {
  static const List<int> _allowedSizes = <int>[0, 80, 160, 256, 512, 1024];
  static String _proxyBaseUrl = '';

  static String get currentBaseUrl => _proxyBaseUrl;

  final String type;
  final String path;
  final String provider;
  final bool remotelyAccessible;
  final String? proxyId;

  const MediaItemImage({
    required this.type,
    required this.path,
    required this.provider,
    required this.remotelyAccessible,
    this.proxyId,
  });

  factory MediaItemImage.fromJson(Map<String, dynamic> json) {
    return MediaItemImage(
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

class MediaItemMetadata {
  final String? description;
  final List<MediaItemImage> images;
  final List<String> genres;
  final String? label;
  final int? popularity;
  final String? cacheChecksum;

  const MediaItemMetadata({
    this.description,
    required this.images,
    required this.genres,
    this.label,
    this.popularity,
    this.cacheChecksum,
  });

  factory MediaItemMetadata.fromJson(Map<String, dynamic> json) {
    return MediaItemMetadata(
      description: (json['description'] as String?)?.trim(),
      images: (json['images'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(MediaItemImage.fromJson)
              .toList() ??
          const <MediaItemImage>[],
      genres: (json['genres'] as List?)?.whereType<String>().toList() ?? const <String>[],
      label: (json['label'] as String?)?.trim(),
      popularity: (json['popularity'] as num?)?.toInt(),
      cacheChecksum: (json['cache_checksum'] as String?)?.trim(),
    );
  }
}

class Artist implements MediaItem {
  @override
  final String itemId;
  @override
  final String provider;
  @override
  final String name;
  @override
  final String? sortName;
  @override
  final String? uri;
  @override
  final List<ProviderMapping> providerMappings;
  @override
  final MediaItemMetadata? metadata;
  @override
  final bool favorite;

  const Artist({
    required this.itemId,
    required this.provider,
    required this.name,
    this.sortName,
    this.uri,
    this.providerMappings = const <ProviderMapping>[],
    this.metadata,
    this.favorite = false,
  });

  @override
  MediaType get mediaType => MediaType.artist;

  MediaItemImage? get primaryImage {
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

  factory Artist.fromJson(Map<String, dynamic> json) {
    final metadataJson = json['metadata'];
    final metadata = metadataJson is Map<String, dynamic> ? MediaItemMetadata.fromJson(metadataJson) : null;

    return Artist(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unknown Artist',
      sortName: (json['sort_name'] as String?)?.trim(),
      uri: (json['uri'] as String?)?.trim(),
      providerMappings: (json['provider_mappings'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(ProviderMapping.fromJson)
              .toList() ??
          const <ProviderMapping>[],
      metadata: metadata,
      favorite: false,
    );
  }
}

class Album implements MediaItem {
  @override
  final String itemId;
  @override
  final String provider;
  @override
  final String name;
  @override
  final String? sortName;
  @override
  final String? uri;
  final int? year;
  final String? albumType;
  final List<Artist> artists;
  @override
  final List<ProviderMapping> providerMappings;
  @override
  final MediaItemMetadata? metadata;
  @override
  final bool favorite;

  const Album({
    required this.itemId,
    required this.provider,
    required this.name,
    required this.artists,
    this.sortName,
    this.uri,
    this.year,
    this.albumType,
    this.providerMappings = const <ProviderMapping>[],
    this.metadata,
    this.favorite = false,
  });

  @override
  MediaType get mediaType => MediaType.album;

  MediaItemImage? get primaryImage {
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

  factory Album.fromJson(Map<String, dynamic> json) {
    final metadataJson = json['metadata'];
    final metadata = metadataJson is Map<String, dynamic> ? MediaItemMetadata.fromJson(metadataJson) : null;

    return Album(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unknown Album',
      sortName: (json['sort_name'] as String?)?.trim(),
      uri: (json['uri'] as String?)?.trim(),
      year: (json['year'] as num?)?.toInt(),
      albumType: (json['album_type'] as String?)?.trim(),
      artists: (json['artists'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(Artist.fromJson)
              .toList() ??
          const <Artist>[],
      providerMappings: (json['provider_mappings'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(ProviderMapping.fromJson)
              .toList() ??
          const <ProviderMapping>[],
      metadata: metadata,
      favorite: false,
    );
  }
}

class Track implements MediaItem {
  @override
  final String itemId;
  @override
  final String provider;
  @override
  final String name;
  @override
  final String? sortName;
  @override
  final String? uri;
  final int duration;
  final List<Artist> artists;
  final Album? album;
  @override
  final List<ProviderMapping> providerMappings;
  @override
  final MediaItemMetadata? metadata;
  @override
  final bool favorite;

  const Track({
    required this.itemId,
    required this.provider,
    required this.name,
    required this.duration,
    required this.artists,
    this.sortName,
    this.uri,
    this.album,
    this.providerMappings = const <ProviderMapping>[],
    this.metadata,
    this.favorite = false,
  });

  @override
  MediaType get mediaType => MediaType.track;

  MediaItemImage? get primaryImage {
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

  factory Track.fromJson(Map<String, dynamic> json) {
    final albumJson = json['album'];
    final metadataJson = json['metadata'];
    final metadata = metadataJson is Map<String, dynamic> ? MediaItemMetadata.fromJson(metadataJson) : null;

    return Track(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unknown Track',
      sortName: (json['sort_name'] as String?)?.trim(),
      uri: (json['uri'] as String?)?.trim(),
      duration: (json['duration'] as num?)?.toInt() ?? 0,
      artists: (json['artists'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(Artist.fromJson)
              .toList() ??
          const <Artist>[],
      album: albumJson is Map<String, dynamic> ? Album.fromJson(albumJson) : null,
      providerMappings: (json['provider_mappings'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(ProviderMapping.fromJson)
              .toList() ??
          const <ProviderMapping>[],
      metadata: metadata,
      favorite: false,
    );
  }
}

class Playlist implements MediaItem {
  @override
  final String itemId;
  @override
  final String provider;
  @override
  final String name;
  @override
  final String? sortName;
  @override
  final String? uri;
  final String? owner;
  final bool isEditable;
  @override
  final List<ProviderMapping> providerMappings;
  @override
  final MediaItemMetadata? metadata;
  @override
  final bool favorite;

  const Playlist({
    required this.itemId,
    required this.provider,
    required this.name,
    required this.isEditable,
    this.sortName,
    this.uri,
    this.owner,
    this.providerMappings = const <ProviderMapping>[],
    this.metadata,
    this.favorite = false,
  });

  @override
  MediaType get mediaType => MediaType.playlist;

  MediaItemImage? get primaryImage {
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

  factory Playlist.fromJson(Map<String, dynamic> json) {
    final metadataJson = json['metadata'];
    final metadata = metadataJson is Map<String, dynamic> ? MediaItemMetadata.fromJson(metadataJson) : null;

    return Playlist(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unknown Playlist',
      sortName: (json['sort_name'] as String?)?.trim(),
      uri: (json['uri'] as String?)?.trim(),
      owner: (json['owner'] as String?)?.trim(),
      isEditable: json['is_editable'] == true,
      providerMappings: (json['provider_mappings'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(ProviderMapping.fromJson)
              .toList() ??
          const <ProviderMapping>[],
      metadata: metadata,
      favorite: false,
    );
  }
}

extension MediaItemImageSizing on MediaItem {
  String? imageUriForSize(int requestedSize) {
    final int normalizedSize = MediaItemImage.mapToAllowedSize(requestedSize);

    if (this is Track) {
      final Track track = this as Track;
      final MediaItemImage? albumImage = track.album?.primaryImage;
      if (albumImage != null) {
        return albumImage.getProxyPath(
          checksum: track.album?.metadata?.cacheChecksum,
          size: normalizedSize,
        );
      }

      final MediaItemImage? trackImage = track.primaryImage;
      if (trackImage == null) {
        return null;
      }

      return trackImage.getProxyPath(
        checksum: track.metadata?.cacheChecksum,
        size: normalizedSize,
      );
    }

    if (this is Album) {
      final Album album = this as Album;
      final MediaItemImage? image = album.primaryImage;
      if (image == null) {
        return null;
      }

      return image.getProxyPath(
        checksum: album.metadata?.cacheChecksum,
        size: normalizedSize,
      );
    }

    if (this is Artist) {
      final Artist artist = this as Artist;
      final MediaItemImage? image = artist.primaryImage;
      if (image == null) {
        return null;
      }

      return image.getProxyPath(
        checksum: artist.metadata?.cacheChecksum,
        size: normalizedSize,
      );
    }

    if (this is Playlist) {
      final Playlist playlist = this as Playlist;
      final MediaItemImage? image = playlist.primaryImage;
      if (image == null) {
        return null;
      }

      return image.getProxyPath(
        checksum: playlist.metadata?.cacheChecksum,
        size: normalizedSize,
      );
    }

    final MediaItemMetadata? meta = metadata;
    final MediaItemImage? image =
        (meta == null || meta.images.isEmpty) ? null : meta.images.first;
    if (image == null) {
      return null;
    }

    return image.getProxyPath(
      checksum: meta?.cacheChecksum,
      size: normalizedSize,
    );
  }
}

// #endregion

// #region Queue/Player Models

class QueueItem {
  final String queueId;
  final String queueItemId;
  final String name;
  final int? duration;
  final int? sortIndex;
  final bool? available;
  final Track? mediaItem;

  const QueueItem({
    required this.queueId,
    required this.queueItemId,
    required this.name,
    this.duration,
    this.sortIndex,
    this.available,
    this.mediaItem,
  });

  factory QueueItem.fromJson(Map<String, dynamic> json) {
    final mediaItemJson = json['media_item'];
    return QueueItem(
      queueId: (json['queue_id'] as String?)?.trim() ?? '',
      queueItemId: (json['queue_item_id'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? '',
      duration: (json['duration'] as num?)?.toInt(),
      sortIndex: (json['sort_index'] as num?)?.toInt(),
      available: json['available'] as bool?,
      mediaItem: mediaItemJson is Map<String, dynamic> ? Track.fromJson(mediaItemJson) : null,
    );
  }
}

class PlayerQueue {
  final String queueId;
  final bool active;
  final String displayName;
  final bool available;
  final int itemCount;
  final bool? shuffleEnabled;
  final RepeatMode? repeatMode;
  final bool? dontStopTheMusicEnabled;
  final int? currentIndex;
  final double? elapsedTime;
  final PlayerPlaybackState? state;
  final QueueItem? currentItem;

  const PlayerQueue({
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

  factory PlayerQueue.fromJson(Map<String, dynamic> json) {
    final currentItemJson = json['current_item'];
    return PlayerQueue(
      queueId: (json['queue_id'] as String?)?.trim() ?? '',
      active: json['active'] == true,
      displayName: (json['display_name'] as String?)?.trim() ?? '',
      available: json['available'] == true,
      itemCount: (json['items'] as num?)?.toInt() ?? 0,
      shuffleEnabled: json['shuffle_enabled'] as bool?,
      repeatMode: repeatModeFromJson((json['repeat_mode'] as String?)),
      dontStopTheMusicEnabled: json['dont_stop_the_music_enabled'] as bool?,
      currentIndex: (json['current_index'] as num?)?.toInt(),
      elapsedTime: (json['elapsed_time'] as num?)?.toDouble(),
      state: _playbackStateFromJson(json['state'] as String?),
      currentItem: currentItemJson is Map<String, dynamic> ? QueueItem.fromJson(currentItemJson) : null,
    );
  }
}

PlayerPlaybackState? _playbackStateFromJson(String? value) {
  switch ((value ?? '').trim().toLowerCase()) {
    case 'idle':
      return PlayerPlaybackState.idle;
    case 'playing':
      return PlayerPlaybackState.playing;
    case 'paused':
      return PlayerPlaybackState.paused;
    case 'buffering':
      return PlayerPlaybackState.buffering;
    case 'unknown':
      return PlayerPlaybackState.unknown;
    default:
      return null;
  }
}

class Player {
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

  const Player({
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

  factory Player.fromJson(Map<String, dynamic> json) {
    return Player(
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

class AuthUser {
  final String? userId;
  final String? username;
  final String? displayName;
  final String? avatarUrl;
  final String? role;
  final Map<String, dynamic>? preferences;

  const AuthUser({
    this.userId,
    this.username,
    this.displayName,
    this.avatarUrl,
    this.role,
    this.preferences,
  });

  factory AuthUser.fromJson(Map<String, dynamic> json) {
    return AuthUser(
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

class ServerInfo {
  final String serverId;
  final String name;
  final String? version;
  final String? serverVersion;
  final int? schemaVersion;
  final int? minSupportedSchemaVersion;
  final bool? homeAssistantAddon;

  const ServerInfo({
    required this.serverId,
    required this.name,
    this.version,
    this.serverVersion,
    this.schemaVersion,
    this.minSupportedSchemaVersion,
    this.homeAssistantAddon,
  });

  factory ServerInfo.fromJson(Map<String, dynamic> json) {
    return ServerInfo(
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

class ProviderManifest {
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

  const ProviderManifest({
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

  factory ProviderManifest.fromJson(Map<String, dynamic> json) {
    return ProviderManifest(
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

class ProviderManifestRegistry {
  static final Map<String, ProviderManifest> _byDomain = <String, ProviderManifest>{};

  static void registerAll(Iterable<ProviderManifest> manifests) {
    for (final manifest in manifests) {
      final key = manifest.domain.trim().toLowerCase();
      if (key.isEmpty) {
        continue;
      }
      _byDomain[key] = manifest;
    }
  }

  static void registerOne(ProviderManifest manifest) {
    registerAll(<ProviderManifest>[manifest]);
  }

  static ProviderManifest? getByProvider(String providerOrDomain) {
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
      final base = MediaItemImage.currentBaseUrl.trim();
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

class RecommendationFolder {
  final String title;
  final String? subtitle;
  final List<MediaItem> items;

  const RecommendationFolder({
    required this.title,
    this.subtitle,
    required this.items,
  });

  factory RecommendationFolder.fromJson(Map<String, dynamic> json) {
    return RecommendationFolder(
      title: (json['title'] as String?)?.trim() ?? (json['name'] as String?)?.trim() ?? 'Empfehlungen',
      subtitle: (json['subtitle'] as String?)?.trim() ?? (json['description'] as String?)?.trim(),
      items: (json['items'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(mediaItemFromJson)
              .toList() ??
          const <MediaItem>[],
    );
  }
}

class SearchResults {
  final List<Artist> artists;
  final List<Album> albums;
  final List<Track> tracks;
  final List<Playlist> playlists;
  final List<MediaItem> radio;

  const SearchResults({
    required this.artists,
    required this.albums,
    required this.tracks,
    required this.playlists,
    required this.radio,
  });

  factory SearchResults.fromJson(Map<String, dynamic> json) {
    return SearchResults(
      artists: (json['artists'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(Artist.fromJson)
              .toList() ??
          const <Artist>[],
      albums: (json['albums'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(Album.fromJson)
              .toList() ??
          const <Album>[],
      tracks: (json['tracks'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(Track.fromJson)
              .toList() ??
          const <Track>[],
      playlists: (json['playlists'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(Playlist.fromJson)
              .toList() ??
          const <Playlist>[],
      radio: (json['radio'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(mediaItemFromJson)
              .toList() ??
          const <MediaItem>[],
    );
  }
}

class BootstrapData {
  final ServerInfo serverInfo;
  final List<Player> players;
  final List<MediaItem> recentlyPlayed;

  const BootstrapData({
    required this.serverInfo,
    required this.players,
    required this.recentlyPlayed,
  });
}

// #endregion