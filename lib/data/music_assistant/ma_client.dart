import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/foundation.dart' show compute;
import 'package:http/http.dart' as http;

import 'ma_models.dart';

/// Runs off the UI isolate via [compute] so large responses don't jank the UI thread.
dynamic _decodeJsonBody(String body) => jsonDecode(body);

class UnauthorizedException implements Exception {
  final String message;

  UnauthorizedException([this.message = 'Nicht authentifiziert']);

  @override
  String toString() => 'UnauthorizedException: $message';
}

class RequestException implements Exception {
  final String message;

  RequestException(this.message);

  @override
  String toString() => 'RequestException: $message';
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
    MediaItemImage.configureProxyBaseUrl(baseUrl);
  }

  // Authentication

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
      throw RequestException('Ungueltige Login-Antwort');
    }

    final success = response['success'];
    if (success == false) {
      throw UnauthorizedException('Anmeldung fehlgeschlagen');
    }

    final token =
        (response['token'] as String?) ?? (response['access_token'] as String?);
    if (token == null || token.trim().isEmpty) {
      throw RequestException('Login ohne Token');
    }

    return token.trim();
  }

  Future<void> logout() async {
    await _sendCommand('auth/logout');
  }

  Future<AuthUser?> getCurrentUser() async {
    final result = await _sendCommand('auth/me');
    if (result is! Map<String, dynamic>) {
      return null;
    }

    return AuthUser.fromJson(result);
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
    await _sendCommand(
      'auth/token/revoke',
      args: <String, dynamic>{'token_id': tokenId},
    );
  }

  Future<AuthUser?> updateUser({
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
    if (userId != null && userId.trim().isNotEmpty) {
      args['user_id'] = userId.trim();
    }
    if (username != null && username.trim().isNotEmpty) {
      args['username'] = username.trim();
    }
    if (displayName != null && displayName.trim().isNotEmpty) {
      args['display_name'] = displayName.trim();
    }
    if (avatarUrl != null && avatarUrl.trim().isNotEmpty) {
      args['avatar_url'] = avatarUrl.trim();
    }
    if (password != null && password.isNotEmpty) args['password'] = password;
    if (role != null && role.trim().isNotEmpty) args['role'] = role.trim();
    if (preferences != null) args['preferences'] = preferences;
    if (playerFilter != null) args['player_filter'] = playerFilter;
    if (providerFilter != null) args['provider_filter'] = providerFilter;

    final result = await _sendCommand('auth/user/update', args: args);
    if (result is! Map<String, dynamic>) {
      return null;
    }

    return AuthUser.fromJson(result);
  }

  // General

  Future<bool> hasValidSession() async {
    try {
      final result = await _sendCommand('info');
      return result is Map<String, dynamic>;
    } on UnauthorizedException {
      return false;
    } catch (_) {
      return false;
    }
  }

  Future<ServerInfo?> getServerInfoSafe() async {
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

  // Bootstrap

  Future<BootstrapData> loadBootstrapData() async {
    final info = await getServerInfo();
    final players = await getPlayersSafe();
    final recent = await getRecentlyPlayedSafe(limit: 20);

    return BootstrapData(
      serverInfo: info,
      players: players,
      recentlyPlayed: recent,
    );
  }

  // Music - Core

  Future<ServerInfo> getServerInfo() async {
    final result = await _sendCommand('info');
    if (result is! Map<String, dynamic>) {
      throw RequestException('Ungueltige Antwort auf info');
    }

    return ServerInfo.fromJson(result);
  }

  Future<Map<String, dynamic>?> getLibraryItem({
    required MediaType mediaType,
    required String itemId,
    String providerInstanceIdOrDomain = 'library',
  }) async {
    final result = await _sendCommand(
      'music/get_library_item',
      args: <String, dynamic>{
        'media_type': mediaTypeToJson(mediaType),
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return result is Map<String, dynamic> ? result : null;
  }

  Future<Map<String, dynamic>?> getMusicItem({
    required MediaType mediaType,
    required String itemId,
    required String providerInstanceIdOrDomain,
  }) async {
    final result = await _sendCommand(
      'music/item',
      args: <String, dynamic>{
        'media_type': mediaTypeToJson(mediaType),
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return result is Map<String, dynamic> ? result : null;
  }

  Future<Map<String, dynamic>?> getItemByUri(String uri) async {
    final result = await _sendCommand(
      'music/item_by_uri',
      args: <String, dynamic>{'uri': uri},
    );
    return result is Map<String, dynamic> ? result : null;
  }

  // Music - Artists

  Future<List<Artist>> getLibraryArtists({
    bool? favorite,
    String? search,
    int? limit,
    int? offset,
    String? orderBy,
    bool libraryItemsOnly = true,
  }) async {
    final args = <String, dynamic>{'library_items_only': libraryItemsOnly};
    if (favorite != null) args['favorite'] = favorite;
    if (search != null && search.trim().isNotEmpty) {
      args['search'] = search.trim();
    }
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;
    if (orderBy != null && orderBy.trim().isNotEmpty) {
      args['order_by'] = orderBy.trim();
    }

    final result = await _sendCommand(
      'music/artists/library_items',
      args: args,
    );
    return _mapList(result, Artist.fromJson);
  }

  Future<Artist?> getArtist(
    String itemId,
    String providerInstanceIdOrDomain,
  ) async {
    final result = await _sendCommand(
      'music/artists/get',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return result is Map<String, dynamic> ? Artist.fromJson(result) : null;
  }

  Future<List<Album>> getArtistAlbums(
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

    return _mapList(result, Album.fromJson);
  }

  Future<List<Track>> getArtistTracks(
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

    return _mapList(result, Track.fromJson);
  }

  Future<List<Track>> getArtistTopTracks(
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
    return _mapList(result, Track.fromJson);
  }

  Future<List<Album>> getArtistTopAlbums(
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
    return _mapList(result, Album.fromJson);
  }

  Future<List<Artist>> getSimilarArtists(
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

    final result = await _sendCommand(
      'music/artists/similar_artists',
      args: args,
    );
    return _mapList(result, Artist.fromJson);
  }

  Future<int> getArtistsCount({bool favoriteOnly = false}) async {
    final result = await _sendCommand(
      'music/artists/count',
      args: <String, dynamic>{'favorite_only': favoriteOnly},
    );
    return (result as num?)?.toInt() ?? 0;
  }

  // Music - Albums

  Future<List<Album>> getLibraryAlbums({
    bool? favorite,
    String? search,
    int? limit,
    int? offset,
    String? orderBy,
    bool libraryItemsOnly = true,
    List<AlbumType>? albumTypes,
  }) async {
    final args = <String, dynamic>{'library_items_only': libraryItemsOnly};
    if (favorite != null) args['favorite'] = favorite;
    if (search != null && search.trim().isNotEmpty) {
      args['search'] = search.trim();
    }
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;
    if (orderBy != null && orderBy.trim().isNotEmpty) {
      args['order_by'] = orderBy.trim();
    }
    if (albumTypes != null && albumTypes.isNotEmpty) {
      args['album_types'] = albumTypes.map((e) => e.name).toList();
    }

    final result = await _sendCommand('music/albums/library_items', args: args);
    return _mapList(result, Album.fromJson);
  }

  Future<Album?> getAlbum(
    String itemId,
    String providerInstanceIdOrDomain,
  ) async {
    final result = await _sendCommand(
      'music/albums/get',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return result is Map<String, dynamic> ? Album.fromJson(result) : null;
  }

  Future<List<Track>> getAlbumTracks(
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

    return _mapList(result, Track.fromJson);
  }

  Future<List<Album>> getAlbumVersions(
    String itemId,
    String providerInstanceIdOrDomain,
  ) async {
    final result = await _sendCommand(
      'music/albums/album_versions',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return _mapList(result, Album.fromJson);
  }

  Future<int> getAlbumsCount({
    bool favoriteOnly = false,
    List<AlbumType>? albumTypes,
  }) async {
    final args = <String, dynamic>{'favorite_only': favoriteOnly};
    if (albumTypes != null && albumTypes.isNotEmpty) {
      args['album_types'] = albumTypes.map((e) => e.name).toList();
    }
    final result = await _sendCommand('music/albums/count', args: args);
    return (result as num?)?.toInt() ?? 0;
  }

  // Music - Tracks

  Future<List<Track>> getLibraryTracks({
    bool? favorite,
    String? search,
    int? limit,
    int? offset,
    String? orderBy,
    bool libraryItemsOnly = true,
  }) async {
    final args = <String, dynamic>{'library_items_only': libraryItemsOnly};
    if (favorite != null) args['favorite'] = favorite;
    if (search != null && search.trim().isNotEmpty) {
      args['search'] = search.trim();
    }
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;
    if (orderBy != null && orderBy.trim().isNotEmpty) {
      args['order_by'] = orderBy.trim();
    }

    final result = await _sendCommand('music/tracks/library_items', args: args);
    return _mapList(result, Track.fromJson);
  }

  Future<Track?> getTrack(
    String itemId,
    String providerInstanceIdOrDomain,
  ) async {
    final result = await _sendCommand(
      'music/tracks/get',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return result is Map<String, dynamic> ? Track.fromJson(result) : null;
  }

  Future<List<Track>> getTrackVersions(
    String itemId,
    String providerInstanceIdOrDomain,
  ) async {
    final result = await _sendCommand(
      'music/tracks/track_versions',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );
    return _mapList(result, Track.fromJson);
  }

  Future<List<Album>> getTrackAlbums(
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

    return _mapList(result, Album.fromJson);
  }

  Future<int> getTracksCount({bool favoriteOnly = false}) async {
    final result = await _sendCommand(
      'music/tracks/count',
      args: <String, dynamic>{'favorite_only': favoriteOnly},
    );
    return (result as num?)?.toInt() ?? 0;
  }

  Future<String?> getTrackPreview(
    String itemId,
    String providerInstanceIdOrDomain,
  ) async {
    final result = await _sendCommand(
      'music/tracks/preview',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );
    return result is String ? result : null;
  }

  Future<List<Track>> getSimilarTracks(
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
    if (preferredProviderInstances != null &&
        preferredProviderInstances.isNotEmpty) {
      args['preferred_provider_instances'] = preferredProviderInstances;
    }

    final result = await _sendCommand(
      'music/tracks/similar_tracks',
      args: args,
    );
    return _mapList(result, Track.fromJson);
  }

  // Music - Playlists

  Future<List<Playlist>> getLibraryPlaylists({
    bool? favorite,
    String? search,
    int? limit,
    int? offset,
    String? orderBy,
    bool libraryItemsOnly = true,
  }) async {
    final args = <String, dynamic>{'library_items_only': libraryItemsOnly};
    if (favorite != null) args['favorite'] = favorite;
    if (search != null && search.trim().isNotEmpty) {
      args['search'] = search.trim();
    }
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;
    if (orderBy != null && orderBy.trim().isNotEmpty) {
      args['order_by'] = orderBy.trim();
    }

    final result = await _sendCommand(
      'music/playlists/library_items',
      args: args,
    );
    return _mapList(result, Playlist.fromJson);
  }

  Future<Playlist?> getPlaylist(
    String itemId,
    String providerInstanceIdOrDomain,
  ) async {
    final result = await _sendCommand(
      'music/playlists/get',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );

    return result is Map<String, dynamic> ? Playlist.fromJson(result) : null;
  }

  Future<List<Track>> getPlaylistTracks(
    String itemId,
    String providerInstanceIdOrDomain, {
    bool forceRefresh = false,
    bool allowDynamicTracks = false,
  }) async {
    final result = await _sendCommand(
      'music/playlists/playlist_tracks',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
        'force_refresh': forceRefresh,
        'allow_dynamic_tracks': allowDynamicTracks,
      },
    );

    return _mapList(result, Track.fromJson);
  }

  Future<Playlist?> createPlaylist(
    String name, {
    String? providerInstanceOrDomain,
  }) async {
    final args = <String, dynamic>{'name': name};
    if (providerInstanceOrDomain != null &&
        providerInstanceOrDomain.trim().isNotEmpty) {
      args['provider_instance_or_domain'] = providerInstanceOrDomain.trim();
    }
    final result = await _sendCommand(
      'music/playlists/create_playlist',
      args: args,
    );
    return result is Map<String, dynamic> ? Playlist.fromJson(result) : null;
  }

  Future<void> addPlaylistTracks(String playlistId, List<String> uris) async {
    await _sendCommand(
      'music/playlists/add_playlist_tracks',
      args: <String, dynamic>{'db_playlist_id': playlistId, 'uris': uris},
    );
  }

  Future<void> removePlaylistTracks(
    String playlistId,
    List<int> positionsToRemove,
  ) async {
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
      args: <String, dynamic>{'item_id': itemId, 'recursive': recursive},
    );
  }

  Future<Map<String, dynamic>?> updatePlaylist(
    String itemId,
    Map<String, dynamic> update, {
    bool overwrite = false,
  }) async {
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
    final result = await _sendCommand(
      'music/playlists/count',
      args: <String, dynamic>{'favorite_only': favoriteOnly},
    );
    return (result as num?)?.toInt() ?? 0;
  }

  // Music - Radio/Podcast

  Future<List<MediaItem>> getLibraryRadios({
    bool? favorite,
    String? search,
    int? limit,
    int? offset,
    String? orderBy,
    bool libraryItemsOnly = true,
  }) async {
    final args = <String, dynamic>{'library_items_only': libraryItemsOnly};
    if (favorite != null) args['favorite'] = favorite;
    if (search != null && search.trim().isNotEmpty) {
      args['search'] = search.trim();
    }
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;
    if (orderBy != null && orderBy.trim().isNotEmpty) {
      args['order_by'] = orderBy.trim();
    }

    final result = await _sendCommand('music/radios/library_items', args: args);
    return _mapList(result, mediaItemFromJson);
  }

  Future<MediaItem?> getRadio(
    String itemId,
    String providerInstanceIdOrDomain,
  ) async {
    final result = await _sendCommand(
      'music/radios/get',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );
    return result is Map<String, dynamic> ? mediaItemFromJson(result) : null;
  }

  Future<int> getRadiosCount({bool favoriteOnly = false}) async {
    final result = await _sendCommand(
      'music/radios/count',
      args: <String, dynamic>{'favorite_only': favoriteOnly},
    );
    return (result as num?)?.toInt() ?? 0;
  }

  Future<List<MediaItem>> getLibraryPodcasts({
    bool? favorite,
    String? search,
    int? limit,
    int? offset,
    String? orderBy,
    bool libraryItemsOnly = true,
  }) async {
    final args = <String, dynamic>{'library_items_only': libraryItemsOnly};
    if (favorite != null) args['favorite'] = favorite;
    if (search != null && search.trim().isNotEmpty) {
      args['search'] = search.trim();
    }
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;
    if (orderBy != null && orderBy.trim().isNotEmpty) {
      args['order_by'] = orderBy.trim();
    }

    final result = await _sendCommand(
      'music/podcasts/library_items',
      args: args,
    );
    return _mapList(result, mediaItemFromJson);
  }

  Future<MediaItem?> getPodcast(
    String itemId,
    String providerInstanceIdOrDomain,
  ) async {
    final result = await _sendCommand(
      'music/podcasts/get',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );
    return result is Map<String, dynamic> ? mediaItemFromJson(result) : null;
  }

  Future<List<MediaItem>> getPodcastEpisodes(
    String itemId,
    String providerInstanceIdOrDomain,
  ) async {
    final result = await _sendCommand(
      'music/podcasts/podcast_episodes',
      args: <String, dynamic>{
        'item_id': itemId,
        'provider_instance_id_or_domain': providerInstanceIdOrDomain,
      },
    );
    return _mapList(result, mediaItemFromJson);
  }

  Future<int> getPodcastsCount({bool favoriteOnly = false}) async {
    final result = await _sendCommand(
      'music/podcasts/count',
      args: <String, dynamic>{'favorite_only': favoriteOnly},
    );
    return (result as num?)?.toInt() ?? 0;
  }

  // Music - Search & Browse

  Future<SearchResults?> search({
    required String searchQuery,
    List<MediaType>? mediaTypes,
    int? limit,
    bool libraryOnly = false,
  }) async {
    final args = <String, dynamic>{
      'search_query': searchQuery,
      'library_only': libraryOnly,
    };
    if (mediaTypes != null && mediaTypes.isNotEmpty) {
      args['media_types'] = mediaTypes.map(mediaTypeToJson).toList();
    }
    if (limit != null) args['limit'] = limit;

    final result = await _sendCommand('music/search', args: args);
    if (result is! Map<String, dynamic>) {
      return null;
    }
    return SearchResults.fromJson(result);
  }

  Future<List<dynamic>> browse({String? path}) async {
    final args = <String, dynamic>{};
    if (path != null && path.trim().isNotEmpty) {
      args['path'] = path.trim();
    }
    final result = await _sendCommand('music/browse', args: args);
    return result is List ? result : const <dynamic>[];
  }

  Future<List<RecommendationFolder>> getRecommendations() async {
    final result = await _sendCommand('music/recommendations');
    return _mapList(result, RecommendationFolder.fromJson);
  }

  Future<List<Track>> getRecentlyAddedTracks({int limit = 50}) async {
    final result = await _sendCommand(
      'music/recently_added_tracks',
      args: <String, dynamic>{'limit': limit},
    );
    return _mapList(result, Track.fromJson);
  }

  Future<List<MediaItem>> getRecentlyPlayedItems({
    int limit = 50,
    List<MediaType>? mediaTypes,
    String? userId,
    String? queueId,
    bool? fullyPlayedOnly,
    bool? userInitiatedOnly,
  }) async {
    final args = <String, dynamic>{'limit': limit};
    if (mediaTypes != null && mediaTypes.isNotEmpty) {
      args['media_types'] = mediaTypes.map(mediaTypeToJson).toList();
    }
    if (userId != null && userId.trim().isNotEmpty) {
      args['user_id'] = userId.trim();
    }
    if (queueId != null && queueId.trim().isNotEmpty) {
      args['queue_id'] = queueId.trim();
    }
    if (fullyPlayedOnly != null) args['fully_played_only'] = fullyPlayedOnly;
    if (userInitiatedOnly != null) {
      args['user_initiated_only'] = userInitiatedOnly;
    }

    final result = await _sendCommand(
      'music/recently_played_items',
      args: args,
    );
    return _mapList(result, mediaItemFromJson);
  }

  // Music - Library Management

  Future<dynamic> addLibraryItemByUri(
    String uri, {
    bool overwriteExisting = false,
  }) {
    return _sendCommand(
      'music/library/add_item',
      args: <String, dynamic>{
        'item': uri,
        'overwrite_existing': overwriteExisting,
      },
    );
  }

  Future<dynamic> addLibraryItemByObject(
    Map<String, dynamic> item, {
    bool overwriteExisting = false,
  }) {
    return _sendCommand(
      'music/library/add_item',
      args: <String, dynamic>{
        'item': item,
        'overwrite_existing': overwriteExisting,
      },
    );
  }

  Future<void> removeLibraryItem({
    required MediaType mediaType,
    required String libraryItemId,
    bool recursive = false,
  }) async {
    await _sendCommand(
      'music/library/remove_item',
      args: <String, dynamic>{
        'media_type': mediaTypeToJson(mediaType),
        'library_item_id': libraryItemId,
        'recursive': recursive,
      },
    );
  }

  Future<void> addFavorite(String itemUri) async {
    await _sendCommand(
      'music/favorites/add_item',
      args: <String, dynamic>{'item': itemUri},
    );
  }

  Future<void> removeFavorite({
    required MediaType mediaType,
    required String libraryItemId,
  }) async {
    await _sendCommand(
      'music/favorites/remove_item',
      args: <String, dynamic>{
        'media_type': mediaTypeToJson(mediaType),
        'library_item_id': libraryItemId,
      },
    );
  }

  // Player Queues

  Future<List<PlayerQueue>> getPlayerQueues() async {
    final result = await _sendCommand('player_queues/all');
    return _mapList(result, PlayerQueue.fromJson);
  }

  Future<PlayerQueue?> getPlayerQueue(String queueId) async {
    final result = await _sendCommand(
      'player_queues/get',
      args: <String, dynamic>{'queue_id': queueId},
    );
    return result is Map<String, dynamic> ? PlayerQueue.fromJson(result) : null;
  }

  Future<PlayerQueue?> getActiveQueueForPlayer(String playerId) async {
    final result = await _sendCommand(
      'player_queues/get_active_queue',
      args: <String, dynamic>{'player_id': playerId},
    );
    return result is Map<String, dynamic> ? PlayerQueue.fromJson(result) : null;
  }

  Future<List<QueueItem>> getQueueItems(
    String queueId, {
    int? limit,
    int? offset,
  }) async {
    final args = <String, dynamic>{'queue_id': queueId};
    if (limit != null) args['limit'] = limit;
    if (offset != null) args['offset'] = offset;

    final result = await _sendCommand('player_queues/items', args: args);

    // Get Provider Manifests
    final items = _mapList(result, QueueItem.fromJson);
    if (items.isNotEmpty) {
      final Set<String> providers = <String>{};
      for (final QueueItem item in items) {
        final Track? media = item.mediaItem;
        if (media == null) {
          continue;
        }

        final String direct = media.provider.trim();
        if (direct.isNotEmpty) {
          providers.add(direct);
        }

        for (final ProviderMapping mapping in media.providerMappings) {
          final String domain = mapping.providerDomain.trim();
          if (domain.isNotEmpty) {
            providers.add(domain);
          }

          final String instance = mapping.providerInstance.trim();
          if (instance.isNotEmpty) {
            providers.add(instance);
          }
        }
      }

      for (final String provider in providers) {
        if (ProviderManifestRegistry.getByProvider(provider) != null) {
          continue;
        }

        try {
          await getProviderManifest(provider);
        } catch (_) {
          // Ignore unavailable manifests and keep fallback icon behavior.
        }
      }
    }

    return items;
  }

  Future<void> playMedia(
    String queueId,
    List<MediaItem> mediaItems, {
    QueueOption option = QueueOption.replace,
    bool radioMode = false,
    dynamic startItem,
    bool shuffle = false,
    String sortBy = 'original',
  }) async {
    if (mediaItems.isEmpty) {
      throw RequestException('Media items list cannot be empty');
    }

    final List<MediaItem> resolvedMediaItems = await resolvePlayableMediaItems(
      mediaItems,
    );
    if (shuffle) {
      _shuffleMediaItems(resolvedMediaItems);
    }
    final List<Map<String, dynamic>> mediaPayload = resolvedMediaItems
        .where(
          (MediaItem item) =>
              item.itemId.trim().isNotEmpty && item.provider.trim().isNotEmpty,
        )
        .map(createMediaItemPayload)
        .toList(growable: false);

    if (mediaPayload.isEmpty) {
      throw RequestException('No playable tracks could be resolved');
    }

    final dynamic resolvedStartItem = _resolveStartItemPayload(
      startItem,
      resolvedMediaItems,
    );

    final args = <String, dynamic>{
      'queue_id': queueId,
      'media': mediaPayload,
      'option': option.name,
      'radio_mode': radioMode,
      'sort_by': sortBy,
    };
    if (resolvedStartItem != null) {
      args['start_item'] = resolvedStartItem;
    }

    await _sendCommand('player_queues/play_media', args: args);
  }

  void _shuffleMediaItems(List<MediaItem> mediaItems) {
    for (int index = mediaItems.length - 1; index > 0; index--) {
      final int swapIndex = math.Random().nextInt(index + 1);
      final MediaItem item = mediaItems[index];
      mediaItems[index] = mediaItems[swapIndex];
      mediaItems[swapIndex] = item;
    }
  }

  Future<List<MediaItem>> resolvePlayableMediaItems(
    List<MediaItem> mediaItems,
  ) async {
    const int artistTopTracksLimit = 25;
    final List<MediaItem> resolvedTracks = <MediaItem>[];

    for (final MediaItem item in mediaItems) {
      switch (item) {
        case Track track:
          resolvedTracks.add(track);
        case Playlist playlist:
          final String? provider = _resolveProvider(playlist);
          if (provider == null || playlist.itemId.trim().isEmpty) {
            continue;
          }
          resolvedTracks.addAll(
            await getPlaylistTracks(playlist.itemId, provider),
          );
        case Album album:
          final String? provider = _resolveProvider(album);
          if (provider == null || album.itemId.trim().isEmpty) {
            continue;
          }
          resolvedTracks.addAll(await getAlbumTracks(album.itemId, provider));
        case Artist artist:
          final String? provider = _resolveProvider(artist);
          if (provider == null || artist.itemId.trim().isEmpty) {
            continue;
          }
          final List<Track> topTracks = await getArtistTopTracks(
            artist.itemId,
            provider,
          );
          if (topTracks.isNotEmpty) {
            resolvedTracks.addAll(topTracks.take(artistTopTracksLimit));
            continue;
          }
          final List<Track> artistTracks = await getArtistTracks(
            artist.itemId,
            provider,
          );
          resolvedTracks.addAll(artistTracks.take(artistTopTracksLimit));
        default:
          continue;
      }
    }

    return resolvedTracks;
  }

  String? _resolveProvider(MediaItem item) {
    final String provider = item.provider.trim();
    if (provider.isNotEmpty) {
      return provider;
    }

    for (final ProviderMapping mapping in item.providerMappings) {
      final String instance = mapping.providerInstance.trim();
      if (instance.isNotEmpty) {
        return instance;
      }

      final String domain = mapping.providerDomain.trim();
      if (domain.isNotEmpty) {
        return domain;
      }
    }

    return null;
  }

  dynamic _resolveStartItemPayload(
    dynamic startItem,
    List<MediaItem> mediaItems,
  ) {
    if (mediaItems.isEmpty) {
      return null;
    }

    final String? startItemId = switch (startItem) {
      MediaItem item => item.itemId.trim(),
      String value => value.trim(),
      _ => null,
    };
    final MediaItem resolvedStartItem =
        startItemId == null || startItemId.isEmpty
        ? mediaItems.first
        : mediaItems.firstWhere(
            (MediaItem item) =>
                (item.uri ?? '').trim().toLowerCase() ==
                    startItemId.toLowerCase() ||
                item.itemId.trim().toLowerCase() == startItemId.toLowerCase(),
            orElse: () => mediaItems.first,
          );
    return createMediaItemPayload(resolvedStartItem);
  }

  Map<String, dynamic> createMediaItemPayload(MediaItem mediaItem) {
    final payload = <String, dynamic>{
      'item_id': mediaItem.itemId,
      'provider': mediaItem.provider,
      'name': mediaItem.name.trim().isEmpty ? mediaItem.itemId : mediaItem.name,
      'sort_name': mediaItem.sortName,
      'uri': mediaItem.uri,
      'media_type': mediaTypeToJson(mediaItem.mediaType),
      'provider_mappings': mediaItem.providerMappings
          .map(
            (mapping) => <String, dynamic>{
              'item_id': mapping.itemId,
              'provider_domain': mapping.providerDomain,
              'provider_instance': mapping.providerInstance,
              'available': mapping.available,
              'url': mapping.url,
            },
          )
          .toList(growable: false),
      'metadata': _createMediaItemMetadataPayload(mediaItem.metadata),
      'favorite': mediaItem.favorite,
    };

    if (mediaItem is Track) {
      payload['duration'] = mediaItem.duration;
      payload['artists'] = mediaItem.artists
          .map((artist) => _createMediaItemMappingPayload(artist))
          .toList(growable: false);
      payload['album'] = mediaItem.album == null
          ? null
          : _createMediaItemMappingPayload(mediaItem.album!);
    } else if (mediaItem is Album) {
      payload['year'] = mediaItem.year;
      payload['album_type'] = mediaItem.albumType;
      payload['artists'] = mediaItem.artists
          .map((artist) => _createMediaItemMappingPayload(artist))
          .toList(growable: false);
    } else if (mediaItem is Playlist) {
      payload['owner'] = mediaItem.owner;
      payload['is_editable'] = mediaItem.isEditable;
    }

    return payload;
  }

  Map<String, dynamic> _createMediaItemMappingPayload(MediaItem mediaItem) {
    return <String, dynamic>{
      'item_id': mediaItem.itemId,
      'provider': mediaItem.provider,
      'name': mediaItem.name.trim().isEmpty ? mediaItem.itemId : mediaItem.name,
      'sort_name': mediaItem.sortName,
      'uri': mediaItem.uri,
      'available': true,
      'metadata': _createMediaItemMetadataPayload(mediaItem.metadata),
      'media_type': mediaTypeToJson(mediaItem.mediaType),
    };
  }

  Map<String, dynamic>? _createMediaItemMetadataPayload(
    MediaItemMetadata? metadata,
  ) {
    if (metadata == null) {
      return null;
    }

    return <String, dynamic>{
      'description': metadata.description,
      'images': metadata.images
          .map(
            (image) => <String, dynamic>{
              'type': image.type,
              'path': image.path,
              'provider': image.provider,
              'remotely_accessible': image.remotelyAccessible,
            },
          )
          .toList(growable: false),
      'genres': metadata.genres,
      'label': metadata.label,
      'popularity': metadata.popularity,
    };
  }

  Future<void> queuePlay(String queueId) => _sendCommand(
    'player_queues/play',
    args: <String, dynamic>{'queue_id': queueId},
  );

  Future<void> queuePause(String queueId) => _sendCommand(
    'player_queues/pause',
    args: <String, dynamic>{'queue_id': queueId},
  );

  Future<void> queuePlayPause(String queueId) => _sendCommand(
    'player_queues/play_pause',
    args: <String, dynamic>{'queue_id': queueId},
  );

  Future<void> queueStop(String queueId) => _sendCommand(
    'player_queues/stop',
    args: <String, dynamic>{'queue_id': queueId},
  );

  Future<void> queueNext(String queueId) => _sendCommand(
    'player_queues/next',
    args: <String, dynamic>{'queue_id': queueId},
  );

  Future<void> queuePrevious(String queueId) => _sendCommand(
    'player_queues/previous',
    args: <String, dynamic>{'queue_id': queueId},
  );

  Future<void> queueSeek(String queueId, int position) => _sendCommand(
    'player_queues/seek',
    args: <String, dynamic>{'queue_id': queueId, 'position': position},
  );

  Future<void> queueSkip(String queueId, int seconds) => _sendCommand(
    'player_queues/skip',
    args: <String, dynamic>{'queue_id': queueId, 'seconds': seconds},
  );

  Future<void> clearQueue(String queueId, {bool skipStop = false}) =>
      _sendCommand(
        'player_queues/clear',
        args: <String, dynamic>{'queue_id': queueId, 'skip_stop': skipStop},
      );

  Future<void> playQueueIndex(String queueId, int index) => _sendCommand(
    'player_queues/play_index',
    args: <String, dynamic>{'queue_id': queueId, 'index': index},
  );

  Future<void> deleteQueueItemByIndex(String queueId, int itemIndex) =>
      _sendCommand(
        'player_queues/delete_item',
        args: <String, dynamic>{
          'queue_id': queueId,
          'item_id_or_index': itemIndex,
        },
      );

  Future<void> deleteQueueItemById(String queueId, String itemId) =>
      _sendCommand(
        'player_queues/delete_item',
        args: <String, dynamic>{
          'queue_id': queueId,
          'item_id_or_index': itemId,
        },
      );

  Future<void> moveQueueItem(
    String queueId,
    String queueItemId, {
    int posShift = 0,
  }) => _sendCommand(
    'player_queues/move_item',
    args: <String, dynamic>{
      'queue_id': queueId,
      'queue_item_id': queueItemId,
      'pos_shift': posShift,
    },
  );

  Future<void> setShuffle(String queueId, bool shuffleEnabled) => _sendCommand(
    'player_queues/shuffle',
    args: <String, dynamic>{
      'queue_id': queueId,
      'shuffle_enabled': shuffleEnabled,
    },
  );

  Future<void> setRepeat(String queueId, RepeatMode repeatMode) => _sendCommand(
    'player_queues/repeat',
    args: <String, dynamic>{
      'queue_id': queueId,
      'repeat_mode': repeatMode.name,
    },
  );

  Future<void> setDontStopTheMusic(String queueId, bool enabled) =>
      _sendCommand(
        'player_queues/dont_stop_the_music',
        args: <String, dynamic>{
          'queue_id': queueId,
          'dont_stop_the_music_enabled': enabled,
        },
      );

  // Players

  Future<List<Player>> getPlayers({bool returnUnavailable = false}) async {
    final result = await _sendCommand(
      'players/all',
      args: <String, dynamic>{'return_unavailable': returnUnavailable},
    );
    return _mapList(result, Player.fromJson);
  }

  Future<List<Player>> getPlayersSafe() async {
    try {
      return await getPlayers(returnUnavailable: true);
    } on UnauthorizedException {
      return const <Player>[];
    } catch (_) {
      return const <Player>[];
    }
  }

  Future<Player?> getPlayer(
    String playerId, {
    bool raiseUnavailable = false,
  }) async {
    final result = await _sendCommand(
      'players/get',
      args: <String, dynamic>{
        'player_id': playerId,
        'raise_unavailable': raiseUnavailable,
      },
    );
    return result is Map<String, dynamic> ? Player.fromJson(result) : null;
  }

  Future<void> setPlayerPower(String playerId, bool powered) => _sendCommand(
    'players/cmd/power',
    args: <String, dynamic>{'player_id': playerId, 'powered': powered},
  );

  Future<void> setPlayerVolume(String playerId, int volumeLevel) =>
      _sendCommand(
        'players/cmd/volume_set',
        args: <String, dynamic>{
          'player_id': playerId,
          'volume_level': volumeLevel,
        },
      );

  Future<void> setPlayerMute(String playerId, bool muted) => _sendCommand(
    'players/cmd/volume_mute',
    args: <String, dynamic>{'player_id': playerId, 'muted': muted},
  );

  Future<void> playerVolumeUp(String playerId) => _sendCommand(
    'players/cmd/volume_up',
    args: <String, dynamic>{'player_id': playerId},
  );

  Future<void> playerVolumeDown(String playerId) => _sendCommand(
    'players/cmd/volume_down',
    args: <String, dynamic>{'player_id': playerId},
  );

  Future<void> playerPlay(String playerId) => _sendCommand(
    'players/cmd/play',
    args: <String, dynamic>{'player_id': playerId},
  );

  Future<void> playerPause(String playerId) => _sendCommand(
    'players/cmd/pause',
    args: <String, dynamic>{'player_id': playerId},
  );

  Future<void> playerPlayPause(String playerId) => _sendCommand(
    'players/cmd/play_pause',
    args: <String, dynamic>{'player_id': playerId},
  );

  Future<void> playerNext(String playerId) => _sendCommand(
    'players/cmd/next',
    args: <String, dynamic>{'player_id': playerId},
  );

  Future<void> playerPrevious(String playerId) => _sendCommand(
    'players/cmd/previous',
    args: <String, dynamic>{'player_id': playerId},
  );

  Future<void> playerStop(String playerId) => _sendCommand(
    'players/cmd/stop',
    args: <String, dynamic>{'player_id': playerId},
  );

  Future<void> playerSeek(String playerId, int position) => _sendCommand(
    'players/cmd/seek',
    args: <String, dynamic>{'player_id': playerId, 'position': position},
  );

  // Providers

  Future<List<ProviderManifest>> getProviderManifests() async {
    final result = await _sendCommand('providers/manifests');
    final manifests = _mapList(result, ProviderManifest.fromJson);
    ProviderManifestRegistry.registerAll(manifests);
    return manifests;
  }

  Future<ProviderManifest?> getProviderManifest(
    String domainOrInstanceId,
  ) async {
    final result = await _sendCommand(
      'providers/manifests/get',
      args: <String, dynamic>{'instance_id_or_domain': domainOrInstanceId},
    );
    if (result is! Map<String, dynamic>) {
      return null;
    }

    final manifest = ProviderManifest.fromJson(result);
    ProviderManifestRegistry.registerOne(manifest);
    return manifest;
  }

  // Existing helper facade

  Future<List<MediaItem>> getRecentlyPlayedSafe({int limit = 20}) async {
    try {
      return await getRecentlyPlayedItems(limit: limit);
    } on UnauthorizedException {
      return const <MediaItem>[];
    } catch (_) {
      return const <MediaItem>[];
    }
  }

  Future<dynamic> _sendCommand(
    String command, {
    Map<String, dynamic>? args,
  }) async {
    final uri = Uri.parse('${baseUrl.replaceAll(RegExp(r'/+$'), '')}/api');
    final payload = <String, dynamic>{
      'command': command,
      'args': args ?? const <String, dynamic>{},
    };

    final headers = <String, String>{'Content-Type': 'application/json'};

    if (!_isAuthCommand(command) &&
        authToken != null &&
        authToken!.trim().isNotEmpty) {
      headers['Authorization'] = 'Bearer ${authToken!.trim()}';
    }

    final response = await _httpClient.post(
      uri,
      headers: headers,
      body: jsonEncode(payload),
    );

    if (response.statusCode == 401) {
      throw UnauthorizedException(
        'Session abgelaufen oder kein gueltiges Token',
      );
    }

    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw RequestException('HTTP ${response.statusCode} bei $command');
    }

    try {
      // Small responses aren't worth the isolate hop; only offload larger payloads.
      if (response.bodyBytes.length < 32 * 1024) {
        return jsonDecode(response.body);
      }
      return await compute(_decodeJsonBody, response.body);
    } catch (_) {
      throw RequestException('Antwort fuer $command ist kein gueltiges JSON');
    }
  }

  bool _isAuthCommand(String command) {
    return command.toLowerCase() == 'auth/login';
  }

  List<T> _mapList<T>(dynamic result, T Function(Map<String, dynamic>) mapper) {
    if (result is! List) {
      return <T>[];
    }

    return result.whereType<Map<String, dynamic>>().map(mapper).toList();
  }

  void dispose() {
    _httpClient.close();
  }
}
