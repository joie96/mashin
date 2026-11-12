import 'dart:async';

import 'package:flutter/foundation.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_query_scope.dart';
import '../core/core_userdata.dart';

/// Holds search state and loading logic for tracks, albums, playlists and artists.
class SearchController extends ChangeNotifier {
  final MashinCoreBridgeApi coreBridge;
  final UserData? userDataController;

  SearchController({required this.coreBridge, this.userDataController}) {
    _catalogSubscription = coreBridge.catalogUpdates.listen(_onCatalogUpdate);
    _readSubscription = coreBridge.catalogReadChanges.listen(
      _onCatalogReadChange,
    );
    _cacheSubscription = coreBridge.mediaReadInvalidations.listen((_) {
      final query = _activeQuery;
      if (!_isDisposed && query != null) {
        unawaited(_runSearch(query, shouldRetainResults: true));
      }
    });
  }

  StreamSubscription<CatalogUpdate>? _catalogSubscription;
  StreamSubscription<CatalogReadChange>? _readSubscription;
  StreamSubscription<void>? _cacheSubscription;
  String? _activeQuery;
  CoreQueryScope _queryScope = CoreQueryScope();
  final _artistAlbumScopes = <(String, String), CoreQueryScope>{};
  final _observedArtistAlbums = <(String, String), List<Album>>{};
  final _indexesByAlbum = <(String, String), List<int>>{};
  final _albumCandidates = <(String, String), Map<(String, String), Album>>{};
  List<Track> _sourceTracks = const <Track>[];

  int _requestId = 0;
  bool _isDisposed = false;

  bool _hasSearchRequest = false;
  bool _isLoadingTracks = false;
  bool _isLoadingAlbums = false;
  bool _isLoadingPlaylists = false;
  bool _isLoadingArtists = false;

  List<Track> _tracks = const <Track>[];
  List<Album> _albums = const <Album>[];
  List<Playlist> _playlists = const <Playlist>[];
  List<Artist> _artists = const <Artist>[];
  final ValueNotifier<bool> requestChanges = ValueNotifier(false);
  final Map<MediaType, ChangeNotifier> _sectionChanges = {
    for (final type in [
      MediaType.track,
      MediaType.album,
      MediaType.playlist,
      MediaType.artist,
    ])
      type: ChangeNotifier(),
  };

  Listenable sectionChanges(MediaType type) => _sectionChanges[type]!;

  // Lifecycle.
  @override
  void dispose() {
    _isDisposed = true;
    _queryScope.cancel();
    _clearTrackAlbums();
    unawaited(_catalogSubscription?.cancel());
    unawaited(_readSubscription?.cancel());
    unawaited(_cacheSubscription?.cancel());
    _requestId++;
    requestChanges.dispose();
    for (final notifier in _sectionChanges.values) {
      notifier.dispose();
    }
    super.dispose();
  }

  // Search commands and favorite projection.
  Future<void> runSearch(String query) =>
      _runSearch(query, shouldRetainResults: false);

  void resetResults() => _clearResults();

  void refreshFavoriteStatuses() {
    _sourceTracks = _withFavoriteStatuses(_sourceTracks);
    _tracks = _withFavoriteStatuses(_tracks);
    _albums = _withFavoriteStatuses(_albums);
    _playlists = _withFavoriteStatuses(_playlists);
    _artists = _withFavoriteStatuses(_artists);
    _notifyListenersIfAlive();
  }

  // Current result snapshots.
  bool get hasSearchRequest => _hasSearchRequest;
  bool get isLoadingTracks => _isLoadingTracks;
  bool get isLoadingAlbums => _isLoadingAlbums;
  bool get isLoadingPlaylists => _isLoadingPlaylists;
  bool get isLoadingArtists => _isLoadingArtists;

  List<Track> get tracks => _tracks;
  List<Album> get albums => _albums;
  List<Playlist> get playlists => _playlists;
  List<Artist> get artists => _artists;

  // Search execution.
  Future<void> _runSearch(
    String query, {
    required bool shouldRetainResults,
  }) async {
    if (_isDisposed) return;
    if (query.isEmpty) {
      resetResults();
      return;
    }

    final int requestId = ++_requestId;
    _queryScope.cancel();
    final scope = CoreQueryScope();
    _queryScope = scope;
    _clearTrackAlbums();
    _activeQuery = query;

    _hasSearchRequest = true;
    _isLoadingTracks = true;
    _isLoadingAlbums = true;
    _isLoadingPlaylists = true;
    _isLoadingArtists = true;
    if (!shouldRetainResults) {
      _sourceTracks = const <Track>[];
      _tracks = const <Track>[];
      _albums = const <Album>[];
      _playlists = const <Playlist>[];
      _artists = const <Artist>[];
    }
    _notifyListenersIfAlive();

    await scope.run(() async {
      if (_isDisposed || requestId != _requestId) return;
      if (shouldRetainResults) _synchronizeTrackAlbums();
      await Future.wait<void>(<Future<void>>[
        _searchMedia(query, MediaType.track, requestId),
        _searchMedia(query, MediaType.album, requestId),
        _searchMedia(query, MediaType.playlist, requestId),
        _searchMedia(query, MediaType.artist, requestId),
      ]);
    });
  }

  // Completes each result group independently, including failed requests.
  Future<void> _searchMedia(
    String query,
    MediaType mediaType,
    int requestId,
  ) async {
    try {
      final SearchResults results = await coreBridge.searchMedia(
        query,
        mediaType,
      );
      if (requestId != _requestId || _isDisposed) {
        return;
      }
      _applyResults(mediaType, results);
    } catch (_) {
    } finally {
      if (requestId == _requestId && !_isDisposed) {
        bool wasLoading;
        switch (mediaType) {
          case MediaType.track:
            wasLoading = _isLoadingTracks;
            _isLoadingTracks = false;
          case MediaType.album:
            wasLoading = _isLoadingAlbums;
            _isLoadingAlbums = false;
          case MediaType.playlist:
            wasLoading = _isLoadingPlaylists;
            _isLoadingPlaylists = false;
          case MediaType.artist:
            wasLoading = _isLoadingArtists;
            _isLoadingArtists = false;
          default:
            wasLoading = false;
        }
        if (wasLoading) _notifyListenersIfAlive(mediaType);
      }
    }
  }

  // Catalog events and dependent album reads.
  void _onCatalogUpdate(CatalogUpdate update) {
    if (_isDisposed || _queryScope.isCanceled) return;
    if (update.kind == CatalogQueryKind.artistAlbums) {
      final key = (update.provider ?? '', update.itemId ?? '');
      if (_artistAlbumScopes.containsKey(key)) {
        _acceptArtistAlbums(key, update.albums);
      }
      return;
    }
    if (_isDisposed ||
        _queryScope.isCanceled ||
        update.kind != CatalogQueryKind.search ||
        update.searchQuery != _activeQuery ||
        update.limit != 50) {
      return;
    }
    final results = update.searchResults;
    if (results == null) return;
    final type = update.mediaType;
    if (type != null) _applyResults(type, results);
  }

  void _onCatalogReadChange(CatalogReadChange change) {
    if (!_isDisposed &&
        !_queryScope.isCanceled &&
        change.kind == CatalogQueryKind.artistAlbums &&
        change.phase == CatalogLoadPhase.invalidated &&
        _artistAlbumScopes.containsKey((
          change.provider ?? '',
          change.itemId ?? '',
        ))) {
      _invalidateSearch();
      return;
    }
    if (!_isDisposed &&
        !_queryScope.isCanceled &&
        change.kind == CatalogQueryKind.search &&
        change.searchQuery == _activeQuery &&
        change.limit == 50 &&
        const {
          MediaType.track,
          MediaType.album,
          MediaType.playlist,
          MediaType.artist,
        }.contains(change.mediaType) &&
        change.phase == CatalogLoadPhase.invalidated) {
      _invalidateSearch();
    }
  }

  // Early and terminal results share deduplication and first-loading transitions.
  void _applyResults(MediaType type, SearchResults results) {
    bool shouldNotify;
    switch (type) {
      case MediaType.track:
        final incoming = _withFavoriteStatuses(results.tracks);
        final hasChanges = !hasSameMediaListContent(_sourceTracks, incoming);
        shouldNotify = hasChanges || _isLoadingTracks;
        _isLoadingTracks = false;
        if (hasChanges) {
          _sourceTracks = incoming;
          _tracks = incoming;
          _synchronizeTrackAlbums();
        }
      case MediaType.album:
        final incoming = _withFavoriteStatuses(results.albums);
        final hasChanges = !hasSameMediaListContent(_albums, incoming);
        shouldNotify = hasChanges || _isLoadingAlbums;
        _isLoadingAlbums = false;
        if (hasChanges) _albums = incoming;
      case MediaType.playlist:
        final incoming = _withFavoriteStatuses(results.playlists);
        final hasChanges = !hasSameMediaListContent(_playlists, incoming);
        shouldNotify = hasChanges || _isLoadingPlaylists;
        _isLoadingPlaylists = false;
        if (hasChanges) _playlists = incoming;
      case MediaType.artist:
        final incoming = _withFavoriteStatuses(results.artists);
        final hasChanges = !hasSameMediaListContent(_artists, incoming);
        shouldNotify = hasChanges || _isLoadingArtists;
        _isLoadingArtists = false;
        if (hasChanges) _artists = incoming;
      default:
        return;
    }
    if (shouldNotify) _notifyListenersIfAlive(type);
  }

  void _synchronizeTrackAlbums() {
    final Set<(String, String)> artistKeys = <(String, String)>{};
    for (final Track track in _sourceTracks) {
      if (track.album == null || track.album!.isYearKnown) {
        continue;
      }
      for (final Artist artist in track.artists) {
        if (artist.itemId.isNotEmpty && artist.provider.isNotEmpty) {
          artistKeys.add((artist.provider, artist.itemId));
        }
      }
    }
    for (final key in _artistAlbumScopes.keys.toList()) {
      if (artistKeys.contains(key)) continue;
      _artistAlbumScopes.remove(key)!.cancel();
      _observedArtistAlbums.remove(key);
    }
    _indexesByAlbum.clear();
    _albumCandidates.clear();
    for (var index = 0; index < _sourceTracks.length; index++) {
      final album = _sourceTracks[index].album;
      if (album != null && !album.isYearKnown) {
        _indexesByAlbum
            .putIfAbsent((album.provider, album.itemId), () => [])
            .add(index);
      }
    }
    for (final entry in _observedArtistAlbums.entries) {
      for (final album in entry.value) {
        final identity = (album.provider, album.itemId);
        if (!_indexesByAlbum.containsKey(identity)) continue;
        _albumCandidates.putIfAbsent(identity, () => {})[entry.key] = album;
      }
    }
    _applyTrackAlbums(
      _albumCandidates.keys.toSet(),
      _indexesByAlbum,
      _albumCandidates,
    );
    for (final key in artistKeys) {
      if (_artistAlbumScopes.containsKey(key)) continue;
      final scope = _queryScope.createChild();
      _artistAlbumScopes[key] = scope;
      unawaited(_loadArtistAlbums(key, scope, _requestId));
    }
  }

  Future<void> _loadArtistAlbums(
    (String, String) key,
    CoreQueryScope scope,
    int requestId,
  ) async {
    try {
      final albums = await scope.run(
        () => coreBridge.getArtistAlbums(key.$2, key.$1, isBackground: true),
      );
      if (!_isDisposed &&
          !scope.isCanceled &&
          requestId == _requestId &&
          identical(_artistAlbumScopes[key], scope)) {
        _acceptArtistAlbums(key, albums);
      }
    } catch (_) {}
  }

  void _acceptArtistAlbums((String, String) key, List<Album> albums) {
    if (_observedArtistAlbums[key] case final previous?
        when hasSameMediaListContent(previous, albums)) {
      return;
    }
    final dirty = <(String, String)>{};
    for (final album in _observedArtistAlbums[key] ?? const <Album>[]) {
      final identity = (album.provider, album.itemId);
      _albumCandidates[identity]?.remove(key);
      dirty.add(identity);
    }
    _observedArtistAlbums[key] = albums;
    for (final album in albums) {
      final identity = (album.provider, album.itemId);
      if (!_indexesByAlbum.containsKey(identity)) continue;
      _albumCandidates.putIfAbsent(identity, () => {})[key] = album;
      dirty.add(identity);
    }
    _applyTrackAlbums(dirty, _indexesByAlbum, _albumCandidates);
  }

  // Derived album fields never replace the observed track source.
  void _applyTrackAlbums(
    Set<(String, String)> dirty,
    Map<(String, String), List<int>> indexesByAlbum,
    Map<(String, String), Map<(String, String), Album>> candidates,
  ) {
    List<Track>? enriched;
    for (final identity in dirty) {
      final album = candidates[identity]?.values.lastOrNull;
      for (final index in indexesByAlbum[identity] ?? const <int>[]) {
        final track = _sourceTracks[index];
        final updated = album == null
            ? track
            : Track(
                itemId: track.itemId,
                provider: track.provider,
                name: track.name,
                duration: track.duration,
                artists: track.artists,
                version: track.version,
                sortName: track.sortName,
                uri: track.uri,
                externalIds: track.externalIds,
                isPlayable: track.isPlayable,
                available: track.available,
                album: album,
                image: track.image,
                providerMappings: track.providerMappings,
                metadata: track.metadata,
                favorite: track.favorite,
                position: track.position,
                dateAdded: track.dateAdded,
                discNumber: track.discNumber,
                trackNumber: track.trackNumber,
              );
        if (!hasSameMediaContent(_tracks[index], updated)) {
          enriched ??= _tracks.toList(growable: false);
          enriched[index] = updated;
        }
      }
    }
    if (enriched != null) {
      _tracks = enriched;
      _notifyListenersIfAlive(MediaType.track);
    }
  }

  // Context invalidation and projection helpers.
  void _clearTrackAlbums() {
    for (final scope in _artistAlbumScopes.values) {
      scope.cancel();
    }
    _artistAlbumScopes.clear();
    _observedArtistAlbums.clear();
    _indexesByAlbum.clear();
    _albumCandidates.clear();
  }

  void _clearResults({bool shouldRetainQuery = false}) {
    if (!shouldRetainQuery) _activeQuery = null;
    _queryScope.cancel();
    _clearTrackAlbums();
    _sourceTracks = const <Track>[];
    _requestId++;
    _hasSearchRequest = false;
    _isLoadingTracks = false;
    _isLoadingAlbums = false;
    _isLoadingPlaylists = false;
    _isLoadingArtists = false;
    _tracks = const <Track>[];
    _albums = const <Album>[];
    _playlists = const <Playlist>[];
    _artists = const <Artist>[];
    _notifyListenersIfAlive();
  }

  // Retire observed server data but retain the query for a later explicit reload.
  void _invalidateSearch() => _clearResults(shouldRetainQuery: true);

  void _notifyListenersIfAlive([MediaType? type]) {
    if (_isDisposed) {
      return;
    }
    requestChanges.value = _hasSearchRequest;
    if (type != null) {
      _sectionChanges[type]!.notifyListeners();
    } else {
      for (final notifier in _sectionChanges.values) {
        notifier.notifyListeners();
      }
    }
    notifyListeners();
  }

  List<T> _withFavoriteStatuses<T extends MediaItem>(List<T> items) {
    final UserData? controller = userDataController;
    if (controller == null) {
      return items;
    }
    return items
        .map((T item) => controller.withFavoriteStatus(item))
        .toList(growable: false);
  }
}
