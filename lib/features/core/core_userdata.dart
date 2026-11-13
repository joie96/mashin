import 'dart:async';

import 'package:flutter/foundation.dart';

import '../shared/item_provider_selection.dart';
import 'core_bridge.dart';
import 'core_models.dart';

class _PendingFavoriteChange {
  _PendingFavoriteChange(this.items, this.isFavorite);

  final List<MediaItem> items;
  final bool isFavorite;
  bool hasCompleted = false;
}

/// Projects native favorites and private playlists for Flutter views.
class UserData extends ChangeNotifier {
  UserData({required MashinCoreBridgeApi coreBridge})
    : _coreBridge = coreBridge {
    _eventSubscription = coreBridge.events.listen(_onCoreEvent);
  }

  final MashinCoreBridgeApi _coreBridge;
  final ChangeNotifier _favoritesChanges = ChangeNotifier();
  final ChangeNotifier _playlistsChanges = ChangeNotifier();
  final Map<String, ChangeNotifier> _playlistChanges = {};
  bool _hasAppliedSnapshot = false;
  int _nativeSnapshotRevision = 0;
  final Map<String, int> _playlistRevisions = {};
  late final StreamSubscription<Map<String, dynamic>> _eventSubscription;
  Future<void>? _snapshotRefresh;
  bool _needsSnapshotRefresh = false;
  int _snapshotRevision = 0;
  int _requiredNativeSnapshotRevision = 0;
  int _unversionedSnapshotRequest = 0;
  bool _isLoadingLibraryData = false;
  bool _isLoadingFavorites = false;
  bool _hasLoadedFavorites = false;
  Map<(MediaType, String, String), bool> _favoriteOverrides = {};
  final List<_PendingFavoriteChange> _pendingFavorites = [];
  bool _isLoadingPlaylists = false;
  Set<String> _loadingPlaylistIds = <String>{};
  bool _isDisposed = false;
  List<Track> _favoriteTracks = const <Track>[];
  List<Album> _favoriteAlbums = const <Album>[];
  List<Playlist> _favoritePlaylists = const <Playlist>[];
  List<Artist> _favoriteArtists = const <Artist>[];
  Set<(MediaType, String, String)> _favoriteIdentityKeys = {};
  Set<(MediaType, String)> _favoriteUriKeys = {};
  List<Playlist> _playlists = const <Playlist>[];
  Map<String, List<Track>> _playlistTracksByPlaylistId =
      const <String, List<Track>>{};

  bool get isLoadingLibraryData => _isLoadingLibraryData;
  bool get isLoadingFavorites => _isLoadingFavorites;
  bool get isLoadingPlaylists => _isLoadingPlaylists;
  bool isLoadingPlaylistTracks(String playlistId) =>
      _loadingPlaylistIds.contains(playlistId);
  List<Track> get favoriteTracks => _projectPendingFavorites(_favoriteTracks);
  List<Album> get favoriteAlbums => _projectPendingFavorites(_favoriteAlbums);
  List<Playlist> get favoritePlaylists =>
      _projectPendingFavorites(_favoritePlaylists);
  List<Artist> get favoriteArtists =>
      _projectPendingFavorites(_favoriteArtists);
  List<Playlist> get playlists => _playlists;
  Listenable get favoritesChanges => _favoritesChanges;
  Listenable get playlistsChanges => _playlistsChanges;
  Listenable playlistChanges(String playlistId) =>
      _playlistChanges.putIfAbsent(playlistId, ChangeNotifier.new);

  bool isPrivatePlaylist(Playlist playlist) => _playlists.any(
    (candidate) =>
        candidate.itemId == playlist.itemId &&
        candidate.provider == playlist.provider,
  );

  List<Track> tracksForPlaylist(String playlistId) =>
      _playlistTracksByPlaylistId[playlistId.trim()] ?? const <Track>[];

  Future<void> loadLibraryData() async {
    if (_isDisposed) return;
    final snapshotRevision = ++_snapshotRevision;
    _isLoadingLibraryData = true;
    _setLibraryLoading(true);
    notifyListeners();
    try {
      if (_isDisposed) return;
      final snapshot = await _coreBridge.loadUserData();
      if (!_isDisposed && snapshotRevision == _snapshotRevision) {
        _applySnapshot(snapshot);
      }
    } catch (_) {
      if (!_isDisposed && snapshotRevision == _snapshotRevision) {
        final loadingIds = _loadingPlaylistIds;
        _loadingPlaylistIds = <String>{};
        _setLibraryLoading(false);
        for (final itemId in loadingIds) {
          _playlistChanges[itemId]?.notifyListeners();
        }
      }
      rethrow;
    } finally {
      _isLoadingLibraryData = false;
      if (!_isDisposed) notifyListeners();
    }
  }

  // Reads the current Rust snapshot without restarting its session-owned loading.
  Future<void> refreshLibrarySnapshot() => _refreshSnapshot();

  bool isFavorite(MediaItem item) {
    for (final change in _pendingFavorites.reversed) {
      if (change.items.any(
        (candidate) => _isSameFavoriteItem(candidate, item),
      )) {
        return change.isFavorite;
      }
    }
    for (final key in _itemIdentityKeys(item)) {
      if (_favoriteOverrides[key] case final known?) return known;
    }
    if (_favoriteUriKeys.contains((item.mediaType, item.uri ?? '')) ||
        _itemIdentityKeys(item).any(_favoriteIdentityKeys.contains)) {
      return true;
    }
    return _hasLoadedFavorites ? false : item.favorite;
  }

  T withFavoriteStatus<T extends MediaItem>(T item) {
    final bool favorite = isFavorite(item);
    if (item.favorite == favorite) return item;
    if (item is Track) return _copyTrack(item, favorite) as T;
    if (item is Album) return _copyAlbum(item, favorite) as T;
    if (item is Artist) return _copyArtist(item, favorite) as T;
    if (item is Playlist) return _copyPlaylist(item, favorite) as T;
    return item;
  }

  Future<void> setFavorite(Iterable<MediaItem> items, bool favorite) async {
    if (_isDisposed) return;
    final values = items.toList(growable: false);
    if (values.isEmpty) return;
    final change = _PendingFavoriteChange(values, favorite);
    _pendingFavorites.add(change);
    _favoritesChanges.notifyListeners();
    notifyListeners();
    try {
      await _mutateUserData(
        () => _coreBridge.setUserDataFavorites(values, favorite),
      );
    } catch (_) {
      _pendingFavorites.remove(change);
      rethrow;
    } finally {
      change.hasCompleted = true;
      while (_pendingFavorites.isNotEmpty &&
          _pendingFavorites.first.hasCompleted) {
        _pendingFavorites.removeAt(0);
      }
      if (!_isDisposed) {
        _favoritesChanges.notifyListeners();
        notifyListeners();
      }
    }
  }

  Future<Playlist?> createPrivatePlaylist(String name) async {
    final String normalizedName = name.trim();
    if (normalizedName.isEmpty) return null;
    await _mutateUserData(
      () => _coreBridge.createPrivatePlaylist(normalizedName),
    );
    return _playlists.lastWhere(
      (playlist) => playlist.name == normalizedName,
      orElse: () =>
          throw StateError('Rust did not return the created private playlist'),
    );
  }

  Future<bool> renamePlaylist(Playlist playlist, String name) async {
    if (playlist.itemId.trim().isEmpty || name.trim().isEmpty) return false;
    await _mutateUserData(
      () => _coreBridge.renamePrivatePlaylist(playlist.itemId, name.trim()),
      playlistId: playlist.itemId,
    );
    return true;
  }

  Future<bool> refreshPrivatePlaylistCover(Playlist playlist) async {
    if (!isPrivatePlaylist(playlist) || !playlist.isEditable) return false;
    await _mutateUserData(
      () => _coreBridge.refreshPrivatePlaylistCover(playlist.itemId),
      playlistId: playlist.itemId,
    );
    return true;
  }

  Future<bool> removePlaylist(Playlist playlist) async {
    if (playlist.itemId.trim().isEmpty) return false;
    await _mutateUserData(
      () => _coreBridge.removePrivatePlaylist(playlist.itemId),
      playlistId: playlist.itemId,
    );
    return true;
  }

  Future<bool> addPlaylistTracks(
    String playlistId,
    Iterable<Track> tracks,
  ) async {
    if (playlistId.trim().isEmpty) return false;
    final List<Track> values = <Track>[];
    for (final track in tracks) {
      final selection = ItemProviderSelection.forItem(track);
      await selection.initialize();
      if ((selection.selectedUri ?? '').trim().isNotEmpty) {
        values.add(track);
      }
    }
    if (values.isEmpty) return false;
    await _mutateUserData(
      () => _coreBridge.addPrivatePlaylistTracks(playlistId, values),
      playlistId: playlistId,
    );
    return true;
  }

  Future<bool> removePlaylistTracks(
    String playlistId,
    Iterable<Track> tracks,
  ) async {
    final List<String> uris = tracks
        .map((track) => track.uri?.trim() ?? '')
        .where((uri) => uri.isNotEmpty)
        .toList(growable: false);
    if (playlistId.trim().isEmpty || uris.isEmpty) return false;
    await _mutateUserData(
      () => _coreBridge.removePrivatePlaylistTracks(playlistId, uris),
      playlistId: playlistId,
    );
    return true;
  }

  // Snapshot refresh and event processing.
  Future<void> _mutateUserData(
    Future<void> Function() command, {
    String? playlistId,
  }) async {
    int observedRevision() => playlistId == null
        ? _nativeSnapshotRevision
        : (_playlistRevisions[playlistId] ?? 0) > _nativeSnapshotRevision
        ? _playlistRevisions[playlistId]!
        : _nativeSnapshotRevision;
    final previousRevision = observedRevision();
    final previousSnapshotRequest = _snapshotRevision;
    await command();
    if (_isDisposed) return;
    if (_snapshotRefresh != null &&
        _snapshotRevision > previousSnapshotRequest) {
      await _snapshotRefresh;
      return;
    }
    if (_hasAppliedSnapshot && observedRevision() > previousRevision) {
      await _snapshotRefresh;
      return;
    }
    await _refreshSnapshot();
  }

  Future<void> _onCoreEvent(Map<String, dynamic> event) async {
    if (_isDisposed) return;
    try {
      if (event['type'] == 'user_data_metadata_changed') {
        final snapshot = _map(event['snapshot']);
        final revision = snapshot['revision'] as int;
        if (revision <= _nativeSnapshotRevision) return;
        final latest = _playlistRevisions.values.fold(
          _nativeSnapshotRevision,
          (latest, value) => value > latest ? value : latest,
        );
        if (!_hasAppliedSnapshot || revision != latest + 1) {
          await _refreshSnapshot(nativeRevision: revision);
        } else {
          _applySnapshot(
            snapshot,
            includesFavorites: false,
            includesPlaylistItems: false,
          );
          if (_snapshotRefresh != null) {
            await _refreshSnapshot(nativeRevision: revision);
          }
        }
      } else if (event['type'] == 'user_data_favorites_changed') {
        final snapshot = _map(event['snapshot']);
        final revision = snapshot['revision'] as int;
        final latest = _playlistRevisions.values.fold(
          _nativeSnapshotRevision,
          (latest, value) => value > latest ? value : latest,
        );
        if (revision <= latest) return;
        if (!_hasAppliedSnapshot ||
            revision != latest + 1 ||
            _snapshotRefresh != null) {
          await _refreshSnapshot(nativeRevision: revision);
        } else {
          _applySnapshot(snapshot, includesPlaylists: false);
        }
      } else if (event['type'] == 'user_data_changed') {
        final playlist = event['playlist'];
        if (playlist is! Map) {
          await _refreshSnapshot();
        } else {
          final revision = event['revision'] as int;
          if (revision <= _nativeSnapshotRevision ||
              revision <= (_playlistRevisions[playlist['item_id']] ?? 0)) {
            return;
          }
          if (!_applyPlaylistSnapshot(_map(playlist), revision) ||
              _snapshotRefresh != null) {
            await _refreshSnapshot(nativeRevision: revision);
          }
        }
      }
    } catch (error, stackTrace) {
      FlutterError.reportError(
        FlutterErrorDetails(exception: error, stack: stackTrace),
      );
    }
  }

  Future<void> _refreshSnapshot({int? nativeRevision}) {
    if (_isDisposed) return Future<void>.value();
    _snapshotRevision++;
    if (nativeRevision == null) {
      _unversionedSnapshotRequest++;
    } else if (nativeRevision > _requiredNativeSnapshotRevision) {
      _requiredNativeSnapshotRevision = nativeRevision;
    }
    _needsSnapshotRefresh = true;
    return _snapshotRefresh ??= Future<void>.microtask(_drainSnapshotRefreshes);
  }

  Future<void> _drainSnapshotRefreshes() async {
    try {
      while (_needsSnapshotRefresh && !_isDisposed) {
        _needsSnapshotRefresh = false;
        final unversionedRequest = _unversionedSnapshotRequest;
        try {
          final snapshot = await _coreBridge.getUserData();
          if (!_isDisposed &&
              unversionedRequest == _unversionedSnapshotRequest &&
              (snapshot['revision'] as int? ?? 0) >=
                  _requiredNativeSnapshotRevision) {
            _applySnapshot(snapshot);
            _needsSnapshotRefresh = false;
          }
        } catch (_) {
          if (!_isDisposed && !_needsSnapshotRefresh) rethrow;
        }
      }
    } finally {
      _snapshotRefresh = null;
    }
  }

  void _setLibraryLoading(bool isLoading) {
    final favoritesChanged = _isLoadingFavorites != isLoading;
    final playlistsChanged = _isLoadingPlaylists != isLoading;
    _isLoadingFavorites = isLoading;
    _isLoadingPlaylists = isLoading;
    if (favoritesChanged) _favoritesChanges.notifyListeners();
    if (playlistsChanged) _playlistsChanges.notifyListeners();
  }

  void _applySnapshot(
    Map<String, dynamic> snapshot, {
    bool includesPlaylists = true,
    bool includesFavorites = true,
    bool includesPlaylistItems = true,
  }) {
    final revision = snapshot['revision'] as int? ?? 0;
    if (revision < _nativeSnapshotRevision ||
        _playlistRevisions.values.any((value) => value > revision)) {
      return;
    }
    _snapshotRevision++;
    _nativeSnapshotRevision = revision;
    _playlistRevisions.removeWhere((_, value) => value <= revision);
    final previousLoadingFavorites = _isLoadingFavorites;
    final previousLoadedFavorites = _hasLoadedFavorites;
    final previousOverrides = _favoriteOverrides;
    final previousLoadingPlaylists = _isLoadingPlaylists;
    final previousLoadingIds = _loadingPlaylistIds;
    final previousTracks = _playlistTracksByPlaylistId;
    final previousPlaylists = _playlists;
    final previousFavoriteTracks = _favoriteTracks;
    final previousFavoriteAlbums = _favoriteAlbums;
    final previousFavoritePlaylists = _favoritePlaylists;
    final previousFavoriteArtists = _favoriteArtists;
    _isLoadingFavorites = snapshot['is_loading_favorites'] == true;
    _hasLoadedFavorites = snapshot['has_loaded_favorites'] == true;
    if (includesFavorites) {
      _favoriteOverrides = {
        for (final entry in _listOfMaps(snapshot['favorite_overrides']))
          for (final key in _itemIdentityKeys(
            MediaItem.fromJson(_map(entry['item'])),
          ))
            key: entry['is_favorite'] == true,
      };
      final Map<String, dynamic> favorites = _map(snapshot['favorites']);
      _favoriteTracks = _retainMediaList(
        _favoriteTracks,
        _readTracks(favorites['tracks'], true),
      );
      _favoriteAlbums = _retainMediaList(
        _favoriteAlbums,
        _readAlbums(favorites['albums'], true),
      );
      _favoritePlaylists = _retainMediaList(
        _favoritePlaylists,
        _readPlaylists(favorites['playlists'], true),
      );
      _favoriteArtists = _retainMediaList(
        _favoriteArtists,
        _readArtists(favorites['artists'], true),
      );
      final favoriteItems = <MediaItem>[
        ..._favoriteTracks,
        ..._favoriteAlbums,
        ..._favoritePlaylists,
        ..._favoriteArtists,
      ];
      _favoriteIdentityKeys = favoriteItems.expand(_itemIdentityKeys).toSet();
      _favoriteUriKeys = {
        for (final item in favoriteItems)
          if ((item.uri ?? '').trim().isNotEmpty)
            (item.mediaType, item.uri!.trim()),
      };
    }
    if (includesPlaylists) {
      _isLoadingPlaylists = snapshot['is_loading_playlists'] == true;
    }

    final Map<String, List<Track>> tracksByPlaylist = includesPlaylists
        ? <String, List<Track>>{}
        : previousTracks;
    final loadingPlaylistIds = includesPlaylists
        ? <String>{}
        : previousLoadingIds;
    final List<Playlist> playlists = <Playlist>[];
    for (final Map<String, dynamic> value in _listOfMaps(
      includesPlaylists ? snapshot['playlists'] : null,
    )) {
      try {
        final Playlist playlist = Playlist.fromJson(value);
        playlists.add(playlist);
        if (value['is_loading_tracks'] == true) {
          loadingPlaylistIds.add(playlist.itemId);
        }
        tracksByPlaylist[playlist.itemId] = includesPlaylistItems
            ? _retainMediaList(
                previousTracks[playlist.itemId] ?? const <Track>[],
                _readTracks(value['items'], null),
              )
            : previousTracks[playlist.itemId] ?? const <Track>[];
      } on FormatException {
        continue;
      }
    }
    if (includesPlaylists) {
      playlists.sort(_comparePlaylists);
      _playlists = _retainMediaList(
        _playlists,
        List<Playlist>.unmodifiable(playlists),
      );
      _loadingPlaylistIds = loadingPlaylistIds;
      _playlistTracksByPlaylistId = Map<String, List<Track>>.unmodifiable(
        tracksByPlaylist,
      );
    }
    final favoritesChanged =
        previousLoadingFavorites != _isLoadingFavorites ||
        previousLoadedFavorites != _hasLoadedFavorites ||
        !mapEquals(previousOverrides, _favoriteOverrides) ||
        !identical(previousFavoriteTracks, _favoriteTracks) ||
        !identical(previousFavoriteAlbums, _favoriteAlbums) ||
        !identical(previousFavoritePlaylists, _favoritePlaylists) ||
        !identical(previousFavoriteArtists, _favoriteArtists);
    final playlistsChanged =
        previousLoadingPlaylists != _isLoadingPlaylists ||
        !identical(previousPlaylists, _playlists);
    final changedPlaylistIds = <String>{
      for (final itemId in {...previousTracks.keys, ...tracksByPlaylist.keys})
        if (!identical(previousTracks[itemId], tracksByPlaylist[itemId]) ||
            previousLoadingIds.contains(itemId) !=
                loadingPlaylistIds.contains(itemId))
          itemId,
    };
    final shouldNotify =
        !_hasAppliedSnapshot ||
        favoritesChanged ||
        playlistsChanged ||
        changedPlaylistIds.isNotEmpty;
    _hasAppliedSnapshot = true;
    if (favoritesChanged) _favoritesChanges.notifyListeners();
    if (playlistsChanged) _playlistsChanges.notifyListeners();
    for (final itemId in changedPlaylistIds) {
      _playlistChanges[itemId]?.notifyListeners();
    }
    if (shouldNotify) notifyListeners();
  }

  bool _applyPlaylistSnapshot(Map<String, dynamic> value, int revision) {
    final playlist = Playlist.fromJson(value);
    final index = _playlists.indexWhere(
      (item) =>
          item.itemId == playlist.itemId && item.provider == playlist.provider,
    );
    if (index < 0) return false;
    _playlistRevisions[playlist.itemId] = revision;
    final previous =
        _playlistTracksByPlaylistId[playlist.itemId] ?? const <Track>[];
    final tracks = _retainMediaList(
      previous,
      _readTracks(value['items'], null),
    );
    final isLoading = value['is_loading_tracks'] == true;
    final tracksChanged =
        !identical(previous, tracks) ||
        _loadingPlaylistIds.contains(playlist.itemId) != isLoading;
    final metadataChanged = !hasSameMediaContent(_playlists[index], playlist);
    if (!tracksChanged && !metadataChanged) return true;
    if (metadataChanged) {
      final playlists = _playlists.toList();
      playlists[index] = playlist;
      playlists.sort(_comparePlaylists);
      _playlists = List.unmodifiable(playlists);
    }
    if (tracksChanged) {
      _playlistTracksByPlaylistId = Map.unmodifiable({
        ..._playlistTracksByPlaylistId,
        playlist.itemId: tracks,
      });
      _loadingPlaylistIds = {..._loadingPlaylistIds};
      if (isLoading) {
        _loadingPlaylistIds.add(playlist.itemId);
      } else {
        _loadingPlaylistIds.remove(playlist.itemId);
      }
    }
    if (metadataChanged) _playlistsChanges.notifyListeners();
    if (tracksChanged) _playlistChanges[playlist.itemId]?.notifyListeners();
    notifyListeners();
    return true;
  }

  List<T> _retainMediaList<T extends MediaItem>(
    List<T> previous,
    List<T> incoming,
  ) => hasSameMediaListContent(previous, incoming) ? previous : incoming;

  List<Track> _readTracks(Object? value, bool? favorite) =>
      List<Track>.unmodifiable(
        _listOfMaps(value).map(
          (json) => favorite == null
              ? Track.fromJson(json)
              : _copyTrack(Track.fromJson(json), favorite),
        ),
      );
  List<Album> _readAlbums(Object? value, bool favorite) =>
      List<Album>.unmodifiable(
        _listOfMaps(
          value,
        ).map((json) => _copyAlbum(Album.fromJson(json), favorite)),
      );
  List<Playlist> _readPlaylists(Object? value, bool favorite) =>
      List<Playlist>.unmodifiable(
        _listOfMaps(
          value,
        ).map((json) => _copyPlaylist(Playlist.fromJson(json), favorite)),
      );
  List<Artist> _readArtists(Object? value, bool favorite) =>
      List<Artist>.unmodifiable(
        _listOfMaps(
          value,
        ).map((json) => _copyArtist(Artist.fromJson(json), favorite)),
      );

  Map<String, dynamic> _map(Object? value) => value is Map
      ? Map<String, dynamic>.from(value)
      : const <String, dynamic>{};
  List<Map<String, dynamic>> _listOfMaps(Object? value) => value is List
      ? value
            .whereType<Map>()
            .map((item) => Map<String, dynamic>.from(item))
            .toList(growable: false)
      : const <Map<String, dynamic>>[];
  Iterable<(MediaType, String, String)> _itemIdentityKeys(
    MediaItem item,
  ) sync* {
    yield (item.mediaType, item.provider, item.itemId);
    for (final mapping in item.providerMappings) {
      yield (item.mediaType, mapping.providerInstance, mapping.itemId);
    }
  }

  int _comparePlaylists(Playlist left, Playlist right) {
    final int sortComparison = (left.sortName ?? '').toLowerCase().compareTo(
      (right.sortName ?? '').toLowerCase(),
    );
    return sortComparison != 0
        ? sortComparison
        : left.name.toLowerCase().compareTo(right.name.toLowerCase());
  }

  // Pending UI wishes never change the confirmed native snapshot.
  bool _isSameFavoriteItem(MediaItem left, MediaItem right) {
    if (left.mediaType != right.mediaType) return false;
    final uri = left.uri?.trim() ?? '';
    if (uri.isNotEmpty && uri == right.uri?.trim()) return true;
    final keys = _itemIdentityKeys(left).toSet();
    return _itemIdentityKeys(right).any(keys.contains);
  }

  List<T> _projectPendingFavorites<T extends MediaItem>(List<T> confirmed) {
    if (_pendingFavorites.isEmpty) return confirmed;
    final projected = confirmed.where(isFavorite).toList();
    for (final change in _pendingFavorites) {
      for (final item in change.items.whereType<T>()) {
        if (isFavorite(item) &&
            !projected.any(
              (candidate) => _isSameFavoriteItem(candidate, item),
            )) {
          projected.add(withFavoriteStatus(item));
        }
      }
    }
    return List<T>.unmodifiable(projected);
  }

  // Favorite projection changes membership only, never playback or provider data.
  Track _copyTrack(Track item, bool favorite) => Track(
    itemId: item.itemId,
    provider: item.provider,
    name: item.name,
    version: item.version,
    externalIds: item.externalIds,
    position: item.position,
    dateAdded: item.dateAdded,
    duration: item.duration,
    discNumber: item.discNumber,
    trackNumber: item.trackNumber,
    artists: item.artists,
    sortName: item.sortName,
    uri: item.uri,
    isPlayable: item.isPlayable,
    available: item.available,
    album: item.album,
    image: item.image,
    providerMappings: item.providerMappings,
    metadata: item.metadata,
    favorite: favorite,
  );
  Album _copyAlbum(Album item, bool favorite) => Album(
    itemId: item.itemId,
    provider: item.provider,
    name: item.name,
    version: item.version,
    externalIds: item.externalIds,
    position: item.position,
    dateAdded: item.dateAdded,
    artists: item.artists,
    sortName: item.sortName,
    uri: item.uri,
    isPlayable: item.isPlayable,
    available: item.available,
    year: item.year,
    isYearKnown: item.isYearKnown,
    albumType: item.albumType,
    providerMappings: item.providerMappings,
    metadata: item.metadata,
    favorite: favorite,
  );
  Artist _copyArtist(Artist item, bool favorite) => Artist(
    itemId: item.itemId,
    provider: item.provider,
    name: item.name,
    version: item.version,
    externalIds: item.externalIds,
    position: item.position,
    dateAdded: item.dateAdded,
    artistType: item.artistType,
    sortName: item.sortName,
    uri: item.uri,
    isPlayable: item.isPlayable,
    available: item.available,
    providerMappings: item.providerMappings,
    metadata: item.metadata,
    favorite: favorite,
  );
  Playlist _copyPlaylist(Playlist item, bool favorite) => Playlist(
    itemId: item.itemId,
    provider: item.provider,
    name: item.name,
    version: item.version,
    externalIds: item.externalIds,
    position: item.position,
    dateAdded: item.dateAdded,
    isEditable: item.isEditable,
    isDynamic: item.isDynamic,
    access: item.access,
    sortName: item.sortName,
    uri: item.uri,
    isPlayable: item.isPlayable,
    available: item.available,
    owner: item.owner,
    providerMappings: item.providerMappings,
    metadata: item.metadata,
    favorite: favorite,
  );

  @override
  void dispose() {
    _isDisposed = true;
    unawaited(_eventSubscription.cancel());
    _favoritesChanges.dispose();
    _playlistsChanges.dispose();
    for (final changes in _playlistChanges.values) {
      changes.dispose();
    }
    super.dispose();
  }

  @override
  void notifyListeners() {
    if (!_isDisposed) super.notifyListeners();
  }
}
