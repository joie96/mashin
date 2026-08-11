import 'dart:convert';

import 'package:http/http.dart' as http;

import 'ma_models.dart';

class MaUnauthorizedException implements Exception {
  final String message;

  MaUnauthorizedException([this.message = 'Nicht authentifiziert']);

  @override
  String toString() => 'MaUnauthorizedException: $message';
}

class MaRequestException implements Exception {
  final String message;

  MaRequestException(this.message);

  @override
  String toString() => 'MaRequestException: $message';
}

class MusicAssistantClient {
  final String baseUrl;
  final String? authToken;
  final http.Client _httpClient;

  MusicAssistantClient({
    required this.baseUrl,
    this.authToken,
    http.Client? httpClient,
  }) : _httpClient = httpClient ?? http.Client() {
    MaMediaItemImage.configureProxyBaseUrl(baseUrl);
  }

  // #region Authentication

  Future<String> login({
    required String username,
    required String password,
  }) async {
    final response = await _sendCommand(
      'auth/login',
      args: <String, dynamic>{
        'username': username,
        'password': password,
        'provider_id': 'builtin',
      },
    );

    if (response is! Map<String, dynamic>) {
      throw MaRequestException('Ungueltige Login-Antwort');
    }

    final success = response['success'];
    if (success == false) {
      throw MaUnauthorizedException('Anmeldung fehlgeschlagen');
    }

    final token = (response['token'] as String?) ?? (response['access_token'] as String?);
    if (token == null || token.trim().isEmpty) {
      throw MaRequestException('Login ohne Token');
    }

    return token.trim();
  }

  Future<void> logout() async {
    await _sendCommand('auth/logout');
  }

  Future<MaAuthUser?> getCurrentUser() async {
    final result = await _sendCommand('auth/me');
    if (result is! Map<String, dynamic>) {
      return null;
    }

    return MaAuthUser.fromJson(result);
  }

  Future<List<dynamic>> getAuthProviders() async {
    final result = await _sendCommand('auth/providers');
    if (result is! List) {
      return const <dynamic>[];
    }

    return result;
  }

  Future<String?> createToken({required String name, String? userId}) async {
    final args = <String, dynamic>{'name': name};
    if (userId != null && userId.trim().isNotEmpty) {
      args['user_id'] = userId.trim();
    }

    final result = await _sendCommand('auth/token/create', args: args);
    return result is String ? result : null;
  }

  Future<void> revokeToken(String tokenId) async {
    await _sendCommand('auth/token/revoke', args: <String, dynamic>{'token_id': tokenId});
  }

  Future<MaAuthUser?> updateUser({
    String? userId,
    String? username,
    String? displayName,
    String? avatarUrl,
    String? password,
    String? role,
    Map<String, dynamic>? preferences,
    List<String>? playerFilter,
    List<String>? providerFilter,
  }) async {
    final args = <String, dynamic>{};
    if (userId != null && userId.trim().isNotEmpty) args['user_id'] = userId.trim();
    if (username != null && username.trim().isNotEmpty) args['username'] = username.trim();
    if (displayName != null && displayName.trim().isNotEmpty) args['display_name'] = displayName.trim();
    if (avatarUrl != null && avatarUrl.trim().isNotEmpty) args['avatar_url'] = avatarUrl.trim();
    if (password != null && password.isNotEmpty) args['password'] = password;
    if (role != null && role.trim().isNotEmpty) args['role'] = role.trim();
    if (preferences != null) args['preferences'] = preferences;
    if (playerFilter != null) args['player_filter'] = playerFilter;
    if (providerFilter != null) args['provider_filter'] = providerFilter;

    final result = await _sendCommand('auth/user/update', args: args);
    if (result is! Map<String, dynamic>) {
      return null;
    }

    return MaAuthUser.fromJson(result);
  }

  // #endregion

  // #region General

  Future<bool> hasValidSession() async {
    try {
      final result = await _sendCommand('info');
      return result is Map<String, dynamic>;
    } on MaUnauthorizedException {
      return false;
    } catch (_) {
      return false;
    }
  }

  Future<MaServerInfo?> getServerInfoSafe() async {
    try {
      return await getServerInfo();
    } catch (_) {
      return null;
    }
  }

  Future<List<dynamic>> getProviders() async {
    final result = await _sendCommand('providers');
    if (result is! List) {
      return const <dynamic>[];
    }

    return result;
  }

  // #endregion

  // #region Bootstrap

  Future<MaBootstrapData> loadBootstrapData() async {
    final info = await getServerInfo();
    final players = await getPlayersSafe();
    final recent = await getRecentlyPlayedSafe(limit: 20);

    return MaBootstrapData(
      serverInfo: info,
      players: players,
      recentlyPlayed: recent,
    );
  }

  // #endregion

  // #region Music - Core

  Future<MaServerInfo> getServerInfo() async {
    final result = await _sendCommand('info');
    if (result is! Map<String, dynamic>) {
      throw MaRequestException('Ungueltige Antwort auf info');
    }

    return MaServerInfo.fromJson(result);
  }

  Future<Map<String, dynamic>?> getLibraryItem({
    required MaMediaType mediaType,
    required String itemId,
    String providerInstanceIdOrDomain = 'library',
  }) async {
    final result = await _sendCommand(
      'music/get_library_item',
      args: <String, dynamic>{
        'media_type': maMediaTypeToJson(mediaType),
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return result is Map<String, dynamic> ? result : null;
  }

  Future<Map<String, dynamic>?> getMusicItem({
    required MaMediaType mediaType,
    required String itemId,
    required String providerInstanceIdOrDomain,
  }) async {
    final result = await _sendCommand(
      'music/item',
      args: <String, dynamic>{
        'media_type': maMediaTypeToJson(mediaType),
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return result is Map<String, dynamic> ? result : null;
  }

  Future<Map<String, dynamic>?> getItemByUri(String uri) async {
    final result = await _sendCommand('music/item_by_uri', args: <String, dynamic>{'uri': uri});
    return result is Map<String, dynamic> ? result : null;
  }

  // #endregion

  // #region Music - Artists

  Future<List<MaArtist>> getLibraryArtists({
    bool? favorite,
    String? search,
    int? limit,
    int? offset,
    String? orderBy,
    bool libraryItemsOnly = true,
  }) async {
    final args = <String, dynamic>{'library_items_only': libraryItemsOnly};
    if (favorite != null) args['favorite'] = favorite;
    if (search != null && search.trim().isNotEmpty) args['search'] = search.trim();
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;
    if (orderBy != null && orderBy.trim().isNotEmpty) args['order_by'] = orderBy.trim();

    final result = await _sendCommand('music/artists/library_items', args: args);
    return _mapList(result, MaArtist.fromJson);
  }

  Future<MaArtist?> getArtist(String itemId, String providerInstanceIdOrDomain) async {
    final result = await _sendCommand(
      'music/artists/get',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return result is Map<String, dynamic> ? MaArtist.fromJson(result) : null;
  }

  Future<List<MaAlbum>> getArtistAlbums(
    String itemId,
    String providerInstanceIdOrDomain, {
    bool inLibraryOnly = false,
  }) async {
    final result = await _sendCommand(
      'music/artists/artist_albums',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
        'in_library_only': inLibraryOnly,
      },
    );

    return _mapList(result, MaAlbum.fromJson);
  }

  Future<List<MaTrack>> getArtistTracks(
    String itemId,
    String providerInstanceIdOrDomain, {
    bool inLibraryOnly = false,
  }) async {
    final result = await _sendCommand(
      'music/artists/artist_tracks',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
        'in_library_only': inLibraryOnly,
      },
    );

    return _mapList(result, MaTrack.fromJson);
  }

  Future<List<MaTrack>> getArtistTopTracks(
    String itemId,
    String providerInstanceIdOrDomain, {
    String? providerFilter,
  }) async {
    final args = <String, dynamic>{
      'item_id': itemId,
      'provider_instance_id_or_domain': providerInstanceIdOrDomain,
    };
    if (providerFilter != null && providerFilter.trim().isNotEmpty) {
      args['provider_filter'] = providerFilter.trim();
    }

    final result = await _sendCommand('music/artists/top_tracks', args: args);
    return _mapList(result, MaTrack.fromJson);
  }

  Future<List<MaAlbum>> getArtistTopAlbums(
    String itemId,
    String providerInstanceIdOrDomain, {
    String? providerFilter,
  }) async {
    final args = <String, dynamic>{
      'item_id': itemId,
      'provider_instance_id_or_domain': providerInstanceIdOrDomain,
    };
    if (providerFilter != null && providerFilter.trim().isNotEmpty) {
      args['provider_filter'] = providerFilter.trim();
    }

    final result = await _sendCommand('music/artists/top_albums', args: args);
    return _mapList(result, MaAlbum.fromJson);
  }

  Future<List<MaArtist>> getSimilarArtists(
    String itemId,
    String providerInstanceIdOrDomain, {
    String? providerFilter,
    int? limit,
  }) async {
    final args = <String, dynamic>{
      'item_id': itemId,
      'provider_instance_id_or_domain': providerInstanceIdOrDomain,
    };
    if (providerFilter != null && providerFilter.trim().isNotEmpty) {
      args['provider_filter'] = providerFilter.trim();
    }
    if (limit != null) {
      args['limit'] = limit;
    }

    final result = await _sendCommand('music/artists/similar_artists', args: args);
    return _mapList(result, MaArtist.fromJson);
  }

  Future<int> getArtistsCount({bool favoriteOnly = false}) async {
    final result = await _sendCommand('music/artists/count', args: <String, dynamic>{'favorite_only': favoriteOnly});
    return (result as num?)?.toInt() ?? 0;
  }

  // #endregion

  // #region Music - Albums

  Future<List<MaAlbum>> getLibraryAlbums({
    bool? favorite,
    String? search,
    int? limit,
    int? offset,
    String? orderBy,
    bool libraryItemsOnly = true,
    List<MaAlbumType>? albumTypes,
  }) async {
    final args = <String, dynamic>{'library_items_only': libraryItemsOnly};
    if (favorite != null) args['favorite'] = favorite;
    if (search != null && search.trim().isNotEmpty) args['search'] = search.trim();
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;
    if (orderBy != null && orderBy.trim().isNotEmpty) args['order_by'] = orderBy.trim();
    if (albumTypes != null && albumTypes.isNotEmpty) {
      args['album_types'] = albumTypes.map((e) => e.name).toList();
    }

    final result = await _sendCommand('music/albums/library_items', args: args);
    return _mapList(result, MaAlbum.fromJson);
  }

  Future<MaAlbum?> getAlbum(String itemId, String providerInstanceIdOrDomain) async {
    final result = await _sendCommand(
      'music/albums/get',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return result is Map<String, dynamic> ? MaAlbum.fromJson(result) : null;
  }

  Future<List<MaTrack>> getAlbumTracks(
    String itemId,
    String providerInstanceIdOrDomain, {
    bool inLibraryOnly = false,
  }) async {
    final result = await _sendCommand(
      'music/albums/album_tracks',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
        'in_library_only': inLibraryOnly,
      },
    );

    return _mapList(result, MaTrack.fromJson);
  }

  Future<List<MaAlbum>> getAlbumVersions(String itemId, String providerInstanceIdOrDomain) async {
    final result = await _sendCommand(
      'music/albums/album_versions',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return _mapList(result, MaAlbum.fromJson);
  }

  Future<int> getAlbumsCount({bool favoriteOnly = false, List<MaAlbumType>? albumTypes}) async {
    final args = <String, dynamic>{'favorite_only': favoriteOnly};
    if (albumTypes != null && albumTypes.isNotEmpty) {
      args['album_types'] = albumTypes.map((e) => e.name).toList();
    }
    final result = await _sendCommand('music/albums/count', args: args);
    return (result as num?)?.toInt() ?? 0;
  }

  // #endregion

  // #region Music - Tracks

  Future<List<MaTrack>> getLibraryTracks({
    bool? favorite,
    String? search,
    int? limit,
    int? offset,
    String? orderBy,
    bool libraryItemsOnly = true,
  }) async {
    final args = <String, dynamic>{'library_items_only': libraryItemsOnly};
    if (favorite != null) args['favorite'] = favorite;
    if (search != null && search.trim().isNotEmpty) args['search'] = search.trim();
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;
    if (orderBy != null && orderBy.trim().isNotEmpty) args['order_by'] = orderBy.trim();

    final result = await _sendCommand('music/tracks/library_items', args: args);
    return _mapList(result, MaTrack.fromJson);
  }

  Future<MaTrack?> getTrack(String itemId, String providerInstanceIdOrDomain) async {
    final result = await _sendCommand(
      'music/tracks/get',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return result is Map<String, dynamic> ? MaTrack.fromJson(result) : null;
  }

  Future<List<MaTrack>> getTrackVersions(String itemId, String providerInstanceIdOrDomain) async {
    final result = await _sendCommand(
      'music/tracks/track_versions',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );
    return _mapList(result, MaTrack.fromJson);
  }

  Future<List<MaAlbum>> getTrackAlbums(
    String itemId,
    String providerInstanceIdOrDomain, {
    bool inLibraryOnly = false,
  }) async {
    final result = await _sendCommand(
      'music/tracks/track_albums',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
        'in_library_only': inLibraryOnly,
      },
    );

    return _mapList(result, MaAlbum.fromJson);
  }

  Future<int> getTracksCount({bool favoriteOnly = false}) async {
    final result = await _sendCommand('music/tracks/count', args: <String, dynamic>{'favorite_only': favoriteOnly});
    return (result as num?)?.toInt() ?? 0;
  }

  Future<String?> getTrackPreview(String itemId, String providerInstanceIdOrDomain) async {
    final result = await _sendCommand(
      'music/tracks/preview',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );
    return result is String ? result : null;
  }

  Future<List<MaTrack>> getSimilarTracks(
    String itemId,
    String providerInstanceIdOrDomain, {
    int? limit,
    bool? allowLookup,
    List<String>? preferredProviderInstances,
  }) async {
    final args = <String, dynamic>{
      'item_id': itemId,
      'provider_instance_id_or_domain': providerInstanceIdOrDomain,
    };
    if (limit != null) args['limit'] = limit;
    if (allowLookup != null) args['allow_lookup'] = allowLookup;
    if (preferredProviderInstances != null && preferredProviderInstances.isNotEmpty) {
      args['preferred_provider_instances'] = preferredProviderInstances;
    }

    final result = await _sendCommand('music/tracks/similar_tracks', args: args);
    return _mapList(result, MaTrack.fromJson);
  }

  // #endregion

  // #region Music - Playlists

  Future<List<MaPlaylist>> getLibraryPlaylists({
    bool? favorite,
    String? search,
    int? limit,
    int? offset,
    String? orderBy,
    bool libraryItemsOnly = true,
  }) async {
    final args = <String, dynamic>{'library_items_only': libraryItemsOnly};
    if (favorite != null) args['favorite'] = favorite;
    if (search != null && search.trim().isNotEmpty) args['search'] = search.trim();
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;
    if (orderBy != null && orderBy.trim().isNotEmpty) args['order_by'] = orderBy.trim();

    final result = await _sendCommand('music/playlists/library_items', args: args);
    return _mapList(result, MaPlaylist.fromJson);
  }

  Future<MaPlaylist?> getPlaylist(String itemId, String providerInstanceIdOrDomain) async {
    final result = await _sendCommand(
      'music/playlists/get',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return result is Map<String, dynamic> ? MaPlaylist.fromJson(result) : null;
  }

  Future<List<MaTrack>> getPlaylistTracks(
    String itemId,
    String providerInstanceIdOrDomain, {
    bool forceRefresh = false,
  }) async {
    final result = await _sendCommand(
      'music/playlists/playlist_tracks',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
        'force_refresh': forceRefresh,
      },
    );

    return _mapList(result, MaTrack.fromJson);
  }

  Future<MaPlaylist?> createPlaylist(String name, {String? providerInstanceOrDomain}) async {
    final args = <String, dynamic>{'name': name};
    if (providerInstanceOrDomain != null && providerInstanceOrDomain.trim().isNotEmpty) {
      args['provider_instance_or_domain'] = providerInstanceOrDomain.trim();
    }
    final result = await _sendCommand('music/playlists/create_playlist', args: args);
    return result is Map<String, dynamic> ? MaPlaylist.fromJson(result) : null;
  }

  Future<void> addPlaylistTracks(String playlistId, List<String> uris) async {
    await _sendCommand(
      'music/playlists/add_playlist_tracks',
      args: <String, dynamic>{
        'db_playlist_id': playlistId,
        'uris': uris,
      },
    );
  }

  Future<void> removePlaylistTracks(String playlistId, List<int> positionsToRemove) async {
    await _sendCommand(
      'music/playlists/remove_playlist_tracks',
      args: <String, dynamic>{
        'db_playlist_id': playlistId,
        'positions_to_remove': positionsToRemove,
      },
    );
  }

  Future<void> removePlaylist(String itemId, {bool recursive = false}) async {
    await _sendCommand(
      'music/playlists/remove',
      args: <String, dynamic>{
        'item_id': itemId,
        'recursive': recursive,
      },
    );
  }

  Future<Map<String, dynamic>?> updatePlaylist(String itemId, Map<String, dynamic> update, {bool overwrite = false}) async {
    final result = await _sendCommand(
      'music/playlists/update',
      args: <String, dynamic>{
        'item_id': itemId,
        'update': update,
        'overwrite': overwrite,
      },
    );

    return result is Map<String, dynamic> ? result : null;
  }

  Future<int> getPlaylistsCount({bool favoriteOnly = false}) async {
    final result = await _sendCommand('music/playlists/count', args: <String, dynamic>{'favorite_only': favoriteOnly});
    return (result as num?)?.toInt() ?? 0;
  }

  // #endregion

  // #region Music - Radio/Podcast

  Future<List<MaMediaItem>> getLibraryRadios({
    bool? favorite,
    String? search,
    int? limit,
    int? offset,
    String? orderBy,
    bool libraryItemsOnly = true,
  }) async {
    final args = <String, dynamic>{'library_items_only': libraryItemsOnly};
    if (favorite != null) args['favorite'] = favorite;
    if (search != null && search.trim().isNotEmpty) args['search'] = search.trim();
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;
    if (orderBy != null && orderBy.trim().isNotEmpty) args['order_by'] = orderBy.trim();

    final result = await _sendCommand('music/radios/library_items', args: args);
    return _mapList(result, MaMediaItem.fromJson);
  }

  Future<MaMediaItem?> getRadio(String itemId, String providerInstanceIdOrDomain) async {
    final result = await _sendCommand(
      'music/radios/get',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );
    return result is Map<String, dynamic> ? MaMediaItem.fromJson(result) : null;
  }

  Future<int> getRadiosCount({bool favoriteOnly = false}) async {
    final result = await _sendCommand('music/radios/count', args: <String, dynamic>{'favorite_only': favoriteOnly});
    return (result as num?)?.toInt() ?? 0;
  }

  Future<List<MaMediaItem>> getLibraryPodcasts({
    bool? favorite,
    String? search,
    int? limit,
    int? offset,
    String? orderBy,
    bool libraryItemsOnly = true,
  }) async {
    final args = <String, dynamic>{'library_items_only': libraryItemsOnly};
    if (favorite != null) args['favorite'] = favorite;
    if (search != null && search.trim().isNotEmpty) args['search'] = search.trim();
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;
    if (orderBy != null && orderBy.trim().isNotEmpty) args['order_by'] = orderBy.trim();

    final result = await _sendCommand('music/podcasts/library_items', args: args);
    return _mapList(result, MaMediaItem.fromJson);
  }

  Future<MaMediaItem?> getPodcast(String itemId, String providerInstanceIdOrDomain) async {
    final result = await _sendCommand(
      'music/podcasts/get',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );
    return result is Map<String, dynamic> ? MaMediaItem.fromJson(result) : null;
  }

  Future<List<MaMediaItem>> getPodcastEpisodes(String itemId, String providerInstanceIdOrDomain) async {
    final result = await _sendCommand(
      'music/podcasts/podcast_episodes',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );
    return _mapList(result, MaMediaItem.fromJson);
  }

  Future<int> getPodcastsCount({bool favoriteOnly = false}) async {
    final result = await _sendCommand('music/podcasts/count', args: <String, dynamic>{'favorite_only': favoriteOnly});
    return (result as num?)?.toInt() ?? 0;
  }

  // #endregion

  // #region Music - Search & Browse

  Future<MaSearchResults?> search({
    required String searchQuery,
    List<MaMediaType>? mediaTypes,
    int? limit,
    bool libraryOnly = false,
  }) async {
    final args = <String, dynamic>{
      'search_query': searchQuery,
      'library_only': libraryOnly,
    };
    if (mediaTypes != null && mediaTypes.isNotEmpty) {
      args['media_types'] = mediaTypes.map(maMediaTypeToJson).toList();
    }
    if (limit != null) args['limit'] = limit;

    final result = await _sendCommand('music/search', args: args);
    if (result is! Map<String, dynamic>) {
      return null;
    }
    return MaSearchResults.fromJson(result);
  }

  Future<List<dynamic>> browse({String? path}) async {
    final args = <String, dynamic>{};
    if (path != null && path.trim().isNotEmpty) {
      args['path'] = path.trim();
    }
    final result = await _sendCommand('music/browse', args: args);
    return result is List ? result : const <dynamic>[];
  }

  Future<List<MaRecommendationFolder>> getRecommendations() async {
    final result = await _sendCommand('music/recommendations');
    return _mapList(result, MaRecommendationFolder.fromJson);
  }

  Future<List<MaTrack>> getRecentlyAddedTracks({int limit = 50}) async {
    final result = await _sendCommand('music/recently_added_tracks', args: <String, dynamic>{'limit': limit});
    return _mapList(result, MaTrack.fromJson);
  }

  Future<List<MaMediaItem>> getRecentlyPlayedItems({
    int limit = 50,
    List<MaMediaType>? mediaTypes,
    String? userId,
    String? queueId,
    bool? fullyPlayedOnly,
    bool? userInitiatedOnly,
  }) async {
    final args = <String, dynamic>{'limit': limit};
    if (mediaTypes != null && mediaTypes.isNotEmpty) {
      args['media_types'] = mediaTypes.map(maMediaTypeToJson).toList();
    }
    if (userId != null && userId.trim().isNotEmpty) args['user_id'] = userId.trim();
    if (queueId != null && queueId.trim().isNotEmpty) args['queue_id'] = queueId.trim();
    if (fullyPlayedOnly != null) args['fully_played_only'] = fullyPlayedOnly;
    if (userInitiatedOnly != null) args['user_initiated_only'] = userInitiatedOnly;

    final result = await _sendCommand('music/recently_played_items', args: args);
    return _mapList(result, MaMediaItem.fromJson);
  }

  // #endregion

  // #region Music - Library Management

  Future<dynamic> addLibraryItemByUri(String uri, {bool overwriteExisting = false}) {
    return _sendCommand(
      'music/library/add_item',
      args: <String, dynamic>{
        'item': uri,
        'overwrite_existing': overwriteExisting,
      },
    );
  }

  Future<dynamic> addLibraryItemByObject(Map<String, dynamic> item, {bool overwriteExisting = false}) {
    return _sendCommand(
      'music/library/add_item',
      args: <String, dynamic>{
        'item': item,
        'overwrite_existing': overwriteExisting,
      },
    );
  }

  Future<void> removeLibraryItem({
    required MaMediaType mediaType,
    required String libraryItemId,
    bool recursive = false,
  }) async {
    await _sendCommand(
      'music/library/remove_item',
      args: <String, dynamic>{
        'media_type': maMediaTypeToJson(mediaType),
        'library_item_id': libraryItemId,
        'recursive': recursive,
      },
    );
  }

  Future<void> addFavorite(String itemUri) async {
    await _sendCommand('music/favorites/add_item', args: <String, dynamic>{'item': itemUri});
  }

  Future<void> removeFavorite({required MaMediaType mediaType, required String libraryItemId}) async {
    await _sendCommand(
      'music/favorites/remove_item',
      args: <String, dynamic>{
        'media_type': maMediaTypeToJson(mediaType),
        'library_item_id': libraryItemId,
      },
    );
  }

  // #endregion

  // #region Player Queues

  Future<List<MaPlayerQueue>> getPlayerQueues() async {
    final result = await _sendCommand('player_queues/all');
    return _mapList(result, MaPlayerQueue.fromJson);
  }

  Future<MaPlayerQueue?> getPlayerQueue(String queueId) async {
    final result = await _sendCommand('player_queues/get', args: <String, dynamic>{'queue_id': queueId});
    return result is Map<String, dynamic> ? MaPlayerQueue.fromJson(result) : null;
  }

  Future<MaPlayerQueue?> getActiveQueueForPlayer(String playerId) async {
    final result = await _sendCommand('player_queues/get_active_queue', args: <String, dynamic>{'player_id': playerId});
    return result is Map<String, dynamic> ? MaPlayerQueue.fromJson(result) : null;
  }

  Future<List<MaQueueItem>> getQueueItems(
    String queueId, {
    int? limit,
    int? offset,
  }) async {
    final args = <String, dynamic>{'queue_id': queueId};
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;

    final result = await _sendCommand('player_queues/items', args: args);
    return _mapList(result, MaQueueItem.fromJson);
  }

  Future<void> playMedia(
    String queueId,
    List<Map<String, dynamic>> mediaItems, {
    MaQueueOption option = MaQueueOption.replace,
    bool radioMode = false,
    dynamic startItem,
    String sortBy = 'original',
  }) async {
    final args = <String, dynamic>{
      'queue_id': queueId,
      'media': mediaItems,
      'option': option.name,
      'radio_mode': radioMode,
      'sort_by': sortBy,
    };
    if (startItem != null) {
      args['start_item'] = startItem;
    }

    await _sendCommand('player_queues/play_media', args: args);
  }

  Future<void> queuePlay(String queueId) => _sendCommand('player_queues/play', args: <String, dynamic>{'queue_id': queueId});

  Future<void> queuePause(String queueId) => _sendCommand('player_queues/pause', args: <String, dynamic>{'queue_id': queueId});

  Future<void> queuePlayPause(String queueId) => _sendCommand('player_queues/play_pause', args: <String, dynamic>{'queue_id': queueId});

  Future<void> queueStop(String queueId) => _sendCommand('player_queues/stop', args: <String, dynamic>{'queue_id': queueId});

  Future<void> queueNext(String queueId) => _sendCommand('player_queues/next', args: <String, dynamic>{'queue_id': queueId});

  Future<void> queuePrevious(String queueId) => _sendCommand('player_queues/previous', args: <String, dynamic>{'queue_id': queueId});

  Future<void> queueSeek(String queueId, int position) =>
      _sendCommand('player_queues/seek', args: <String, dynamic>{'queue_id': queueId, 'position': position});

  Future<void> queueSkip(String queueId, int seconds) =>
      _sendCommand('player_queues/skip', args: <String, dynamic>{'queue_id': queueId, 'seconds': seconds});

  Future<void> clearQueue(String queueId, {bool skipStop = false}) => _sendCommand(
        'player_queues/clear',
        args: <String, dynamic>{'queue_id': queueId, 'skip_stop': skipStop},
      );

  Future<void> playQueueIndex(String queueId, int index) =>
      _sendCommand('player_queues/play_index', args: <String, dynamic>{'queue_id': queueId, 'index': index});

  Future<void> deleteQueueItemByIndex(String queueId, int itemIndex) => _sendCommand(
        'player_queues/delete_item',
        args: <String, dynamic>{'queue_id': queueId, 'item_id_or_index': itemIndex},
      );

  Future<void> deleteQueueItemById(String queueId, String itemId) => _sendCommand(
        'player_queues/delete_item',
        args: <String, dynamic>{'queue_id': queueId, 'item_id_or_index': itemId},
      );

  Future<void> moveQueueItem(String queueId, String queueItemId, {int posShift = 0}) => _sendCommand(
        'player_queues/move_item',
        args: <String, dynamic>{
          'queue_id': queueId,
          'queue_item_id': queueItemId,
          'pos_shift': posShift,
        },
      );

  Future<void> setShuffle(String queueId, bool shuffleEnabled) => _sendCommand(
        'player_queues/shuffle',
        args: <String, dynamic>{'queue_id': queueId, 'shuffle_enabled': shuffleEnabled},
      );

  Future<void> setRepeat(String queueId, MaRepeatMode repeatMode) => _sendCommand(
        'player_queues/repeat',
        args: <String, dynamic>{'queue_id': queueId, 'repeat_mode': repeatMode.name},
      );

  Future<void> setDontStopTheMusic(String queueId, bool enabled) => _sendCommand(
        'player_queues/dont_stop_the_music',
        args: <String, dynamic>{'queue_id': queueId, 'dont_stop_the_music_enabled': enabled},
      );

  // #endregion

  // #region Players

  Future<List<MaPlayer>> getPlayers({bool returnUnavailable = false}) async {
    final result = await _sendCommand('players/all', args: <String, dynamic>{'return_unavailable': returnUnavailable});
    return _mapList(result, MaPlayer.fromJson);
  }

  Future<List<MaPlayer>> getPlayersSafe() async {
    try {
      return await getPlayers(returnUnavailable: true);
    } on MaUnauthorizedException {
      return const <MaPlayer>[];
    } catch (_) {
      return const <MaPlayer>[];
    }
  }

  Future<MaPlayer?> getPlayer(String playerId, {bool raiseUnavailable = false}) async {
    final result = await _sendCommand(
      'players/get',
      args: <String, dynamic>{'player_id': playerId, 'raise_unavailable': raiseUnavailable},
    );
    return result is Map<String, dynamic> ? MaPlayer.fromJson(result) : null;
  }

  Future<void> setPlayerPower(String playerId, bool powered) =>
      _sendCommand('players/cmd/power', args: <String, dynamic>{'player_id': playerId, 'powered': powered});

  Future<void> setPlayerVolume(String playerId, int volumeLevel) => _sendCommand(
        'players/cmd/volume_set',
        args: <String, dynamic>{'player_id': playerId, 'volume_level': volumeLevel},
      );

  Future<void> setPlayerMute(String playerId, bool muted) =>
      _sendCommand('players/cmd/volume_mute', args: <String, dynamic>{'player_id': playerId, 'muted': muted});

  Future<void> playerVolumeUp(String playerId) =>
      _sendCommand('players/cmd/volume_up', args: <String, dynamic>{'player_id': playerId});

  Future<void> playerVolumeDown(String playerId) =>
      _sendCommand('players/cmd/volume_down', args: <String, dynamic>{'player_id': playerId});

  Future<void> playerPlay(String playerId) =>
      _sendCommand('players/cmd/play', args: <String, dynamic>{'player_id': playerId});

  Future<void> playerPause(String playerId) =>
      _sendCommand('players/cmd/pause', args: <String, dynamic>{'player_id': playerId});

  Future<void> playerPlayPause(String playerId) =>
      _sendCommand('players/cmd/play_pause', args: <String, dynamic>{'player_id': playerId});

  Future<void> playerNext(String playerId) =>
      _sendCommand('players/cmd/next', args: <String, dynamic>{'player_id': playerId});

  Future<void> playerPrevious(String playerId) =>
      _sendCommand('players/cmd/previous', args: <String, dynamic>{'player_id': playerId});

  Future<void> playerStop(String playerId) =>
      _sendCommand('players/cmd/stop', args: <String, dynamic>{'player_id': playerId});

  Future<void> playerSeek(String playerId, int position) =>
      _sendCommand('players/cmd/seek', args: <String, dynamic>{'player_id': playerId, 'position': position});

  // #endregion

  // #region Providers

  Future<List<MaProviderManifest>> getProviderManifests() async {
    final result = await _sendCommand('providers/manifests');
    final manifests = _mapList(result, MaProviderManifest.fromJson);
    MaProviderManifestRegistry.registerAll(manifests);
    return manifests;
  }

  Future<MaProviderManifest?> getProviderManifest(String domainOrInstanceId) async {
    final result = await _sendCommand(
      'providers/manifests/get',
      args: <String, dynamic>{'instance_id_or_domain': domainOrInstanceId},
    );
    if (result is! Map<String, dynamic>) {
      return null;
    }

    final manifest = MaProviderManifest.fromJson(result);
    MaProviderManifestRegistry.registerOne(manifest);
    return manifest;
  }

  // #endregion

  // #region Existing helper facade

  Future<List<MaMediaItem>> getRecentlyPlayedSafe({int limit = 20}) async {
    try {
      return await getRecentlyPlayedItems(limit: limit);
    } on MaUnauthorizedException {
      return const <MaMediaItem>[];
    } catch (_) {
      return const <MaMediaItem>[];
    }
  }

  // #endregion

  Future<dynamic> _sendCommand(
    String command, {
    Map<String, dynamic>? args,
  }) async {
    final uri = Uri.parse('${baseUrl.replaceAll(RegExp(r'/+$'), '')}/api');
    final payload = <String, dynamic>{
      'command': command,
      'args': args ?? const <String, dynamic>{},
    };

    final headers = <String, String>{
      'Content-Type': 'application/json',
    };

    if (!_isAuthCommand(command) && authToken != null && authToken!.trim().isNotEmpty) {
      headers['Authorization'] = 'Bearer ${authToken!.trim()}';
    }

    final response = await _httpClient.post(
      uri,
      headers: headers,
      body: jsonEncode(payload),
    );

    if (response.statusCode == 401) {
      throw MaUnauthorizedException('Session abgelaufen oder kein gueltiges Token');
    }

    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw MaRequestException('HTTP ${response.statusCode} bei $command');
    }

    try {
      return jsonDecode(response.body);
    } catch (_) {
      throw MaRequestException('Antwort fuer $command ist kein gueltiges JSON');
    }
  }

  bool _isAuthCommand(String command) {
    return command.toLowerCase() == 'auth/login';
  }

  List<T> _mapList<T>(dynamic result, T Function(Map<String, dynamic>) mapper) {
    if (result is! List) {
      return <T>[];
    }

    return result
        .whereType<Map<String, dynamic>>()
        .map(mapper)
        .toList();
  }

  void dispose() {
    _httpClient.close();
  }
}
