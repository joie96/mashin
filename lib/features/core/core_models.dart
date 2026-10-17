// Music Assistant media and playback enumerations.

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
  collection,
  announcement,
  flowStream,
  pluginSource,
  audioSource,
  soundEffect,
  genre,
  unknown,
}

enum AlbumType { album, single, live, soundtrack, compilation, ep, unknown }

enum ContentType { pcm, opus, flac, mp3, aac, wav, ogg, unknown }

enum QueueOption { play, replace, next, replaceNext, add }

enum RepeatMode { off, one, all }

enum LogLevelPreference {
  trace,
  debug,
  info,
  warn,
  error;

  static LogLevelPreference parse(String value) {
    return switch (value.trim().toLowerCase()) {
      'trace' => LogLevelPreference.trace,
      'debug' => LogLevelPreference.debug,
      'info' => LogLevelPreference.info,
      'warn' || 'warning' => LogLevelPreference.warn,
      'error' => LogLevelPreference.error,
      _ => LogLevelPreference.info,
    };
  }
}

enum PlayState {
  uninitialized,
  idle,
  buffering,
  playing,
  paused,
  seeking,
  error,
  unknown,
}

// Media JSON conversion and Music Assistant command serialization.

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
    case 'collection':
      return MediaType.collection;
    case 'announcement':
      return MediaType.announcement;
    case 'flow_stream':
      return MediaType.flowStream;
    case 'plugin_source':
      return MediaType.pluginSource;
    case 'audio_source':
      return MediaType.audioSource;
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
    case MediaType.audioSource:
      return 'audio_source';
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

ContentType _contentTypeFromJson(String? value) {
  return ContentType.values.firstWhere(
    (ContentType type) => type.name == (value ?? '').trim().toLowerCase(),
    orElse: () => ContentType.unknown,
  );
}

AlbumType _albumTypeFromJson(String? value) {
  return AlbumType.values.firstWhere(
    (AlbumType type) => type.name == (value ?? '').trim().toLowerCase(),
    orElse: () => AlbumType.unknown,
  );
}

List<List<String>> _stringPairsFromJson(Object? value) {
  return (value as List?)
          ?.whereType<List>()
          .map((List pair) => pair.whereType<String>().toList(growable: false))
          .toList(growable: false) ??
      const <List<String>>[];
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
    case MediaType.radio:
      return Radio.fromJson(json);
    case MediaType.audiobook:
      return Audiobook.fromJson(json);
    case MediaType.podcast:
      return Podcast.fromJson(json);
    case MediaType.podcastEpisode:
      return PodcastEpisode.fromJson(json);
    case MediaType.folder:
    case MediaType.collection:
    case MediaType.announcement:
    case MediaType.flowStream:
    case MediaType.pluginSource:
    case MediaType.audioSource:
    case MediaType.soundEffect:
    case MediaType.genre:
      return GenericMediaItem.fromJson(json);
    default:
      throw FormatException(
        'Unknown Music Assistant media type: ${json['media_type']}',
      );
  }
}

Map<String, dynamic> createMusicAssistantMediaItemPayload(MediaItem mediaItem) {
  final payload = <String, dynamic>{
    'item_id': mediaItem.itemId,
    'provider': mediaItem.provider,
    'name': mediaItem.name.trim().isEmpty ? mediaItem.itemId : mediaItem.name,
    'version': mediaItem.version,
    'sort_name': mediaItem.sortName,
    'uri': mediaItem.uri,
    'external_ids': mediaItem.externalIds,
    'is_playable': mediaItem.isPlayable,
    'available': mediaItem.available,
    'media_type': mediaTypeToJson(mediaItem.mediaType),
    'provider_mappings': mediaItem.providerMappings
        .map(
          (mapping) => <String, dynamic>{
            'item_id': mapping.itemId,
            'provider_domain': mapping.providerDomain,
            'provider_instance': mapping.providerInstance,
            'available': mapping.available,
            'in_library': mapping.inLibrary,
            'is_unique': mapping.isUnique,
            'audio_format': _createAudioFormatPayload(mapping.audioFormat),
            'url': mapping.url,
            'details': mapping.details,
          },
        )
        .toList(growable: false),
    'metadata': _createMusicAssistantMediaItemMetadataPayload(
      mediaItem.metadata,
    ),
    'favorite': mediaItem.favorite,
    'position': mediaItem.position,
    'date_added': mediaItem.dateAdded,
  };

  if (mediaItem is Track) {
    payload['duration'] = mediaItem.duration;
    payload['artists'] = mediaItem.artists
        .map(createMusicAssistantMediaItemPayload)
        .toList(growable: false);
    payload['album'] = mediaItem.album == null
        ? null
        : createMusicAssistantMediaItemPayload(mediaItem.album!);
    payload['image'] = _createMusicAssistantMediaItemImagePayload(
      mediaItem.image,
    );
    payload['disc_number'] = mediaItem.discNumber;
    payload['track_number'] = mediaItem.trackNumber;
  } else if (mediaItem is Album) {
    payload['year'] = mediaItem.year;
    payload['album_type'] = mediaItem.albumType.name;
    payload['artists'] = mediaItem.artists
        .map(createMusicAssistantMediaItemPayload)
        .toList(growable: false);
  } else if (mediaItem is Artist) {
    payload['artist_type'] = mediaItem.artistType;
  } else if (mediaItem is PodcastEpisode) {
    payload['podcast'] = createMusicAssistantMediaItemPayload(
      mediaItem.podcast,
    );
    payload['duration'] = mediaItem.duration;
    payload['resume_position_ms'] = mediaItem.resumePositionMs;
  } else if (mediaItem is Podcast) {
    payload['publisher'] = mediaItem.publisher;
    payload['total_episodes'] = mediaItem.totalEpisodes;
  } else if (mediaItem is Playlist) {
    payload['owner'] = mediaItem.owner;
    payload['is_editable'] = mediaItem.isEditable;
    payload['is_dynamic'] = mediaItem.isDynamic;
  }

  return payload;
}

Map<String, dynamic>? _createMusicAssistantMediaItemImagePayload(
  MediaItemImage? image,
) {
  if (image == null) {
    return null;
  }

  return <String, dynamic>{
    'type': image.type,
    'path': image.path,
    'provider': image.provider,
    'remotely_accessible': image.remotelyAccessible,
    'proxy_id': image.proxyId,
  };
}

Map<String, dynamic>? _createMusicAssistantMediaItemMetadataPayload(
  MediaItemMetadata? metadata,
) {
  if (metadata == null) {
    return null;
  }

  return <String, dynamic>{
    'description': metadata.description,
    'description_language': metadata.descriptionLanguage,
    'images': metadata.images
        .map(_createMusicAssistantMediaItemImagePayload)
        .toList(growable: false),
    'genres': metadata.genres,
    'label': metadata.label,
    'popularity': metadata.popularity,
    'cache_checksum': metadata.cacheChecksum,
    'explicit': metadata.explicit,
    'lyrics': metadata.lyrics,
    'transcript': metadata.transcript,
  };
}

Map<String, dynamic>? _createAudioFormatPayload(AudioFormat? format) {
  if (format == null) {
    return null;
  }
  return <String, dynamic>{
    'content_type': format.contentType.name,
    'codec_type': format.codecType.name,
    'sample_rate': format.sampleRate,
    'bit_depth': format.bitDepth,
    'channels': format.channels,
    'output_format_str': format.outputFormatStr,
    'bit_rate': format.bitRate,
  };
}

// Canonical Music Assistant media domain models.

class MediaItem {
  final String itemId;
  final String provider;
  final String name;
  final String version;
  final String? sortName;
  final String? uri;
  final List<List<String>> externalIds;
  final bool isPlayable;
  final bool? available;
  final MediaType mediaType;
  final List<ProviderMapping> providerMappings;
  final MediaItemMetadata? metadata;
  final bool favorite;
  final int? position;
  final String? dateAdded;

  const MediaItem({
    required this.itemId,
    required this.provider,
    required this.name,
    required this.mediaType,
    required this.providerMappings,
    this.version = '',
    this.sortName,
    this.uri,
    this.externalIds = const <List<String>>[],
    this.isPlayable = true,
    this.available,
    this.metadata,
    this.favorite = false,
    this.position,
    this.dateAdded,
  });

  factory MediaItem.fromJson(Map<String, dynamic> json) {
    return MediaItem(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unbekannter Titel',
      version: (json['version'] as String?)?.trim() ?? '',
      sortName: (json['sort_name'] as String?)?.trim(),
      uri: (json['uri'] as String?)?.trim(),
      externalIds: _stringPairsFromJson(json['external_ids']),
      isPlayable: json['is_playable'] != false,
      available: json['available'] as bool?,
      mediaType: mediaTypeFromJson(json['media_type'] as String?),
      providerMappings:
          (json['provider_mappings'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(ProviderMapping.fromJson)
              .toList() ??
          const <ProviderMapping>[],
      metadata: _metadataFromJson(json),
      favorite: json['favorite'] == true,
      position: (json['position'] as num?)?.toInt(),
      dateAdded: (json['date_added'] as String?)?.trim(),
    );
  }
}

class ProviderMapping {
  final String itemId;
  final String providerDomain;
  final String providerInstance;
  final bool available;
  final bool? inLibrary;
  final bool? isUnique;
  final AudioFormat? audioFormat;
  final String? url;
  final String? details;

  const ProviderMapping({
    required this.itemId,
    required this.providerDomain,
    required this.providerInstance,
    required this.available,
    this.inLibrary,
    this.isUnique,
    this.audioFormat,
    this.url,
    this.details,
  });

  factory ProviderMapping.fromJson(Map<String, dynamic> json) {
    return ProviderMapping(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      providerDomain: (json['provider_domain'] as String?)?.trim() ?? '',
      providerInstance: (json['provider_instance'] as String?)?.trim() ?? '',
      available: json['available'] != false,
      inLibrary: json['in_library'] as bool?,
      isUnique: json['is_unique'] as bool?,
      audioFormat: json['audio_format'] is Map<String, dynamic>
          ? AudioFormat.fromJson(json['audio_format'] as Map<String, dynamic>)
          : null,
      url: (json['url'] as String?)?.trim(),
      details: (json['details'] as String?)?.trim(),
    );
  }
}

extension MediaItemAvailability on MediaItem {
  bool get isAvailable =>
      available != false &&
      (providerMappings.isEmpty ||
          providerMappings.any((mapping) => mapping.available));

  bool get canPlay => isPlayable && isAvailable;
}

class AudioFormat {
  final ContentType contentType;
  final ContentType codecType;
  final int sampleRate;
  final int bitDepth;
  final int channels;
  final String outputFormatStr;
  final int? bitRate;

  const AudioFormat({
    this.contentType = ContentType.unknown,
    this.codecType = ContentType.unknown,
    this.sampleRate = 44100,
    this.bitDepth = 16,
    this.channels = 2,
    this.outputFormatStr = '',
    this.bitRate,
  });

  factory AudioFormat.fromJson(Map<String, dynamic> json) => AudioFormat(
    contentType: _contentTypeFromJson(json['content_type'] as String?),
    codecType: _contentTypeFromJson(json['codec_type'] as String?),
    sampleRate: (json['sample_rate'] as num?)?.toInt() ?? 44100,
    bitDepth: (json['bit_depth'] as num?)?.toInt() ?? 16,
    channels: (json['channels'] as num?)?.toInt() ?? 2,
    outputFormatStr: (json['output_format_str'] as String?)?.trim() ?? '',
    bitRate: (json['bit_rate'] as num?)?.toInt(),
  );
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
  final String? descriptionLanguage;
  final List<MediaItemImage> images;
  final List<String> genres;
  final String? label;
  final int? popularity;
  final String? cacheChecksum;
  final bool? explicit;
  final String? lyrics;
  final String? transcript;

  const MediaItemMetadata({
    this.description,
    this.descriptionLanguage,
    required this.images,
    required this.genres,
    this.label,
    this.popularity,
    this.cacheChecksum,
    this.explicit,
    this.lyrics,
    this.transcript,
  });

  factory MediaItemMetadata.fromJson(Map<String, dynamic> json) {
    return MediaItemMetadata(
      description: (json['description'] as String?)?.trim(),
      descriptionLanguage: (json['description_language'] as String?)?.trim(),
      images:
          (json['images'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(MediaItemImage.fromJson)
              .toList() ??
          const <MediaItemImage>[],
      genres:
          (json['genres'] as List?)?.whereType<String>().toList() ??
          const <String>[],
      label: (json['label'] as String?)?.trim(),
      popularity: (json['popularity'] as num?)?.toInt(),
      cacheChecksum: (json['cache_checksum'] as String?)?.trim(),
      explicit: json['explicit'] as bool?,
      lyrics: (json['lyrics'] as String?)?.trim(),
      transcript: (json['transcript'] as String?)?.trim(),
    );
  }
}

MediaItemMetadata? _metadataFromJson(Map<String, dynamic> json) {
  final metadataJson = json['metadata'];
  final imageJson = json['image'];
  if (metadataJson is! Map<String, dynamic> &&
      imageJson is! Map<String, dynamic>) {
    return null;
  }
  final metadata = metadataJson is Map<String, dynamic>
      ? Map<String, dynamic>.from(metadataJson)
      : <String, dynamic>{};
  if ((metadata['images'] is! List || (metadata['images'] as List).isEmpty) &&
      imageJson is Map<String, dynamic>) {
    metadata['images'] = <Map<String, dynamic>>[imageJson];
  }
  return MediaItemMetadata.fromJson(metadata);
}

class Artist implements MediaItem {
  @override
  final String itemId;
  @override
  final String provider;
  @override
  final String name;
  @override
  final String version;
  @override
  final String? sortName;
  @override
  final String? uri;
  @override
  final List<List<String>> externalIds;
  @override
  final bool isPlayable;
  @override
  final bool? available;
  @override
  final List<ProviderMapping> providerMappings;
  @override
  final MediaItemMetadata? metadata;
  @override
  final bool favorite;
  @override
  final int? position;
  @override
  final String? dateAdded;
  final String? artistType;

  const Artist({
    required this.itemId,
    required this.provider,
    required this.name,
    this.version = '',
    this.sortName,
    this.uri,
    this.externalIds = const <List<String>>[],
    this.isPlayable = true,
    this.available,
    this.providerMappings = const <ProviderMapping>[],
    this.metadata,
    this.favorite = false,
    this.position,
    this.dateAdded,
    this.artistType,
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
    return Artist(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unknown Artist',
      version: (json['version'] as String?)?.trim() ?? '',
      sortName: (json['sort_name'] as String?)?.trim(),
      uri: (json['uri'] as String?)?.trim(),
      externalIds: _stringPairsFromJson(json['external_ids']),
      isPlayable: json['is_playable'] != false,
      available: json['available'] as bool?,
      providerMappings:
          (json['provider_mappings'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(ProviderMapping.fromJson)
              .toList() ??
          const <ProviderMapping>[],
      metadata: _metadataFromJson(json),
      favorite: json['favorite'] == true,
      position: (json['position'] as num?)?.toInt(),
      dateAdded: (json['date_added'] as String?)?.trim(),
      artistType: (json['artist_type'] as String?)?.trim(),
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
  final String version;
  @override
  final String? sortName;
  @override
  final String? uri;
  @override
  final List<List<String>> externalIds;
  @override
  final bool isPlayable;
  @override
  final bool? available;
  final int? year;
  final AlbumType albumType;
  final List<Artist> artists;
  @override
  final List<ProviderMapping> providerMappings;
  @override
  final MediaItemMetadata? metadata;
  @override
  final bool favorite;
  @override
  final int? position;
  @override
  final String? dateAdded;

  const Album({
    required this.itemId,
    required this.provider,
    required this.name,
    required this.artists,
    this.version = '',
    this.sortName,
    this.uri,
    this.externalIds = const <List<String>>[],
    this.isPlayable = true,
    this.available,
    this.year,
    this.albumType = AlbumType.unknown,
    this.providerMappings = const <ProviderMapping>[],
    this.metadata,
    this.favorite = false,
    this.position,
    this.dateAdded,
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
    return Album(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unknown Album',
      version: (json['version'] as String?)?.trim() ?? '',
      sortName: (json['sort_name'] as String?)?.trim(),
      uri: (json['uri'] as String?)?.trim(),
      externalIds: _stringPairsFromJson(json['external_ids']),
      isPlayable: json['is_playable'] != false,
      available: json['available'] as bool?,
      year: (json['year'] as num?)?.toInt(),
      albumType: _albumTypeFromJson(json['album_type'] as String?),
      artists:
          (json['artists'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(Artist.fromJson)
              .toList() ??
          const <Artist>[],
      providerMappings:
          (json['provider_mappings'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(ProviderMapping.fromJson)
              .toList() ??
          const <ProviderMapping>[],
      metadata: _metadataFromJson(json),
      favorite: json['favorite'] == true,
      position: (json['position'] as num?)?.toInt(),
      dateAdded: (json['date_added'] as String?)?.trim(),
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
  final String version;
  @override
  final String? sortName;
  @override
  final String? uri;
  @override
  final List<List<String>> externalIds;
  @override
  final bool isPlayable;
  @override
  final bool? available;
  final int duration;
  final List<Artist> artists;
  final Album? album;
  final MediaItemImage? image;
  @override
  final List<ProviderMapping> providerMappings;
  @override
  final MediaItemMetadata? metadata;
  @override
  final bool favorite;
  @override
  final int? position;
  @override
  final String? dateAdded;
  final int discNumber;
  final int trackNumber;

  const Track({
    required this.itemId,
    required this.provider,
    required this.name,
    required this.duration,
    required this.artists,
    this.version = '',
    this.sortName,
    this.uri,
    this.externalIds = const <List<String>>[],
    this.isPlayable = true,
    this.available,
    this.album,
    this.image,
    this.providerMappings = const <ProviderMapping>[],
    this.metadata,
    this.favorite = false,
    this.position,
    this.dateAdded,
    this.discNumber = 0,
    this.trackNumber = 0,
  });

  @override
  MediaType get mediaType => MediaType.track;

  MediaItemImage? get primaryImage {
    final albumImage = album?.primaryImage;
    if (albumImage != null) {
      return albumImage;
    }

    final MediaItemImage? trackImage = image;
    if (trackImage != null) {
      return trackImage;
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

    final MediaItemImage? trackImage = image;
    if (trackImage != null) {
      return trackImage.getProxyPath();
    }

    final MediaItemImage? metadataImage = primaryImage;
    if (metadataImage == null) {
      return null;
    }

    return metadataImage.getProxyPath(checksum: metadata?.cacheChecksum);
  }

  factory Track.fromJson(Map<String, dynamic> json) {
    final albumJson = json['album'];
    return Track(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unknown Track',
      version: (json['version'] as String?)?.trim() ?? '',
      sortName: (json['sort_name'] as String?)?.trim(),
      uri: (json['uri'] as String?)?.trim(),
      externalIds: _stringPairsFromJson(json['external_ids']),
      isPlayable: json['is_playable'] != false,
      available: json['available'] as bool?,
      duration: (json['duration'] as num?)?.toInt() ?? 0,
      artists:
          (json['artists'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(Artist.fromJson)
              .toList() ??
          const <Artist>[],
      album: albumJson is Map<String, dynamic>
          ? Album.fromJson(albumJson)
          : null,
      image: json['image'] is Map<String, dynamic>
          ? MediaItemImage.fromJson(json['image'] as Map<String, dynamic>)
          : null,
      providerMappings:
          (json['provider_mappings'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(ProviderMapping.fromJson)
              .toList() ??
          const <ProviderMapping>[],
      metadata: _metadataFromJson(json),
      favorite: json['favorite'] == true,
      position: (json['position'] as num?)?.toInt(),
      dateAdded: (json['date_added'] as String?)?.trim(),
      discNumber: (json['disc_number'] as num?)?.toInt() ?? 0,
      trackNumber: (json['track_number'] as num?)?.toInt() ?? 0,
    );
  }
}

class Radio extends MediaItem {
  final int? duration;
  final bool isDynamic;

  Radio._(MediaItem media, {this.duration, this.isDynamic = false})
    : super(
        itemId: media.itemId,
        provider: media.provider,
        name: media.name,
        mediaType: MediaType.radio,
        providerMappings: media.providerMappings,
        version: media.version,
        sortName: media.sortName,
        uri: media.uri,
        externalIds: media.externalIds,
        isPlayable: media.isPlayable,
        available: media.available,
        metadata: media.metadata,
        favorite: media.favorite,
        position: media.position,
        dateAdded: media.dateAdded,
      );

  factory Radio.fromJson(Map<String, dynamic> json) => Radio._(
    MediaItem.fromJson(json),
    duration: (json['duration'] as num?)?.toInt(),
    isDynamic: json['is_dynamic'] == true,
  );
}

class Audiobook extends MediaItem {
  final int duration;
  final int? resumePositionMs;

  Audiobook._(MediaItem media, {required this.duration, this.resumePositionMs})
    : super(
        itemId: media.itemId,
        provider: media.provider,
        name: media.name,
        mediaType: MediaType.audiobook,
        providerMappings: media.providerMappings,
        version: media.version,
        sortName: media.sortName,
        uri: media.uri,
        externalIds: media.externalIds,
        isPlayable: media.isPlayable,
        available: media.available,
        metadata: media.metadata,
        favorite: media.favorite,
        position: media.position,
        dateAdded: media.dateAdded,
      );

  factory Audiobook.fromJson(Map<String, dynamic> json) => Audiobook._(
    MediaItem.fromJson(json),
    duration: (json['duration'] as num?)?.toInt() ?? 0,
    resumePositionMs: (json['resume_position_ms'] as num?)?.toInt(),
  );
}

class Podcast extends MediaItem {
  final String? publisher;
  final int? totalEpisodes;

  Podcast._(MediaItem media, {this.publisher, this.totalEpisodes})
    : super(
        itemId: media.itemId,
        provider: media.provider,
        name: media.name,
        mediaType: MediaType.podcast,
        providerMappings: media.providerMappings,
        version: media.version,
        sortName: media.sortName,
        uri: media.uri,
        externalIds: media.externalIds,
        isPlayable: media.isPlayable,
        available: media.available,
        metadata: media.metadata,
        favorite: media.favorite,
        position: media.position,
        dateAdded: media.dateAdded,
      );

  factory Podcast.fromJson(Map<String, dynamic> json) => Podcast._(
    MediaItem.fromJson(json),
    publisher: (json['publisher'] as String?)?.trim(),
    totalEpisodes: (json['total_episodes'] as num?)?.toInt(),
  );
}

class PodcastEpisode extends MediaItem {
  final Podcast podcast;
  final int duration;
  final int? resumePositionMs;

  PodcastEpisode._(
    MediaItem media, {
    required this.podcast,
    required this.duration,
    this.resumePositionMs,
  }) : super(
         itemId: media.itemId,
         provider: media.provider,
         name: media.name,
         mediaType: MediaType.podcastEpisode,
         providerMappings: media.providerMappings,
         version: media.version,
         sortName: media.sortName,
         uri: media.uri,
         externalIds: media.externalIds,
         isPlayable: media.isPlayable,
         available: media.available,
         metadata: media.metadata,
         favorite: media.favorite,
         position: media.position,
         dateAdded: media.dateAdded,
       );

  factory PodcastEpisode.fromJson(Map<String, dynamic> json) {
    final podcastJson = json['podcast'];
    if (podcastJson is! Map<String, dynamic>) {
      throw const FormatException(
        'Music Assistant podcast episode requires podcast',
      );
    }
    return PodcastEpisode._(
      MediaItem.fromJson(json),
      podcast: Podcast.fromJson(podcastJson),
      duration: (json['duration'] as num?)?.toInt() ?? 0,
      resumePositionMs: (json['resume_position_ms'] as num?)?.toInt(),
    );
  }
}

class GenericMediaItem extends MediaItem {
  final int? duration;

  GenericMediaItem._(MediaItem media, {this.duration})
    : super(
        itemId: media.itemId,
        provider: media.provider,
        name: media.name,
        mediaType: media.mediaType,
        providerMappings: media.providerMappings,
        version: media.version,
        sortName: media.sortName,
        uri: media.uri,
        externalIds: media.externalIds,
        isPlayable: media.isPlayable,
        available: media.available,
        metadata: media.metadata,
        favorite: media.favorite,
        position: media.position,
        dateAdded: media.dateAdded,
      );

  factory GenericMediaItem.fromJson(Map<String, dynamic> json) =>
      GenericMediaItem._(
        MediaItem.fromJson(json),
        duration: (json['duration'] as num?)?.toInt(),
      );
}

class BrowseFolder {
  final String itemId;
  final String provider;
  final String name;
  final String path;
  final MediaItemImage? image;

  const BrowseFolder({
    required this.itemId,
    required this.provider,
    required this.name,
    this.path = '',
    this.image,
  });

  factory BrowseFolder.fromJson(Map<String, dynamic> json) => BrowseFolder(
    itemId: (json['item_id'] as String?)?.trim() ?? '',
    provider: (json['provider'] as String?)?.trim() ?? '',
    name: (json['name'] as String?)?.trim() ?? '',
    path: (json['path'] as String?)?.trim() ?? '',
    image: json['image'] is Map<String, dynamic>
        ? MediaItemImage.fromJson(json['image'] as Map<String, dynamic>)
        : null,
  );
}

class Playlist implements MediaItem {
  @override
  final String itemId;
  @override
  final String provider;
  @override
  final String name;
  @override
  final String version;
  @override
  final String? sortName;
  @override
  final String? uri;
  @override
  final List<List<String>> externalIds;
  @override
  final bool isPlayable;
  @override
  final bool? available;
  final String? owner;
  final bool isEditable;
  final bool isDynamic;
  @override
  final List<ProviderMapping> providerMappings;
  @override
  final MediaItemMetadata? metadata;
  @override
  final bool favorite;
  @override
  final int? position;
  @override
  final String? dateAdded;

  const Playlist({
    required this.itemId,
    required this.provider,
    required this.name,
    required this.isEditable,
    this.version = '',
    this.sortName,
    this.uri,
    this.externalIds = const <List<String>>[],
    this.isPlayable = true,
    this.available,
    this.owner,
    this.isDynamic = false,
    this.providerMappings = const <ProviderMapping>[],
    this.metadata,
    this.favorite = false,
    this.position,
    this.dateAdded,
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
    final metadata = metadataJson is Map<String, dynamic>
        ? MediaItemMetadata.fromJson(metadataJson)
        : null;

    return Playlist(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unknown Playlist',
      version: (json['version'] as String?)?.trim() ?? '',
      sortName: (json['sort_name'] as String?)?.trim(),
      uri: (json['uri'] as String?)?.trim(),
      externalIds: _stringPairsFromJson(json['external_ids']),
      isPlayable: json['is_playable'] != false,
      available: json['available'] as bool?,
      owner: (json['owner'] as String?)?.trim(),
      isEditable: json['is_editable'] == true,
      isDynamic: json['is_dynamic'] == true,
      providerMappings:
          (json['provider_mappings'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(ProviderMapping.fromJson)
              .toList() ??
          const <ProviderMapping>[],
      metadata: metadata,
      favorite: json['favorite'] == true,
      position: (json['position'] as num?)?.toInt(),
      dateAdded: (json['date_added'] as String?)?.trim(),
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

      final MediaItemImage? trackImage = track.image;
      if (trackImage != null) {
        return trackImage.getProxyPath(size: normalizedSize);
      }

      final MediaItemImage? metadataImage = track.primaryImage;
      if (metadataImage == null) {
        return null;
      }

      return metadataImage.getProxyPath(
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
    final MediaItemImage? image = (meta == null || meta.images.isEmpty)
        ? null
        : meta.images.first;
    if (image == null) {
      return null;
    }

    return image.getProxyPath(
      checksum: meta?.cacheChecksum,
      size: normalizedSize,
    );
  }
}

// Music Assistant queue and remote player domain models.

class QueueItem {
  final String queueId;
  final String queueItemId;
  final String name;
  final int? duration;
  final int? sortIndex;
  final bool? available;
  final MediaItem mediaItem;
  final int? originalIndex;

  const QueueItem({
    required this.queueItemId,
    required this.mediaItem,
    this.queueId = '',
    this.name = '',
    this.duration,
    this.sortIndex,
    this.available,
    this.originalIndex,
  });

  factory QueueItem.fromJson(Map<String, dynamic> json) {
    final mediaItemJson = json['media_item'];
    if (mediaItemJson is! Map<String, dynamic>) {
      throw const FormatException(
        'Music Assistant queue item requires media_item',
      );
    }
    return QueueItem(
      queueId: (json['queue_id'] as String?)?.trim() ?? '',
      queueItemId: (json['queue_item_id'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? '',
      duration: (json['duration'] as num?)?.toInt(),
      sortIndex: (json['sort_index'] as num?)?.toInt(),
      available: json['available'] as bool?,
      mediaItem: mediaItemFromJson(mediaItemJson),
      originalIndex: (json['index'] as num?)?.toInt(),
    );
  }
}

extension QueueItemNumbering on List<QueueItem> {
  // Derive labels from this snapshot without changing its playback order.
  List<int> get normalizedItemNumbers {
    final orderedIndexes = List<int>.generate(length, (index) => index);
    orderedIndexes.sort((firstIndex, secondIndex) {
      final comparison = (this[firstIndex].sortIndex ?? firstIndex).compareTo(
        this[secondIndex].sortIndex ?? secondIndex,
      );
      return comparison != 0 ? comparison : firstIndex.compareTo(secondIndex);
    });
    final numbers = List<int>.filled(length, 0);
    for (var rank = 0; rank < orderedIndexes.length; rank++) {
      numbers[orderedIndexes[rank]] = rank + 1;
    }
    return numbers;
  }
}

extension QueueItemAvailability on QueueItem {
  bool get isAvailable => available ?? mediaItem.isAvailable;

  bool get canPlay => available ?? mediaItem.canPlay;
}

PlayState? _playbackStateFromJson(String? value) {
  switch ((value ?? '').trim().toLowerCase()) {
    case 'idle':
      return PlayState.idle;
    case 'playing':
      return PlayState.playing;
    case 'paused':
      return PlayState.paused;
    case 'buffering':
      return PlayState.buffering;
    case 'unknown':
      return PlayState.unknown;
    default:
      return null;
  }
}

class DeviceInfo {
  final String model;
  final String manufacturer;
  final String? softwareVersion;
  final String? modelId;
  final String? manufacturerId;

  const DeviceInfo({
    required this.model,
    required this.manufacturer,
    this.softwareVersion,
    this.modelId,
    this.manufacturerId,
  });

  factory DeviceInfo.fromJson(Map<String, dynamic> json) => DeviceInfo(
    model: (json['model'] as String?)?.trim() ?? '',
    manufacturer: (json['manufacturer'] as String?)?.trim() ?? '',
    softwareVersion: (json['software_version'] as String?)?.trim(),
    modelId: (json['model_id'] as String?)?.trim(),
    manufacturerId: (json['manufacturer_id'] as String?)?.trim(),
  );
}

class PlayerMedia {
  final String uri;
  final MediaType? mediaType;
  final String? title;
  final String? artist;
  final String? album;
  final String? imageUrl;
  final int? duration;
  final String? queueItemId;

  const PlayerMedia({
    required this.uri,
    this.mediaType,
    this.title,
    this.artist,
    this.album,
    this.imageUrl,
    this.duration,
    this.queueItemId,
  });

  factory PlayerMedia.fromJson(Map<String, dynamic> json) => PlayerMedia(
    uri: (json['uri'] as String?)?.trim() ?? '',
    mediaType: json['media_type'] is String
        ? mediaTypeFromJson(json['media_type'] as String)
        : null,
    title: (json['title'] as String?)?.trim(),
    artist: (json['artist'] as String?)?.trim(),
    album: (json['album'] as String?)?.trim(),
    imageUrl: (json['image_url'] as String?)?.trim(),
    duration: (json['duration'] as num?)?.toInt(),
    queueItemId: (json['queue_item_id'] as String?)?.trim(),
  );
}

class MusicAssistantOutputProtocol {
  final String outputProtocolId;
  final String name;
  final String protocolDomain;

  const MusicAssistantOutputProtocol({
    required this.outputProtocolId,
    this.name = '',
    this.protocolDomain = '',
  });

  factory MusicAssistantOutputProtocol.fromJson(Map<String, dynamic> json) =>
      MusicAssistantOutputProtocol(
        outputProtocolId: (json['output_protocol_id'] as String?)?.trim() ?? '',
        name: (json['name'] as String?)?.trim() ?? '',
        protocolDomain: (json['protocol_domain'] as String?)?.trim() ?? '',
      );
}

class MusicAssistantPlayer {
  final String playerId;
  final String provider;
  final String playerType;
  final String name;
  final bool available;
  final DeviceInfo deviceInfo;
  final PlayState playbackState;
  final double? elapsedTime;
  final bool? powered;
  final int? volumeLevel;
  final bool? volumeMuted;
  final String? activeSource;
  final PlayerMedia? currentMedia;
  final List<MusicAssistantOutputProtocol> outputProtocols;

  const MusicAssistantPlayer({
    required this.playerId,
    required this.provider,
    required this.playerType,
    required this.name,
    required this.available,
    required this.deviceInfo,
    this.playbackState = PlayState.idle,
    this.elapsedTime,
    this.volumeLevel,
    this.volumeMuted,
    this.powered,
    this.activeSource,
    this.currentMedia,
    this.outputProtocols = const <MusicAssistantOutputProtocol>[],
  });

  factory MusicAssistantPlayer.fromJson(Map<String, dynamic> json) {
    final deviceInfoJson = json['device_info'];
    return MusicAssistantPlayer(
      playerId: (json['player_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      playerType: (json['type'] as String?)?.trim() ?? '',
      name: (json['name'] as String?)?.trim() ?? 'Unbekannter Player',
      available: json['available'] == true,
      deviceInfo: deviceInfoJson is Map<String, dynamic>
          ? DeviceInfo.fromJson(deviceInfoJson)
          : const DeviceInfo(model: '', manufacturer: ''),
      playbackState:
          _playbackStateFromJson(json['playback_state'] as String?) ??
          PlayState.idle,
      elapsedTime: (json['elapsed_time'] as num?)?.toDouble(),
      volumeLevel: (json['volume_level'] as num?)?.toInt(),
      volumeMuted: json['volume_muted'] as bool?,
      powered: json['powered'] as bool?,
      activeSource: (json['active_source'] as String?)?.trim(),
      currentMedia: json['current_media'] is Map<String, dynamic>
          ? PlayerMedia.fromJson(json['current_media'] as Map<String, dynamic>)
          : null,
      outputProtocols:
          (json['output_protocols'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(MusicAssistantOutputProtocol.fromJson)
              .toList(growable: false) ??
          const <MusicAssistantOutputProtocol>[],
    );
  }
}

// Music Assistant session, server, provider, and discovery models.

class MusicAssistantUser {
  final String userId;
  final String username;
  final String role;
  final bool enabled;
  final String? displayName;
  final String? avatarUrl;
  final List<String> providerFilter;
  final List<String> playerFilter;

  const MusicAssistantUser({
    required this.userId,
    required this.username,
    required this.role,
    this.enabled = false,
    this.displayName,
    this.avatarUrl,
    this.providerFilter = const <String>[],
    this.playerFilter = const <String>[],
  });

  factory MusicAssistantUser.fromJson(Map<String, dynamic> json) {
    return MusicAssistantUser(
      userId: (json['user_id'] as String?)?.trim() ?? '',
      username: (json['username'] as String?)?.trim() ?? '',
      role: (json['role'] as String?)?.trim() ?? '',
      enabled: json['enabled'] == true,
      displayName: (json['display_name'] as String?)?.trim(),
      avatarUrl: (json['avatar_url'] as String?)?.trim(),
      providerFilter:
          (json['provider_filter'] as List?)?.whereType<String>().toList() ??
          const <String>[],
      playerFilter:
          (json['player_filter'] as List?)?.whereType<String>().toList() ??
          const <String>[],
    );
  }
}

class MusicAssistantSession {
  final String serverUrl;
  final String accessToken;
  final String userId;
  final String username;

  const MusicAssistantSession({
    required this.serverUrl,
    required this.accessToken,
    required this.userId,
    required this.username,
  });

  factory MusicAssistantSession.fromJson(Map<String, dynamic> json) =>
      MusicAssistantSession(
        serverUrl: (json['server_url'] as String?)?.trim() ?? '',
        accessToken: (json['access_token'] as String?)?.trim() ?? '',
        userId: (json['user_id'] as String?)?.trim() ?? '',
        username: (json['username'] as String?)?.trim() ?? '',
      );
}

class ServerInfo {
  final String serverId;
  final String serverVersion;
  final int schemaVersion;
  final int minSupportedSchemaVersion;
  final String? name;
  final bool homeAssistantAddon;
  final bool onboardDone;

  const ServerInfo({
    required this.serverId,
    required this.serverVersion,
    required this.schemaVersion,
    required this.minSupportedSchemaVersion,
    this.name,
    this.homeAssistantAddon = false,
    this.onboardDone = false,
  });

  factory ServerInfo.fromJson(Map<String, dynamic> json) {
    return ServerInfo(
      serverId: (json['server_id'] as String?)?.trim() ?? '',
      serverVersion: (json['server_version'] as String?)?.trim() ?? '',
      schemaVersion: (json['schema_version'] as num?)?.toInt() ?? 0,
      minSupportedSchemaVersion:
          (json['min_supported_schema_version'] as num?)?.toInt() ?? 0,
      name: (json['name'] as String?)?.trim(),
      homeAssistantAddon: json['homeassistant_addon'] == true,
      onboardDone: json['onboard_done'] == true,
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
  final List<String> iconImages;
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
    this.iconImages = const <String>[],
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
      iconImages:
          (json['icon_images'] as List?)
              ?.whereType<String>()
              .map((String value) => value.trim().toLowerCase())
              .where((String value) => value.isNotEmpty)
              .toList(growable: false) ??
          const <String>[],
      type: (json['type'] as String?)?.trim(),
      builtin: json['builtin'] as bool?,
      raw: Map<String, dynamic>.from(json),
    );
  }

  String? get preferredIconData => iconSvg?.trim() ?? icon?.trim();

  bool get hasDefaultIcon => iconImages.contains('default');
}

class ProviderManifestRegistry {
  static final Map<String, ProviderManifest> _byDomain =
      <String, ProviderManifest>{};
  static final Map<String, String> _iconDataByDomain = <String, String>{};

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

  static void registerIconData(String providerOrDomain, String iconData) {
    final domain = _providerDomain(providerOrDomain);
    final value = iconData.trim();
    if (domain.isNotEmpty && value.isNotEmpty) {
      _iconDataByDomain[domain] = value;
    }
  }

  static ProviderManifest? getByProvider(String providerOrDomain) {
    final domain = _providerDomain(providerOrDomain);
    if (domain.isEmpty) {
      return null;
    }
    return _byDomain[domain];
  }

  static String? iconSvgForProvider(String providerOrDomain) {
    final domain = _providerDomain(providerOrDomain);
    final raw =
        _iconDataByDomain[domain]?.trim() ??
        getByProvider(providerOrDomain)?.preferredIconData?.trim();
    if (raw == null || raw.isEmpty) {
      return null;
    }

    if (raw.startsWith('http://') ||
        raw.startsWith('https://') ||
        raw.startsWith('data:image/')) {
      return raw;
    }

    final base = MediaItemImage.currentBaseUrl.trim();
    if (base.isNotEmpty) {
      return raw.startsWith('/') ? '$base$raw' : '$base/$raw';
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
  final String itemId;
  final String provider;
  final String title;
  final String? subtitle;
  final List<MediaItem> items;

  const RecommendationFolder({
    required this.itemId,
    required this.provider,
    required this.title,
    this.subtitle,
    required this.items,
  });

  factory RecommendationFolder.fromJson(Map<String, dynamic> json) {
    return RecommendationFolder(
      itemId: (json['item_id'] as String?)?.trim() ?? '',
      provider: (json['provider'] as String?)?.trim() ?? '',
      title:
          (json['title'] as String?)?.trim() ??
          (json['name'] as String?)?.trim() ??
          'Empfehlungen',
      subtitle:
          (json['subtitle'] as String?)?.trim() ??
          (json['description'] as String?)?.trim(),
      items:
          (json['items'] as List?)
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
      artists:
          (json['artists'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(Artist.fromJson)
              .toList() ??
          const <Artist>[],
      albums:
          (json['albums'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(Album.fromJson)
              .toList() ??
          const <Album>[],
      tracks:
          (json['tracks'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(Track.fromJson)
              .toList() ??
          const <Track>[],
      playlists:
          (json['playlists'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(Playlist.fromJson)
              .toList() ??
          const <Playlist>[],
      radio:
          (json['radio'] as List?)
              ?.whereType<Map<String, dynamic>>()
              .map(mediaItemFromJson)
              .toList() ??
          const <MediaItem>[],
    );
  }
}

class BootstrapData {
  final ServerInfo serverInfo;
  final List<MusicAssistantPlayer> players;
  final List<MediaItem> recentlyPlayed;

  const BootstrapData({
    required this.serverInfo,
    required this.players,
    required this.recentlyPlayed,
  });
}

// Immutable Flutter contracts for versioned Rust media-session events and queries.
enum CoreConnectionState { offline, connecting, online, reconnecting }

enum PlaybackMode { local, remote }

// Media-session state contracts.

class MusicAssistantQueue {
  final String queueId;
  final bool active;
  final String displayName;
  final bool available;
  final List<QueueItem> items;
  final int itemCount;
  final int? currentIndex;
  final bool? shuffleEnabled;
  final RepeatMode? repeatMode;
  final bool? autoplayEnabled;
  final double? elapsedTime;
  final PlayState? state;
  final QueueItem? currentItem;
  final QueueItem? nextItem;
  final bool isDynamic;

  const MusicAssistantQueue({
    this.queueId = '',
    this.active = false,
    this.displayName = '',
    this.available = true,
    this.items = const <QueueItem>[],
    this.itemCount = 0,
    this.currentIndex,
    this.shuffleEnabled,
    this.repeatMode,
    this.autoplayEnabled,
    this.elapsedTime,
    this.state,
    this.currentItem,
    this.nextItem,
    this.isDynamic = false,
  });

  factory MusicAssistantQueue.fromJson(Map<String, dynamic> json) {
    final Object? itemsJson = json['items'];
    final List<QueueItem> items = itemsJson is List
        ? itemsJson
              .whereType<Map<String, dynamic>>()
              .map(QueueItem.fromJson)
              .toList(growable: false)
        : const <QueueItem>[];
    final currentItemJson = json['current_item'];
    final nextItemJson = json['next_item'];
    return MusicAssistantQueue(
      queueId: (json['queue_id'] as String?)?.trim() ?? '',
      active: json['active'] == true,
      displayName: (json['display_name'] as String?)?.trim() ?? '',
      available: json['available'] != false,
      items: items,
      itemCount: itemsJson is num ? itemsJson.toInt() : items.length,
      shuffleEnabled: json['shuffle_enabled'] as bool?,
      repeatMode: repeatModeFromJson(json['repeat_mode'] as String?),
      autoplayEnabled: json['autoplay_enabled'] as bool?,
      currentIndex: (json['current_index'] as num?)?.toInt(),
      elapsedTime: (json['elapsed_time'] as num?)?.toDouble(),
      state: _playbackStateFromJson(json['state'] as String?),
      currentItem: currentItemJson is Map<String, dynamic>
          ? QueueItem.fromJson(currentItemJson)
          : null,
      nextItem: nextItemJson is Map<String, dynamic>
          ? QueueItem.fromJson(nextItemJson)
          : null,
      isDynamic: json['is_dynamic'] == true,
    );
  }

  MusicAssistantQueue copyWith({
    String? queueId,
    bool clearQueueId = false,
    List<QueueItem>? items,
    int? currentIndex,
    bool clearCurrentIndex = false,
    bool? shuffleEnabled,
    bool clearShuffleEnabled = false,
    RepeatMode? repeatMode,
    bool clearRepeatMode = false,
    bool? autoplayEnabled,
    bool clearAutoplayEnabled = false,
  }) => MusicAssistantQueue(
    queueId: clearQueueId ? '' : (queueId ?? this.queueId),
    active: active,
    displayName: displayName,
    available: available,
    items: items ?? this.items,
    itemCount: itemCount,
    currentIndex: clearCurrentIndex
        ? null
        : (currentIndex ?? this.currentIndex),
    shuffleEnabled: clearShuffleEnabled
        ? null
        : (shuffleEnabled ?? this.shuffleEnabled),
    repeatMode: clearRepeatMode ? null : (repeatMode ?? this.repeatMode),
    autoplayEnabled: clearAutoplayEnabled
        ? null
        : (autoplayEnabled ?? this.autoplayEnabled),
    elapsedTime: elapsedTime,
    state: state,
    currentItem: currentItem,
    nextItem: nextItem,
    isDynamic: isDynamic,
  );
}

class Queue {
  final String queueId;
  final List<QueueItem> items;
  final int? currentIndex;
  final bool shuffleEnabled;
  final RepeatMode repeatMode;
  final bool autoplayEnabled;

  const Queue({
    this.queueId = '',
    this.items = const <QueueItem>[],
    this.currentIndex,
    this.shuffleEnabled = false,
    this.repeatMode = RepeatMode.off,
    this.autoplayEnabled = false,
  });

  factory Queue.fromJson(Map<String, dynamic> json) {
    final itemsJson = json['items'];
    return Queue(
      queueId: (json['queueId'] as String?)?.trim() ?? '',
      items: itemsJson is List
          ? itemsJson
                .whereType<Map<String, dynamic>>()
                .map(QueueItem.fromJson)
                .toList(growable: false)
          : const <QueueItem>[],
      currentIndex: (json['currentIndex'] as num?)?.toInt(),
      shuffleEnabled: json['shuffleEnabled'] == true,
      repeatMode:
          repeatModeFromJson(
            (json['repeatMode'] as String?)?.trim() ?? 'off',
          ) ??
          RepeatMode.off,
      autoplayEnabled: json['autoplayEnabled'] == true,
    );
  }
}

class PlayerState {
  final PlaybackMode? activeMode;
  final CoreConnectionState connectionState;
  final PlayState playbackState;
  final String? currentQueueItemId;
  final int? currentIndex;
  final int? positionMillis;
  final int? durationMillis;
  final int? bufferedPositionMillis;
  final int volume;
  final bool muted;

  const PlayerState({
    this.activeMode,
    this.connectionState = CoreConnectionState.offline,
    this.playbackState = PlayState.uninitialized,
    this.currentQueueItemId,
    this.currentIndex,
    this.positionMillis,
    this.durationMillis,
    this.bufferedPositionMillis,
    this.volume = 50,
    this.muted = false,
  });

  factory PlayerState.fromJson(Map<String, dynamic> json) {
    return PlayerState(
      activeMode: _parseOutputMode(json['activeMode']),
      connectionState: _parseConnectionState(json['connectionState']),
      playbackState: _parsePlaybackState(json['playbackState']),
      currentQueueItemId: (json['currentQueueItemId'] as String?)?.trim(),
      currentIndex: (json['currentIndex'] as num?)?.toInt(),
      positionMillis: _toClampedMillis(
        (json['positionMillis'] as num?)?.toDouble(),
      ),
      durationMillis: _toClampedMillis(
        (json['durationMillis'] as num?)?.toDouble(),
      ),
      bufferedPositionMillis: _toClampedMillis(
        (json['bufferedPositionMillis'] as num?)?.toDouble(),
      ),
      volume: (json['volume'] as num?)?.toInt().clamp(0, 100) ?? 50,
      muted: json['muted'] == true,
    );
  }
}

class MediaSessionState {
  final Queue queue;
  final PlayerState playerState;

  const MediaSessionState({
    this.queue = const Queue(),
    this.playerState = const PlayerState(),
  });
}

class SessionSettingsUpdate {
  const SessionSettingsUpdate({
    required this.localCacheSizeBytes,
    required this.isDarkTheme,
    required this.logLevel,
  });

  final int localCacheSizeBytes;
  final bool isDarkTheme;
  final LogLevelPreference logLevel;

  Map<String, Object?> toJson() => <String, Object?>{
    'localCacheSizeBytes': localCacheSizeBytes,
    'theme': isDarkTheme ? 'dark' : 'light',
    'logLevel': logLevel.name,
  };
}

class SessionSettings {
  const SessionSettings({
    required this.musicAssistantUrl,
    required this.username,
    required this.audioProfile,
    required this.localCacheSizeBytes,
    required this.isDarkTheme,
    required this.logLevel,
  });

  final String musicAssistantUrl;
  final String? username;
  final String audioProfile;
  final int localCacheSizeBytes;
  final bool isDarkTheme;
  final LogLevelPreference logLevel;

  factory SessionSettings.fromJson(Map<String, dynamic> json) {
    return SessionSettings(
      musicAssistantUrl: json['musicAssistantUrl'] as String? ?? '',
      username: json['username'] as String?,
      audioProfile: json['audioProfile'] as String? ?? '',
      localCacheSizeBytes:
          (json['localCacheSizeBytes'] as num?)?.toInt() ?? 250 * 1024 * 1024,
      isDarkTheme: json['theme'] != 'light',
      logLevel: LogLevelPreference.parse(json['logLevel'] as String? ?? 'info'),
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

PlaybackMode? _parseOutputMode(Object? value) =>
    switch ((value as String?)?.trim()) {
      'local' => PlaybackMode.local,
      'remote' => PlaybackMode.remote,
      _ => null,
    };
CoreConnectionState _parseConnectionState(Object? value) =>
    switch ((value as String?)?.trim().toLowerCase()) {
      'connecting' => CoreConnectionState.connecting,
      'online' => CoreConnectionState.online,
      'reconnecting' => CoreConnectionState.reconnecting,
      _ => CoreConnectionState.offline,
    };
PlayState _parsePlaybackState(Object? value) =>
    switch ((value as String?)?.trim().toLowerCase()) {
      'idle' => PlayState.idle,
      'buffering' => PlayState.buffering,
      'playing' => PlayState.playing,
      'paused' => PlayState.paused,
      'error' => PlayState.error,
      _ => PlayState.uninitialized,
    };
