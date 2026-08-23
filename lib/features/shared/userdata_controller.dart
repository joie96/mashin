import 'dart:async';

import 'package:flutter/foundation.dart';

import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';

/// Manages app-specific user data in Music Assistant `auth/me.preferences`.
/// Favorites and local playlists are stored under two root keys in `preferences`.
class UserDataController extends ChangeNotifier {
  static const String favoritesRootKey = 'mashin.favorites';
  static const String playlistsRootKey = 'mashin.playlists';
  static const String localPlaylistProvider = 'mashin';

  UserDataController({
    required this.client,
    required this.configuredUsername,
  });

  final MusicAssistantClient client;
  final String? configuredUsername;

  bool _isDisposed = false;
  bool _isLoadingPreferences = false;

  final Object _pushQueueGate = Object();
  bool _isPushInProgress = false;
  Map<String, dynamic>? _pendingPushPreferences;

  Future<void> _stateLock = Future<void>.value();

  Map<String, dynamic> _preferences = <String, dynamic>{};

  List<Track> _favoriteTracks = const <Track>[];
  List<Album> _favoriteAlbums = const <Album>[];
  List<Playlist> _favoritePlaylists = const <Playlist>[];
  List<Artist> _favoriteArtists = const <Artist>[];

  List<Playlist> _playlists = const <Playlist>[];
  Map<String, List<Track>> _playlistTracksByPlaylistId =
      <String, List<Track>>{};

  @override
  void notifyListeners() {
    if (!_isDisposed) {
      super.notifyListeners();
    }
  }

  @override
  void dispose() {
    _isDisposed = true;
    super.dispose();
  }

  bool get isLoadingPreferences => _isLoadingPreferences;

  List<Track> get favoriteTracks => _favoriteTracks;
  List<Album> get favoriteAlbums => _favoriteAlbums;
  List<Playlist> get favoritePlaylists => _favoritePlaylists;
  List<Artist> get favoriteArtists => _favoriteArtists;

  List<Playlist> get playlists => _playlists;

  List<Track> tracksForPlaylist(String playlistId) {
    final List<Track>? tracks = _playlistTracksByPlaylistId[playlistId.trim()];
    return tracks ?? const <Track>[];
  }

  Map<String, dynamic> get preferences => _cloneMap(_preferences);

  /// Loads the current user preferences from Music Assistant `auth/me.preferences`
  Future<Map<String, dynamic>> loadPreferences() async {
    _isLoadingPreferences = true;
    notifyListeners();

    try {
      final AuthUser? user = await client.getCurrentUser();
      await _withStateLock(() async {
        if (user == null) {
          _preferences = <String, dynamic>{};
        } else {
          _preferences = _normalizeMap(user.preferences);
        }

        _loadFavoritesSnapshot();
        _loadPlaylistsSnapshot();
      });

      return _cloneMap(_preferences);
    } catch (_) {
      await _withStateLock(() async {
        _preferences = <String, dynamic>{};
        _loadFavoritesSnapshot();
        _loadPlaylistsSnapshot();
      });
      return _cloneMap(_preferences);
    } finally {
      _isLoadingPreferences = false;
      notifyListeners();
    }
  }

  /// Persists the current snapshot using `auth/user/update`.
  /// Parallel save calls are coalesced: while a push is active, only the latest pending snapshot is retained.
  Future<bool> pushPreferences() async {
    final String username = (configuredUsername ?? '').trim();
    if (username.isEmpty) {
      return false;
    }

    final Map<String, dynamic> initialSnapshot = await _withStateLock(() async {
      _convertRuntimeStateToSnapshot();
      return _cloneMap(_preferences);
    });

    bool shouldStartWorker = false;
    synchronized(_pushQueueGate, () {
      if (_isPushInProgress) {
        _pendingPushPreferences = initialSnapshot;
      } else {
        _isPushInProgress = true;
        shouldStartWorker = true;
      }
    });

    if (!shouldStartWorker) {
      return true;
    }

    bool lastPushResult = true;
    Map<String, dynamic> currentPreferencesToPush = initialSnapshot;

    try {
      while (true) {
        try {
          final AuthUser? updated = await client.updateUser(
            username: username,
            preferences: currentPreferencesToPush,
          );
          lastPushResult = updated != null;
        } catch (_) {
          lastPushResult = false;
        }

        Map<String, dynamic>? nextSnapshot;
        synchronized(_pushQueueGate, () {
          nextSnapshot = _pendingPushPreferences;
          _pendingPushPreferences = null;
          if (nextSnapshot == null) {
            _isPushInProgress = false;
          }
        });

        if (nextSnapshot == null) {
          return lastPushResult;
        }

        currentPreferencesToPush = nextSnapshot!;
      }
    } finally {
      synchronized(_pushQueueGate, () {
        _isPushInProgress = false;
      });
    }
  }

  void queuePreferencesPush() {
    unawaited(_pushPreferencesInBackground());
  }

  Future<void> _pushPreferencesInBackground() async {
    try {
      await pushPreferences();
    } catch (_) {
      // Background push errors must not interrupt UI flows.
    }
  }

  bool isFavorite(MediaItem item) {
    final String uri = (item.uri ?? '').trim();
    if (uri.isEmpty) {
      return false;
    }

    switch (item.mediaType) {
      case MediaType.track:
        return _favoriteTracks.any((Track t) => _equalsUri(t.uri, uri));
      case MediaType.album:
        return _favoriteAlbums.any((Album a) => _equalsUri(a.uri, uri));
      case MediaType.artist:
        return _favoriteArtists.any((Artist a) => _equalsUri(a.uri, uri));
      case MediaType.playlist:
        return _favoritePlaylists.any((Playlist p) => _equalsUri(p.uri, uri));
      default:
        return false;
    }
  }

  Future<void> setFavorite(
    Iterable<MediaItem> mediaItems,
    bool favorite,
  ) async {
    await _withStateLock(() async {
      bool changed = false;

      final List<Track> tracks = List<Track>.of(_favoriteTracks);
      final List<Album> albums = List<Album>.of(_favoriteAlbums);
      final List<Playlist> playlists = List<Playlist>.of(_favoritePlaylists);
      final List<Artist> artists = List<Artist>.of(_favoriteArtists);

      for (final MediaItem item in mediaItems) {
        final String uri = (item.uri ?? '').trim();
        if (uri.isEmpty) {
          continue;
        }

        switch (item.mediaType) {
          case MediaType.track:
            changed =
                _updateFavoriteList<Track>(tracks, uri, favorite, () => _asTrack(item, true)) ||
                changed;
            break;
          case MediaType.album:
            changed =
                _updateFavoriteList<Album>(albums, uri, favorite, () => _asAlbum(item, true)) ||
                changed;
            break;
          case MediaType.artist:
            changed =
                _updateFavoriteList<Artist>(artists, uri, favorite, () => _asArtist(item, true)) ||
                changed;
            break;
          case MediaType.playlist:
            changed = _updateFavoriteList<Playlist>(
                  playlists,
                  uri,
                  favorite,
                  () => _asPlaylist(item, true),
                ) ||
                changed;
            break;
          default:
            break;
        }
      }

      if (!changed) {
        return;
      }

      _favoriteTracks = List<Track>.unmodifiable(tracks);
      _favoriteAlbums = List<Album>.unmodifiable(albums);
      _favoritePlaylists = List<Playlist>.unmodifiable(playlists);
      _favoriteArtists = List<Artist>.unmodifiable(artists);

      notifyListeners();
    });

    queuePreferencesPush();
  }

  Future<Playlist> addPlaylist(Playlist playlist) async {
    final Playlist stored = await _withStateLock(() async {
      final String normalizedItemId =
          playlist.itemId.trim().isEmpty ? _generateNextLocalPlaylistId() : playlist.itemId.trim();

      final Playlist localPlaylist = Playlist(
        itemId: normalizedItemId,
        provider: playlist.provider.trim().isEmpty
            ? localPlaylistProvider
            : playlist.provider,
        name: playlist.name,
        isEditable: playlist.isEditable,
        sortName: playlist.sortName,
        uri: playlist.uri,
        owner: playlist.owner,
        providerMappings: playlist.providerMappings,
        metadata: playlist.metadata,
        favorite: playlist.favorite,
      );

      final List<Playlist> next = List<Playlist>.of(_playlists);
      next.add(localPlaylist);
      next.sort(_comparePlaylists);

      _playlists = List<Playlist>.unmodifiable(next);
      _playlistTracksByPlaylistId = <String, List<Track>>{
        ..._playlistTracksByPlaylistId,
        normalizedItemId: _playlistTracksByPlaylistId[normalizedItemId] ??
            const <Track>[],
      };

      notifyListeners();
      return localPlaylist;
    });

    queuePreferencesPush();
    return stored;
  }

  Future<bool> updatePlaylist(Playlist playlist) async {
    if (playlist.itemId.trim().isEmpty) {
      return false;
    }

    final bool updated = await _withStateLock(() async {
      final int index = _playlists.indexWhere(
        (Playlist p) =>
            p.itemId.toLowerCase() == playlist.itemId.trim().toLowerCase(),
      );
      if (index < 0) {
        return false;
      }

      final Playlist replacement = Playlist(
        itemId: playlist.itemId,
        provider: playlist.provider.trim().isEmpty
            ? localPlaylistProvider
            : playlist.provider,
        name: playlist.name,
        isEditable: playlist.isEditable,
        sortName: playlist.sortName,
        uri: playlist.uri,
        owner: playlist.owner,
        providerMappings: playlist.providerMappings,
        metadata: playlist.metadata,
        favorite: playlist.favorite,
      );

      final List<Playlist> next = List<Playlist>.of(_playlists);
      next[index] = replacement;
      next.sort(_comparePlaylists);
      _playlists = List<Playlist>.unmodifiable(next);

      notifyListeners();
      return true;
    });

    if (updated) {
      queuePreferencesPush();
    }

    return updated;
  }

  Future<bool> removePlaylist(Playlist playlist) async {
    final String id = playlist.itemId.trim();
    if (id.isEmpty) {
      return false;
    }

    final bool removed = await _withStateLock(() async {
      final List<Playlist> next = _playlists
          .where((Playlist existing) => existing.itemId.toLowerCase() != id.toLowerCase())
          .toList(growable: true);

      if (next.length == _playlists.length) {
        return false;
      }

      final Map<String, List<Track>> nextTracks =
          <String, List<Track>>{..._playlistTracksByPlaylistId};
      nextTracks.removeWhere((String key, List<Track> _) => key.toLowerCase() == id.toLowerCase());

      _playlists = List<Playlist>.unmodifiable(next);
      _playlistTracksByPlaylistId = nextTracks;

      notifyListeners();
      return true;
    });

    if (removed) {
      queuePreferencesPush();
    }

    return removed;
  }

  Future<bool> addPlaylistTracks(
    String playlistId,
    Iterable<Track> tracks,
  ) async {
    final String id = playlistId.trim();
    if (id.isEmpty) {
      return false;
    }

    final List<Track> tracksToAdd = tracks
        .where((Track t) => (t.uri ?? '').trim().isNotEmpty)
        .map((Track t) => _asTrack(t, t.favorite))
        .toList(growable: false);

    if (tracksToAdd.isEmpty) {
      return false;
    }

    final bool changed = await _withStateLock(() async {
      final int playlistIndex = _playlists.indexWhere(
        (Playlist p) => p.itemId.toLowerCase() == id.toLowerCase(),
      );
      if (playlistIndex < 0) {
        return false;
      }

      final String actualId = _playlists[playlistIndex].itemId;
      final List<Track> existing =
          List<Track>.of(_playlistTracksByPlaylistId[actualId] ?? const <Track>[]);
      existing.addAll(tracksToAdd);

      _playlistTracksByPlaylistId = <String, List<Track>>{
        ..._playlistTracksByPlaylistId,
        actualId: List<Track>.unmodifiable(existing),
      };

      notifyListeners();
      return true;
    });

    if (changed) {
      queuePreferencesPush();
    }

    return changed;
  }

  Future<bool> removePlaylistTracks(
    String playlistId,
    Iterable<Track> tracks,
  ) async {
    final String id = playlistId.trim();
    if (id.isEmpty) {
      return false;
    }

    final Set<String> urisToRemove = tracks
        .map((Track t) => (t.uri ?? '').trim().toLowerCase())
        .where((String uri) => uri.isNotEmpty)
        .toSet();

    if (urisToRemove.isEmpty) {
      return false;
    }

    final bool changed = await _withStateLock(() async {
      final int playlistIndex = _playlists.indexWhere(
        (Playlist p) => p.itemId.toLowerCase() == id.toLowerCase(),
      );
      if (playlistIndex < 0) {
        return false;
      }

      final String actualId = _playlists[playlistIndex].itemId;
      final List<Track> existing =
          List<Track>.of(_playlistTracksByPlaylistId[actualId] ?? const <Track>[]);
      final int before = existing.length;

      existing.removeWhere((Track track) {
        final String uri = (track.uri ?? '').trim().toLowerCase();
        return uri.isNotEmpty && urisToRemove.contains(uri);
      });

      if (existing.length == before) {
        return false;
      }

      _playlistTracksByPlaylistId = <String, List<Track>>{
        ..._playlistTracksByPlaylistId,
        actualId: List<Track>.unmodifiable(existing),
      };

      notifyListeners();
      return true;
    });

    if (changed) {
      queuePreferencesPush();
    }

    return changed;
  }

  // Snapshot loading
  void _loadFavoritesSnapshot() {
    final dynamic root = _preferences[favoritesRootKey];
    if (root is! Map) {
      _preferences[favoritesRootKey] = <String, dynamic>{
        'tracks': <Map<String, dynamic>>[],
        'albums': <Map<String, dynamic>>[],
        'playlists': <Map<String, dynamic>>[],
        'artists': <Map<String, dynamic>>[],
      };
      _favoriteTracks = const <Track>[];
      _favoriteAlbums = const <Album>[];
      _favoritePlaylists = const <Playlist>[];
      _favoriteArtists = const <Artist>[];
      return;
    }

    final Map<String, dynamic> map = _normalizeMap(root);
    _favoriteTracks = List<Track>.unmodifiable(
      _readModelList(map['tracks'], Track.fromJson, favorite: true),
    );
    _favoriteAlbums = List<Album>.unmodifiable(
      _readModelList(map['albums'], Album.fromJson, favorite: true),
    );
    _favoritePlaylists = List<Playlist>.unmodifiable(
      _readModelList(map['playlists'], Playlist.fromJson, favorite: true),
    );
    _favoriteArtists = List<Artist>.unmodifiable(
      _readModelList(map['artists'], Artist.fromJson, favorite: true),
    );

    _preferences[favoritesRootKey] = map;
  }

  void _loadPlaylistsSnapshot() {
    final dynamic root = _preferences[playlistsRootKey];
    if (root is! Map) {
      _preferences[playlistsRootKey] = <String, dynamic>{
        'playlists': <Map<String, dynamic>>[],
      };
      _playlists = const <Playlist>[];
      _playlistTracksByPlaylistId = <String, List<Track>>{};
      return;
    }

    final Map<String, dynamic> map = _normalizeMap(root);
    final List<Playlist> parsedPlaylists = <Playlist>[];
    final Map<String, List<Track>> parsedTracks = <String, List<Track>>{};

    for (final Map<String, dynamic> playlistMap in _toMapList(map['playlists'])) {
      final Playlist playlist = _asPlaylist(Playlist.fromJson(playlistMap), false);
      parsedPlaylists.add(playlist);

      final List<Track> items = _readPlaylistTrackList(playlistMap['items']);
      parsedTracks[playlist.itemId] = List<Track>.unmodifiable(items);
    }

    parsedPlaylists.sort(_comparePlaylists);
    _playlists = List<Playlist>.unmodifiable(parsedPlaylists);
    _playlistTracksByPlaylistId = parsedTracks;

    _preferences[playlistsRootKey] = map;
  }

  // Snapshot serialization
  void _convertRuntimeStateToSnapshot() {
    _preferences[favoritesRootKey] = <String, dynamic>{
      'tracks': _favoriteTracks.map(_trackToJson).toList(growable: false),
      'albums': _favoriteAlbums.map(_albumToJson).toList(growable: false),
      'playlists': _favoritePlaylists
          .map(_playlistToJson)
          .toList(growable: false),
      'artists': _favoriteArtists.map(_artistToJson).toList(growable: false),
    };

    _preferences[playlistsRootKey] = <String, dynamic>{
      'playlists': _playlists.map((Playlist playlist) {
        final String id = playlist.itemId;
        final List<Track> items =
            _playlistTracksByPlaylistId[id] ?? const <Track>[];
        return <String, dynamic>{
          ..._playlistToJson(playlist),
          'items': items.map(_trackToJson).toList(growable: false),
        };
      }).toList(growable: false),
    };
  }

  // Mapping helpers
  Track _asTrack(MediaItem item, bool favorite) {
    if (item is Track) {
      return Track(
        itemId: item.itemId,
        provider: item.provider,
        name: item.name,
        duration: item.duration,
        artists: item.artists,
        sortName: item.sortName,
        uri: item.uri,
        album: item.album,
        providerMappings: item.providerMappings,
        metadata: item.metadata,
        favorite: favorite,
      );
    }

    return Track(
      itemId: item.itemId,
      provider: item.provider,
      name: item.name,
      duration: 0,
      artists: const <Artist>[],
      sortName: item.sortName,
      uri: item.uri,
      album: null,
      providerMappings: item.providerMappings,
      metadata: item.metadata,
      favorite: favorite,
    );
  }

  Album _asAlbum(MediaItem item, bool favorite) {
    if (item is Album) {
      return Album(
        itemId: item.itemId,
        provider: item.provider,
        name: item.name,
        artists: item.artists,
        sortName: item.sortName,
        uri: item.uri,
        year: item.year,
        albumType: item.albumType,
        providerMappings: item.providerMappings,
        metadata: item.metadata,
        favorite: favorite,
      );
    }

    return Album(
      itemId: item.itemId,
      provider: item.provider,
      name: item.name,
      artists: const <Artist>[],
      sortName: item.sortName,
      uri: item.uri,
      year: null,
      albumType: null,
      providerMappings: item.providerMappings,
      metadata: item.metadata,
      favorite: favorite,
    );
  }

  Artist _asArtist(MediaItem item, bool favorite) {
    if (item is Artist) {
      return Artist(
        itemId: item.itemId,
        provider: item.provider,
        name: item.name,
        sortName: item.sortName,
        uri: item.uri,
        providerMappings: item.providerMappings,
        metadata: item.metadata,
        favorite: favorite,
      );
    }

    return Artist(
      itemId: item.itemId,
      provider: item.provider,
      name: item.name,
      sortName: item.sortName,
      uri: item.uri,
      providerMappings: item.providerMappings,
      metadata: item.metadata,
      favorite: favorite,
    );
  }

  Playlist _asPlaylist(MediaItem item, bool favorite) {
    if (item is Playlist) {
      return Playlist(
        itemId: item.itemId,
        provider: item.provider.trim().isEmpty
            ? localPlaylistProvider
            : item.provider,
        name: item.name,
        isEditable: item.isEditable,
        sortName: item.sortName,
        uri: item.uri,
        owner: item.owner,
        providerMappings: item.providerMappings,
        metadata: item.metadata,
        favorite: favorite,
      );
    }

    return Playlist(
      itemId: item.itemId,
      provider: item.provider.trim().isEmpty
          ? localPlaylistProvider
          : item.provider,
      name: item.name,
      isEditable: true,
      sortName: item.sortName,
      uri: item.uri,
      owner: null,
      providerMappings: item.providerMappings,
      metadata: item.metadata,
      favorite: favorite,
    );
  }

  bool _updateFavoriteList<T extends MediaItem>(
    List<T> list,
    String uri,
    bool favorite,
    T Function() create,
  ) {
    final int index = list.indexWhere(
      (T item) => _equalsUri(item.uri, uri),
    );

    if (favorite) {
      if (index >= 0) {
        return false;
      }
      list.add(create());
      return true;
    }

    if (index < 0) {
      return false;
    }
    list.removeAt(index);
    return true;
  }

  int _comparePlaylists(Playlist left, Playlist right) {
    final String leftSort = (left.sortName ?? '').toLowerCase();
    final String rightSort = (right.sortName ?? '').toLowerCase();
    final int sortCompare = leftSort.compareTo(rightSort);
    if (sortCompare != 0) {
      return sortCompare;
    }

    return left.name.toLowerCase().compareTo(right.name.toLowerCase());
  }

  String _generateNextLocalPlaylistId() {
    final int next = _playlists
            .map((Playlist p) => int.tryParse(p.itemId.trim()) ?? 0)
            .where((int id) => id > 0)
            .fold<int>(0, (int maxValue, int value) => value > maxValue ? value : maxValue) +
        1;
    return next.toString();
  }

  bool _equalsUri(String? value, String uri) {
    return (value ?? '').trim().toLowerCase() == uri.trim().toLowerCase();
  }

  Future<T> _withStateLock<T>(Future<T> Function() action) {
    final Future<void> previous = _stateLock;
    final Completer<void> completer = Completer<void>();
    _stateLock = completer.future;

    return previous.then((_) => action()).whenComplete(() {
      completer.complete();
    });
  }

  List<T> _readModelList<T extends MediaItem>(
    dynamic value,
    T Function(Map<String, dynamic>) factory,
    {
    required bool favorite,
  }) {
    final List<T> result = <T>[];
    for (final Map<String, dynamic> map in _toMapList(value)) {
      try {
        final T item = factory(map);
        if (item is Track) {
          result.add(_asTrack(item, favorite) as T);
        } else if (item is Album) {
          result.add(_asAlbum(item, favorite) as T);
        } else if (item is Artist) {
          result.add(_asArtist(item, favorite) as T);
        } else if (item is Playlist) {
          result.add(_asPlaylist(item, favorite) as T);
        } else {
          result.add(item);
        }
      } catch (_) {
        continue;
      }
    }
    return result;
  }

  List<Map<String, dynamic>> _toMapList(dynamic value) {
    if (value is! List) {
      return const <Map<String, dynamic>>[];
    }

    return value
        .whereType<Map>()
        .map((Map entry) => entry.map(
              (dynamic key, dynamic nestedValue) =>
                  MapEntry(key.toString(), _normalizeValue(nestedValue)),
            ))
        .map((Map<String, dynamic> entry) =>
            entry..removeWhere((String _, dynamic nestedValue) => nestedValue == null))
        .toList(growable: false);
  }

  List<Track> _readPlaylistTrackList(dynamic value) {
    final List<Track> tracks = <Track>[];
    for (final Map<String, dynamic> raw in _toMapList(value)) {
      try {
        tracks.add(_asTrack(Track.fromJson(raw), false));
      } catch (_) {
        continue;
      }
    }
    return tracks;
  }

  static void synchronized(Object lock, void Function() action) {
    // Flutter runs in a single isolate; this guard keeps push-queue state
    // updates in one explicit place instead of spreading flag changes.
    action();
  }

  Map<String, dynamic> _normalizeMap(Map<dynamic, dynamic>? source) {
    final Map<String, dynamic> normalized = <String, dynamic>{};
    if (source == null) {
      return normalized;
    }

    for (final MapEntry<dynamic, dynamic> entry in source.entries) {
      final String key = entry.key.toString().trim();
      if (key.isEmpty) {
        continue;
      }

      final dynamic value = _normalizeValue(entry.value);
      if (value != null) {
        normalized[key] = value;
      }
    }

    return normalized;
  }

  dynamic _normalizeValue(dynamic value) {
    if (value == null) {
      return null;
    }

    if (value is Map) {
      return _normalizeMap(value.cast<dynamic, dynamic>());
    }

    if (value is List) {
      return value
          .map(_normalizeValue)
          .where((dynamic item) => item != null)
          .toList(growable: false);
    }

    if (value is num || value is bool || value is String) {
      return value;
    }

    return value;
  }

  Map<String, dynamic> _cloneMap(Map<String, dynamic> source) {
    final Map<String, dynamic> clone = <String, dynamic>{};
    for (final MapEntry<String, dynamic> entry in source.entries) {
      final dynamic cloned = _cloneValue(entry.value);
      if (cloned != null) {
        clone[entry.key] = cloned;
      }
    }
    return clone;
  }

  dynamic _cloneValue(dynamic value) {
    if (value == null) {
      return null;
    }
    if (value is Map<String, dynamic>) {
      return _cloneMap(value);
    }
    if (value is List) {
      return value
          .map(_cloneValue)
          .where((dynamic item) => item != null)
          .toList(growable: false);
    }
    return value;
  }

  Map<String, dynamic> _providerMappingToJson(ProviderMapping mapping) {
    return <String, dynamic>{
      'item_id': mapping.itemId,
      'provider_domain': mapping.providerDomain,
      'provider_instance': mapping.providerInstance,
      'available': mapping.available,
      'url': mapping.url,
    };
  }

  Map<String, dynamic> _imageToJson(MediaItemImage image) {
    return <String, dynamic>{
      'type': image.type,
      'path': image.path,
      'provider': image.provider,
      'remotely_accessible': image.remotelyAccessible,
      if (image.proxyId != null) 'proxy_id': image.proxyId,
    };
  }

  Map<String, dynamic>? _metadataToJson(MediaItemMetadata? metadata) {
    if (metadata == null) {
      return null;
    }

    return <String, dynamic>{
      'description': metadata.description,
      'images': metadata.images.map(_imageToJson).toList(growable: false),
      'genres': metadata.genres,
      'label': metadata.label,
      'popularity': metadata.popularity,
      'cache_checksum': metadata.cacheChecksum,
    };
  }

  Map<String, dynamic> _artistToJson(Artist artist) {
    return <String, dynamic>{
      'item_id': artist.itemId,
      'provider': artist.provider,
      'name': artist.name,
      'sort_name': artist.sortName,
      'uri': artist.uri,
      'media_type': 'artist',
      'provider_mappings': artist.providerMappings
          .map(_providerMappingToJson)
          .toList(growable: false),
      'metadata': _metadataToJson(artist.metadata),
      'favorite': artist.favorite,
    };
  }

  Map<String, dynamic> _albumToJson(Album album) {
    return <String, dynamic>{
      'item_id': album.itemId,
      'provider': album.provider,
      'name': album.name,
      'sort_name': album.sortName,
      'uri': album.uri,
      'media_type': 'album',
      'year': album.year,
      'album_type': album.albumType,
      'artists': album.artists.map(_artistToJson).toList(growable: false),
      'provider_mappings': album.providerMappings
          .map(_providerMappingToJson)
          .toList(growable: false),
      'metadata': _metadataToJson(album.metadata),
      'favorite': album.favorite,
    };
  }

  Map<String, dynamic> _trackToJson(Track track) {
    return <String, dynamic>{
      'item_id': track.itemId,
      'provider': track.provider,
      'name': track.name,
      'sort_name': track.sortName,
      'uri': track.uri,
      'media_type': 'track',
      'duration': track.duration,
      'artists': track.artists.map(_artistToJson).toList(growable: false),
      'album': track.album == null ? null : _albumToJson(track.album!),
      'provider_mappings': track.providerMappings
          .map(_providerMappingToJson)
          .toList(growable: false),
      'metadata': _metadataToJson(track.metadata),
      'favorite': track.favorite,
    };
  }

  Map<String, dynamic> _playlistToJson(Playlist playlist) {
    return <String, dynamic>{
      'item_id': playlist.itemId,
      'provider': playlist.provider,
      'name': playlist.name,
      'sort_name': playlist.sortName,
      'uri': playlist.uri,
      'media_type': 'playlist',
      'owner': playlist.owner,
      'is_editable': playlist.isEditable,
      'provider_mappings': playlist.providerMappings
          .map(_providerMappingToJson)
          .toList(growable: false),
      'metadata': _metadataToJson(playlist.metadata),
      'favorite': playlist.favorite,
    };
  }
}