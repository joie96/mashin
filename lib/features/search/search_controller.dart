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
  CoreQueryScope? _trackAlbumsScope;
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

  // Lifecycle.
  @override
  void dispose() {
    _isDisposed = true;
    _queryScope.cancel();
    unawaited(_catalogSubscription?.cancel());
    unawaited(_readSubscription?.cancel());
    unawaited(_cacheSubscription?.cancel());
    _requestId++;
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
    _trackAlbumsScope = null;
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
      if (shouldRetainResults) unawaited(_loadTrackAlbums(requestId));
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
        if (wasLoading) _notifyListenersIfAlive();
      }
    }
  }

  // Catalog events and dependent album reads.
  void _onCatalogUpdate(CatalogUpdate update) {
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
          _trackAlbumsScope?.cancel();
          _trackAlbumsScope = null;
        }
        if (_trackAlbumsScope == null) {
          unawaited(_loadTrackAlbums(_requestId));
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
    if (shouldNotify) _notifyListenersIfAlive();
  }

  // Each observed track revision owns its album reads; the scheduler limits I/O.
  Future<void> _loadTrackAlbums(int requestId) async {
    final scope = _queryScope.createChild();
    _trackAlbumsScope = scope;
    bool isCurrent() =>
        !_isDisposed && !scope.isCanceled && requestId == _requestId;
    final Set<(String, String)> artistKeys = <(String, String)>{};
    for (final Track track in _sourceTracks) {
      if (track.album == null || track.album!.year != null) {
        continue;
      }
      for (final Artist artist in track.artists) {
        if (artist.itemId.isNotEmpty && artist.provider.isNotEmpty) {
          artistKeys.add((artist.provider, artist.itemId));
        }
      }
    }
    if (artistKeys.isEmpty) {
      scope.cancel();
      return;
    }

    final observed = <(String, String), List<Album>>{};
    void accept((String, String) key, List<Album> albums) {
      if (!isCurrent()) return;
      if (observed[key] case final previous?
          when hasSameMediaListContent(previous, albums)) {
        return;
      }
      observed[key] = albums;
      _applyTrackAlbums(observed.values.expand((albums) => albums));
    }

    final subscription = coreBridge.catalogUpdates.listen((update) {
      final key = (update.provider ?? '', update.itemId ?? '');
      if (update.kind == CatalogQueryKind.artistAlbums &&
          artistKeys.contains(key)) {
        accept(key, update.albums);
      }
    });
    final readSubscription = coreBridge.catalogReadChanges.listen((change) {
      if (isCurrent() &&
          change.kind == CatalogQueryKind.artistAlbums &&
          artistKeys.contains((change.provider ?? '', change.itemId ?? '')) &&
          change.phase == CatalogLoadPhase.invalidated) {
        _invalidateSearch();
      }
    });
    try {
      await scope.run(
        () => Future.wait(
          artistKeys.map((key) async {
            try {
              accept(key, await coreBridge.getArtistAlbums(key.$2, key.$1));
            } catch (_) {}
          }),
        ),
      );
    } finally {
      await subscription.cancel();
      await readSubscription.cancel();
      scope.cancel();
    }
  }

  // Derived album fields never replace the observed track source.
  void _applyTrackAlbums(Iterable<Album> albums) {
    final Map<(String, String), Album> albumsByKey =
        <(String, String), Album>{};
    for (final album in albums) {
      albumsByKey[(album.provider, album.itemId)] = album;
    }
    final List<Track> enriched = _sourceTracks
        .map((Track track) {
          final Album? reference = track.album;
          if (reference == null || reference.year != null) {
            return track;
          }
          final Album? album =
              albumsByKey[(reference.provider, reference.itemId)];
          if (album == null) {
            return track;
          }
          return Track(
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
        })
        .toList(growable: false);
    if (!hasSameMediaListContent(_tracks, enriched)) {
      _tracks = enriched;
      _notifyListenersIfAlive();
    }
  }

  // Context invalidation and projection helpers.
  void _clearResults({bool shouldRetainQuery = false}) {
    if (!shouldRetainQuery) _activeQuery = null;
    _queryScope.cancel();
    _trackAlbumsScope = null;
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

  void _notifyListenersIfAlive() {
    if (_isDisposed) {
      return;
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
