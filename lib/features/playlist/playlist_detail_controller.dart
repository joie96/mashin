import 'dart:async';
import 'dart:collection' as collection;

import 'package:flutter/foundation.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_query_scope.dart';
import '../core/core_userdata.dart';
import '../shared/detail_provider_selection.dart';
import '../shared/item_provider_selection.dart';

/// Loads and holds the data of a playlist detail page (metadata + tracks) and
/// exposes the playback/favorite/edit actions used by the desktop and mobile pages.
class PlaylistDetailController extends ChangeNotifier {
  PlaylistDetailController({
    required this.coreBridge,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
  }) : sectionProvider = DetailProviderSelection(
         itemId: itemId,
         provider: providerInstanceIdOrDomain,
       ) {
    userDataController?.favoritesChanges.addListener(_onFavoritesChanged);
    userDataController?.playlistsChanges.addListener(_onUserDataChanged);
    _catalogSubscription = coreBridge.catalogUpdates.listen(_onCatalogUpdate);
    _cacheSubscription = coreBridge.mediaReadInvalidations.listen(
      _onMediaReadInvalidated,
    );
    _catalogReadSubscription = coreBridge.catalogReadChanges.listen(
      _onCatalogReadChange,
    );
  }

  final MashinCoreBridgeApi coreBridge;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;
  final DetailProviderSelection sectionProvider;
  StreamSubscription<CatalogUpdate>? _catalogSubscription;
  StreamSubscription<void>? _cacheSubscription;
  StreamSubscription<CatalogReadChange>? _catalogReadSubscription;
  final ChangeNotifier _readStatusNotifier = ChangeNotifier();
  CatalogReadChange? _tracksReadState;
  bool _isSelectingProvider = false;

  Playlist? _playlist;
  List<Track> _tracks = const <Track>[];
  List<Track> _sourceTracks = const <Track>[];
  CoreQueryScope? _trackAlbumsScope;
  (int, List<Track>)? _trackAlbumsContext;
  final Map<(String, String), CoreQueryScope> _artistAlbumScopes = {};
  final Map<(String, String), List<Album>> _observedArtistAlbums = {};
  final Set<(String, String)> _pendingArtistAlbums = {};
  final Map<(String, String), List<int>> _trackIndexesByAlbum = {};
  final Map<(String, String), Map<(String, String), Album>> _albumCandidates =
      {};
  final Set<(String, String)> _dirtyAlbumKeys = {};
  Timer? _trackAlbumsPublication;
  final collection.Queue<((String, String), CoreQueryScope)>
  _queuedArtistAlbums = collection.Queue();
  Timer? _artistAlbumsAdmission;

  bool _isLoadingMetadata = true;
  bool _isLoadingTracks = true;
  bool _isLoadingTrackAlbums = false;
  bool _hasLoadedPrivatePlaylist = false;
  bool _isPlaylistDeleted = false;
  bool _isDisposed = false;
  bool _isReadingSnapshot = false;
  String? _observedPrivatePlaylistId;
  int _loadGeneration = 0;
  bool _isPageVisible = true;
  bool _requiresReload = false;
  CoreQueryScope _queryScope = CoreQueryScope();
  Future<void>? _loadingFuture;
  Completer<Playlist?>? _firstMetadata;

  CoreQueryScope get _activeQueryScope => CoreQueryScope.current ?? _queryScope;
  bool get _isLoadCanceled => _isDisposed || _activeQueryScope.isCanceled;

  // Route state survives navigation, but hidden pages do not retain read subscriptions.
  Future<void> setPageVisible(bool isVisible) async {
    if (_isDisposed || _isPageVisible == isVisible) return;
    _isPageVisible = isVisible;
    if (!isVisible) {
      _requiresReload =
          _loadingFuture != null ||
          isLoadingMetadata ||
          isLoadingTracks ||
          _isLoadingTrackAlbums;
      _isLoadingTrackAlbums = false;
      _trackAlbumsPublication?.cancel();
      _artistAlbumsAdmission?.cancel();
      _loadGeneration++;
      _queryScope.cancel();
      _loadingFuture = null;
      return;
    }
    _queryScope = CoreQueryScope();
    if (_requiresReload) {
      _requiresReload = false;
      await loadPlaylist();
    }
  }

  void _onMediaReadInvalidated(void _) {
    if (_isDisposed) return;
    _requiresReload = true;
    _queryScope.cancel();
    _loadGeneration++;
    _loadingFuture = null;
    if (!_isPageVisible) return;
    _queryScope = CoreQueryScope();
    _requiresReload = false;
    unawaited(loadPlaylist());
  }

  @override
  void notifyListeners() {
    if (!_isLoadCanceled) {
      super.notifyListeners();
    }
  }

  @override
  void dispose() {
    _isDisposed = true;
    _trackAlbumsPublication?.cancel();
    _artistAlbumsAdmission?.cancel();
    unawaited(_catalogSubscription?.cancel());
    unawaited(_cacheSubscription?.cancel());
    unawaited(_catalogReadSubscription?.cancel());
    _readStatusNotifier.dispose();
    _queryScope.cancel();
    _loadGeneration++;
    userDataController?.favoritesChanges.removeListener(_onFavoritesChanged);
    userDataController?.playlistsChanges.removeListener(_onUserDataChanged);
    final playlistId = _observedPrivatePlaylistId;
    if (playlistId != null) {
      userDataController
          ?.playlistChanges(playlistId)
          .removeListener(_onUserDataChanged);
    }
    super.dispose();
  }

  Playlist? get playlist => _playlist;
  List<Track> get tracks => _tracks;
  CatalogReadChange? get tracksReadState => _tracksReadState;
  Listenable get readStatusListenable => _readStatusNotifier;

  bool get isLoadingMetadata => _isLoadingMetadata;
  bool get isLoadingTracks => _isLoadingTracks;
  bool get isLoadingSections => _isSelectingProvider || _isLoadingTracks;
  bool get isPlaylistFavorite =>
      _playlist != null && isItemFavorite(_playlist!);
  bool get isPlaylistDeleted => _isPlaylistDeleted;
  bool get isEditable => _playlist?.isEditable ?? false;
  bool get isPrivatePlaylist =>
      _playlist != null &&
      (userDataController?.isPrivatePlaylist(_playlist!) ?? false);

  String get playlistName {
    final String name = _playlist?.name.trim() ?? '';
    return name.isEmpty ? 'Unbekannte Wiedergabeliste' : name;
  }

  String? get ownerName {
    final String owner = _playlist?.owner?.trim() ?? '';
    return owner.isEmpty ? null : owner;
  }

  String get tracksCountText {
    final int count = _tracks.length;
    return count == 1 ? '1 Titel' : '$count Titel';
  }

  String get totalDurationText {
    final int seconds = _tracks.fold<int>(
      0,
      (int sum, Track track) => sum + track.duration,
    );
    if (seconds <= 0) {
      return '0m';
    }

    final int hours = seconds ~/ 3600;
    final int minutes = (seconds % 3600) ~/ 60;
    if (hours > 0) {
      return '${hours}h ${minutes}m';
    }
    return '${minutes}m';
  }

  // Data loading
  void _onCatalogUpdate(CatalogUpdate update) {
    if (update.allowDynamicTracks == true) return;
    if (_isLoadCanceled || !_isPageVisible) return;
    if (update.kind == CatalogQueryKind.artistAlbums) {
      _acceptArtistAlbums((
        update.provider ?? '',
        update.itemId ?? '',
      ), update.albums);
      return;
    }
    if (update.matches(
          CatalogQueryKind.playlist,
          itemId,
          providerInstanceIdOrDomain,
        ) &&
        update.item is Playlist) {
      final playlist = update.item! as Playlist;
      final incoming =
          userDataController?.withFavoriteStatus(playlist) ?? playlist;
      final hasChangedMetadata = !hasSameMediaContent(_playlist, incoming);
      if (hasChangedMetadata) _playlist = incoming;
      final firstMetadata = _firstMetadata;
      if (firstMetadata != null && !firstMetadata.isCompleted) {
        firstMetadata.complete(_playlist);
      }
      final wasLoadingMetadata = _isLoadingMetadata;
      _isLoadingMetadata = false;
      if (!hasChangedMetadata && !wasLoadingMetadata) return;
    } else if (update.matches(
      CatalogQueryKind.playlistTracks,
      sectionProvider.selectedItemId,
      sectionProvider.selectedProvider,
    )) {
      final incoming = List<Track>.unmodifiable(
        update.tracks.map(
          (track) => userDataController?.withFavoriteStatus(track) ?? track,
        ),
      );
      final hasChangedTracks = _applyTracks(incoming);
      if (!hasChangedTracks && !_isLoadingTracks) return;
      _isLoadingTracks = false;
    } else {
      return;
    }
    notifyListeners();
  }

  void _onCatalogReadChange(CatalogReadChange change) {
    if (_isLoadCanceled ||
        !_isPageVisible ||
        change.allowDynamicTracks == true) {
      return;
    }
    if (change.kind == CatalogQueryKind.artistAlbums &&
        _artistAlbumScopes.containsKey((
          change.provider ?? '',
          change.itemId ?? '',
        )) &&
        change.phase == CatalogLoadPhase.invalidated) {
      _invalidateServerContext();
      return;
    }
    final isHeader = change.matches(
      CatalogQueryKind.playlist,
      itemId,
      providerInstanceIdOrDomain,
    );
    if (!isHeader &&
        !change.matches(
          CatalogQueryKind.playlistTracks,
          sectionProvider.selectedItemId,
          sectionProvider.selectedProvider,
        )) {
      return;
    }
    if (!isHeader) _tracksReadState = change;
    _readStatusNotifier.notifyListeners();
    if (change.phase == CatalogLoadPhase.invalidated) {
      _invalidateServerContext();
    }
  }

  // Header, track and enrichment invalidations retire the same visible context.
  void _invalidateServerContext() {
    _loadGeneration++;
    _queryScope.cancel();
    _loadingFuture = null;
    _playlist = null;
    _tracks = const [];
    _sourceTracks = const [];
    _isLoadingMetadata = false;
    _isLoadingTracks = false;
    _isLoadingTrackAlbums = false;
    _requiresReload = true;
    if (_firstMetadata case final pending? when !pending.isCompleted) {
      pending.complete(null);
    }
    super.notifyListeners();
  }

  Future<void> loadPlaylist() {
    if (_isLoadCanceled) return Future<void>.value();
    if (_loadingFuture case final pending?) return pending;
    late final Future<void> loading;
    loading = _queryScope.run(_loadPlaylist).whenComplete(() {
      if (identical(_loadingFuture, loading)) _loadingFuture = null;
    });
    return _loadingFuture = loading;
  }

  Future<void> _loadPlaylist() async {
    if (_isLoadCanceled) return;
    final int loadGeneration = ++_loadGeneration;
    bool hasLoadedPrivateTracks = false;
    _isLoadingMetadata = true;
    _isLoadingTracks = true;
    notifyListeners();

    try {
      final userData = userDataController;
      if (userData != null) {
        final retained = _resolvePlaylistFromUserData(
          itemId,
          providerInstanceIdOrDomain,
        );
        if (retained != null && userData.isPrivatePlaylist(retained)) {
          _loadPrivatePlaylist(
            retained,
            userData.tracksForPlaylist(retained.itemId),
          );
          _isLoadingMetadata = false;
          _isLoadingTracks = userData.isLoadingPlaylistTracks(retained.itemId);
          notifyListeners();
        }
        _isReadingSnapshot = true;
        try {
          await _activeQueryScope.execute(
            userData.refreshLibrarySnapshot,
            onCancel: () {},
          );
        } catch (_) {
          if (!_isDisposed &&
              loadGeneration == _loadGeneration &&
              !_hasLoadedPrivatePlaylist) {
            _playlist = null;
            _tracks = const <Track>[];
          }
          return;
        } finally {
          _isReadingSnapshot = false;
        }
        if (_isDisposed || loadGeneration != _loadGeneration) return;
      }
      final Playlist? local = _resolvePlaylistFromUserData(
        itemId,
        providerInstanceIdOrDomain,
      );

      if (local != null) {
        final bool isUserPlaylist =
            userDataController?.isPrivatePlaylist(local) ?? false;
        final List<Track> storedTracks =
            userDataController
                ?.tracksForPlaylist(local.itemId)
                .toList(growable: false) ??
            const <Track>[];

        // Private MA playlists use the Rust-owned snapshot, refreshed by MA events.
        if (isUserPlaylist || storedTracks.isNotEmpty) {
          _loadPrivatePlaylist(local, storedTracks);
          _isLoadingMetadata = false;
          notifyListeners();
          if (isUserPlaylist) {
            try {
              final metadata = coreBridge.getPlaylist(local.itemId, 'library');
              final metadataResult = metadata.then<Playlist?>(
                (value) => value,
                onError: (Object _) => null,
              );
              final tracks = await _activeQueryScope.execute(
                () => coreBridge.getPlaylistTracks(
                  local.itemId,
                  'library',
                  forceRefresh: false,
                ),
                onCancel: () {},
              );
              final refreshedPlaylist = await metadataResult;
              if (!_isLoadCanceled && loadGeneration == _loadGeneration) {
                _loadPrivatePlaylist(refreshedPlaylist ?? local, tracks);
                hasLoadedPrivateTracks = true;
              }
            } on CoreQueryCanceled {
              return;
            } catch (_) {
              return;
            }
          }
          if (!isUserPlaylist) {
            await sectionProvider.initialize(local);
            if (sectionProvider.mappings.isNotEmpty && !_isDisposed) {
              await _loadTracks(loadGeneration);
            }
          }
          return;
        }
      }

      final firstMetadata = Completer<Playlist?>();
      _firstMetadata = firstMetadata;
      if (_playlist != null) firstMetadata.complete(_playlist);
      final metadataLoading = _loadMetadata();
      try {
        final originalPlaylist = await Future.any<Playlist?>([
          firstMetadata.future,
          metadataLoading.then((_) => _playlist),
        ]);
        if (_isLoadCanceled || loadGeneration != _loadGeneration) return;
        if (originalPlaylist != null) {
          await sectionProvider.initialize(originalPlaylist);
        }
        if (_isLoadCanceled || loadGeneration != _loadGeneration) return;
        await _loadTracks(loadGeneration);
        await metadataLoading;
        if (_isLoadCanceled || loadGeneration != _loadGeneration) return;
        final refreshedPlaylist = _playlist;
        if (refreshedPlaylist != null &&
            !identical(originalPlaylist, refreshedPlaylist)) {
          final previousContext = (
            sectionProvider.selectedItemId,
            sectionProvider.selectedProvider,
          );
          await sectionProvider.initialize(refreshedPlaylist);
          if (_isLoadCanceled || loadGeneration != _loadGeneration) return;
          if (previousContext !=
              (
                sectionProvider.selectedItemId,
                sectionProvider.selectedProvider,
              )) {
            await _loadTracks(loadGeneration);
          }
        }
      } finally {
        if (identical(_firstMetadata, firstMetadata)) _firstMetadata = null;
      }
    } finally {
      if (!_isDisposed && loadGeneration == _loadGeneration) {
        _isLoadingMetadata = false;
        _isLoadingTracks =
            !hasLoadedPrivateTracks &&
            _playlist != null &&
            (userDataController?.isLoadingPlaylistTracks(_playlist!.itemId) ??
                false);
        notifyListeners();
      }
    }
  }

  // The library playlist stays authoritative for metadata and editing.
  Future<void> selectProvider(ProviderMapping mapping) async {
    if (_isDisposed || isPrivatePlaylist || isLoadingSections) return;
    _isSelectingProvider = true;
    notifyListeners();
    try {
      await sectionProvider.select(mapping);
      if (_isDisposed) return;
      final int loadGeneration = ++_loadGeneration;
      _tracks = const <Track>[];
      _sourceTracks = const <Track>[];
      _isLoadingTracks = true;
      notifyListeners();
      await _loadTracks(loadGeneration);
    } finally {
      _isSelectingProvider = false;
      _isLoadingTracks = false;
      notifyListeners();
    }
  }

  void _onUserDataChanged() {
    if (_isDisposed ||
        _isPlaylistDeleted ||
        _isReadingSnapshot ||
        _loadGeneration == 0) {
      return;
    }
    final local = _resolvePlaylistFromUserData(
      itemId,
      providerInstanceIdOrDomain,
    );
    if (local == null && _hasLoadedPrivatePlaylist) {
      _loadGeneration++;
      _isPlaylistDeleted = true;
      _playlist = null;
      _tracks = const <Track>[];
      _isLoadingMetadata = false;
      _isLoadingTracks = false;
      notifyListeners();
      return;
    }
    if (local == null ||
        !(userDataController?.isPrivatePlaylist(local) ?? false)) {
      return;
    }
    _loadGeneration++;
    _loadPrivatePlaylist(
      local,
      userDataController!.tracksForPlaylist(local.itemId),
    );
    _isLoadingMetadata = false;
    _isLoadingTracks = userDataController!.isLoadingPlaylistTracks(
      local.itemId,
    );
    notifyListeners();
  }

  void _loadPrivatePlaylist(Playlist playlist, List<Track> storedTracks) {
    if (_observedPrivatePlaylistId != playlist.itemId) {
      final previousId = _observedPrivatePlaylistId;
      if (previousId != null) {
        userDataController
            ?.playlistChanges(previousId)
            .removeListener(_onUserDataChanged);
      }
      _observedPrivatePlaylistId = playlist.itemId;
      userDataController
          ?.playlistChanges(playlist.itemId)
          .addListener(_onUserDataChanged);
    }
    _hasLoadedPrivatePlaylist = true;
    _playlist = userDataController?.withFavoriteStatus(playlist) ?? playlist;

    _applyTracks(
      List<Track>.unmodifiable(
        storedTracks
            .map((Track track) {
              final Track withFavorite =
                  userDataController?.withFavoriteStatus(track) ?? track;
              return withFavorite;
            })
            .toList(growable: false),
      ),
    );
  }

  Future<void> _loadMetadata() async {
    try {
      final Playlist online = await _activeQueryScope.execute(
        () => coreBridge.getPlaylist(itemId, providerInstanceIdOrDomain),
        onCancel: () {},
      );
      if (_isLoadCanceled) return;
      final incoming = userDataController?.withFavoriteStatus(online) ?? online;
      if (!hasSameMediaContent(_playlist, incoming)) _playlist = incoming;
    } catch (_) {
    } finally {
      if (!_isLoadCanceled) {
        _isLoadingMetadata = false;
        notifyListeners();
      }
    }
  }

  Future<void> _loadTracks(int loadGeneration) async {
    try {
      final trackRead = _activeQueryScope.execute(
        () => coreBridge.getPlaylistTracks(
          sectionProvider.selectedItemId,
          sectionProvider.selectedProvider,
          forceRefresh: false,
        ),
        onCancel: () {},
      );
      unawaited(_loadTrackAlbums(loadGeneration, _sourceTracks));
      final List<Track> loaded = await trackRead;
      if (_isDisposed || loadGeneration != _loadGeneration) {
        return;
      }

      final incoming = List<Track>.unmodifiable(
        loaded
            .map((Track track) {
              final Track withFavorite =
                  userDataController?.withFavoriteStatus(track) ?? track;
              return withFavorite;
            })
            .toList(growable: false),
      );
      _applyTracks(incoming);
    } catch (_) {}
  }

  bool _applyTracks(List<Track> incoming) {
    final hasChanges = !hasSameMediaListContent(_sourceTracks, incoming);
    if (hasChanges) {
      _sourceTracks = incoming;
      _tracks = incoming;
    }
    unawaited(_loadTrackAlbums(_loadGeneration, _sourceTracks));
    return hasChanges;
  }

  // Enriches online tracks without delaying the initial playlist display.
  Future<void> _loadTrackAlbums(
    int loadGeneration,
    List<Track> initialTracks,
  ) async {
    final context = (loadGeneration, initialTracks);
    if (context == _trackAlbumsContext || _isLoadCanceled || !_isPageVisible) {
      return;
    }
    if (_trackAlbumsContext?.$1 != loadGeneration ||
        (_trackAlbumsScope?.isCanceled ?? true)) {
      _trackAlbumsScope?.cancel();
      _trackAlbumsPublication?.cancel();
      _artistAlbumsAdmission?.cancel();
      _queuedArtistAlbums.clear();
      _artistAlbumScopes.clear();
      _observedArtistAlbums.clear();
      _pendingArtistAlbums.clear();
      _albumCandidates.clear();
      _dirtyAlbumKeys.clear();
      _trackAlbumsScope = _activeQueryScope.createChild();
    }
    _trackAlbumsContext = context;
    _trackIndexesByAlbum.clear();
    for (var index = 0; index < initialTracks.length; index++) {
      final album = initialTracks[index].album;
      if (album != null && !album.isYearKnown) {
        _trackIndexesByAlbum
            .putIfAbsent((album.provider, album.itemId), () => [])
            .add(index);
      }
    }
    final Set<(String, String)> artistKeys = <(String, String)>{};
    for (final Track track in initialTracks) {
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
      _pendingArtistAlbums.remove(key);
    }
    _albumCandidates.clear();
    _dirtyAlbumKeys.addAll(_trackIndexesByAlbum.keys);
    for (final entry in _observedArtistAlbums.entries) {
      _indexArtistAlbums(entry.key, entry.value);
    }
    _applyTrackAlbums();
    for (final key in artistKeys) {
      if (_artistAlbumScopes.containsKey(key)) continue;
      final scope = _trackAlbumsScope!.createChild();
      _artistAlbumScopes[key] = scope;
      _pendingArtistAlbums.add(key);
      _queuedArtistAlbums.add((key, scope));
    }
    _isLoadingTrackAlbums = _pendingArtistAlbums.isNotEmpty;
    if (_queuedArtistAlbums.isNotEmpty &&
        !(_artistAlbumsAdmission?.isActive ?? false)) {
      _artistAlbumsAdmission = Timer(Duration.zero, _startArtistAlbumReads);
    }
  }

  // Yield between admission batches without waiting for any network refresh to finish.
  void _startArtistAlbumReads() {
    if (_isLoadCanceled || !_isPageVisible) return;
    for (
      var admitted = 0;
      admitted < 8 && _queuedArtistAlbums.isNotEmpty;
      admitted++
    ) {
      final (key, scope) = _queuedArtistAlbums.removeFirst();
      if (scope.isCanceled || !identical(_artistAlbumScopes[key], scope)) {
        continue;
      }
      unawaited(_observeArtistAlbums(key, scope));
    }
    if (_queuedArtistAlbums.isNotEmpty) {
      _artistAlbumsAdmission = Timer(Duration.zero, _startArtistAlbumReads);
    }
  }

  Future<void> _observeArtistAlbums(
    (String, String) key,
    CoreQueryScope scope,
  ) async {
    try {
      final albums = await scope.run(
        () => scope.execute(
          () => coreBridge.getArtistAlbums(key.$2, key.$1, isBackground: true),
          onCancel: () {},
        ),
      );
      if (identical(_artistAlbumScopes[key], scope) && !scope.isCanceled) {
        _acceptArtistAlbums(key, albums);
      }
    } catch (_) {
    } finally {
      if (identical(_artistAlbumScopes[key], scope)) {
        _pendingArtistAlbums.remove(key);
        _isLoadingTrackAlbums =
            !_isDisposed && _isPageVisible && _pendingArtistAlbums.isNotEmpty;
      }
    }
  }

  void _acceptArtistAlbums((String, String) key, List<Album> albums) {
    final scope = _artistAlbumScopes[key];
    if (_isDisposed ||
        !_isPageVisible ||
        scope == null ||
        scope.isCanceled ||
        _trackAlbumsContext?.$1 != _loadGeneration) {
      return;
    }
    if (_observedArtistAlbums[key] case final previous?
        when hasSameMediaListContent(previous, albums)) {
      return;
    }
    for (final album in _observedArtistAlbums[key] ?? const <Album>[]) {
      final albumKey = (album.provider, album.itemId);
      _albumCandidates[albumKey]?.remove(key);
      if (_trackIndexesByAlbum.containsKey(albumKey)) {
        _dirtyAlbumKeys.add(albumKey);
      }
    }
    _observedArtistAlbums[key] = albums;
    _indexArtistAlbums(key, albums);
    if (_trackAlbumsPublication?.isActive ?? false) return;
    _trackAlbumsPublication = Timer(Duration.zero, () {
      if (_isDisposed ||
          !_isPageVisible ||
          (_trackAlbumsScope?.isCanceled ?? true)) {
        return;
      }
      _applyTrackAlbums();
    });
  }

  void _indexArtistAlbums((String, String) artistKey, List<Album> albums) {
    for (final album in albums) {
      final albumKey = (album.provider, album.itemId);
      if (!_trackIndexesByAlbum.containsKey(albumKey)) continue;
      _albumCandidates.putIfAbsent(albumKey, () => {})[artistKey] = album;
      _dirtyAlbumKeys.add(albumKey);
    }
  }

  // Only affected occurrences are reprojected; observed source years remain authoritative.
  void _applyTrackAlbums() {
    List<Track>? enriched;
    for (final albumKey in _dirtyAlbumKeys) {
      final album = _albumCandidates[albumKey]?.values.firstOrNull;
      for (final index in _trackIndexesByAlbum[albumKey] ?? const <int>[]) {
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
        if (hasSameMediaContent(_tracks[index], updated)) continue;
        enriched ??= List<Track>.of(_tracks);
        enriched[index] = updated;
      }
    }
    _dirtyAlbumKeys.clear();
    if (enriched != null) {
      _tracks = List<Track>.unmodifiable(enriched);
      notifyListeners();
    }
  }

  // Playback
  Future<void> playPlaylist() async {
    if (_tracks.isEmpty) {
      return;
    }
    await coreBridge.queuePlayMedia(_tracks);
  }

  Future<void> shufflePlaylist() async {
    if (_tracks.isEmpty) {
      return;
    }
    await coreBridge.queuePlayMedia(_tracks, shuffle: true);
  }

  Future<void> startTrackRadio(Track track) async {
    await coreBridge.startRadio(<MediaItem>[track]);
  }

  Future<void> startItemRadio(MediaItem item) async {
    await coreBridge.startRadio(<MediaItem>[item]);
  }

  // Favorites
  void _onFavoritesChanged() {
    if (_isDisposed) return;
    notifyListeners();
  }

  bool isItemFavorite(MediaItem item) =>
      userDataController?.isFavorite(item) ?? item.favorite;

  Future<void> toggleItemFavorite(MediaItem item) async {
    final bool next = !isItemFavorite(item);
    try {
      await userDataController?.setFavorite(<MediaItem>[item], next);
    } catch (_) {}
  }

  Future<void> togglePlaylistFavorite() async {
    final Playlist? current = _playlist;
    if (current == null) {
      return;
    }
    await toggleItemFavorite(current);
  }

  // Editing
  Future<void> removeTrack(Track track) async {
    final Playlist? current = _playlist;
    final int position = _tracks.indexOf(track);
    if (current == null || !current.isEditable || position < 0) {
      return;
    }

    final List<Track> previous = _tracks;
    _tracks = List<Track>.unmodifiable(
      List<Track>.of(_tracks)..removeAt(position),
    );
    notifyListeners();

    try {
      final bool isUserPlaylist =
          userDataController?.isPrivatePlaylist(current) ?? false;

      if (isUserPlaylist && userDataController != null) {
        await userDataController!.removePlaylistTracks(current.itemId, <Track>[
          track,
        ]);
      } else {
        // The API positions are 1-based.
        await coreBridge.removePlaylistTracks(current.itemId, <int>[
          position + 1,
        ]);
      }
    } catch (_) {
      _tracks = previous;
      notifyListeners();
    }
  }

  Future<void> addTracksToPlaylist(
    String playlistId,
    List<MediaItem> items,
  ) async {
    if (playlistId.trim().isEmpty) return;
    final List<String> uris = <String>[];
    for (final item in items) {
      final selection = ItemProviderSelection.forItem(item);
      await selection.initialize();
      final uri = selection.selectedUri;
      if (uri != null && uri.isNotEmpty) uris.add(uri);
    }
    if (uris.isEmpty) return;

    final Playlist? current = _playlist;
    final bool isUserPlaylist =
        current != null &&
        (userDataController?.isPrivatePlaylist(current) ?? false);

    if (isUserPlaylist && userDataController != null) {
      final List<Track> tracks = items.whereType<Track>().toList(
        growable: false,
      );
      if (tracks.isEmpty) {
        return;
      }

      await userDataController!.addPlaylistTracks(playlistId, tracks);
      return;
    }

    await coreBridge.addPlaylistTracks(playlistId, uris);
  }

  Playlist? _resolvePlaylistFromUserData(
    String targetItemId,
    String targetProvider,
  ) {
    final List<Playlist> playlists =
        userDataController?.playlists ?? const <Playlist>[];
    if (playlists.isEmpty) {
      return null;
    }

    final String normalizedItemId = targetItemId.trim().toLowerCase();
    final String normalizedProvider = targetProvider.trim().toLowerCase();

    for (final Playlist playlist in playlists) {
      if (playlist.itemId.trim().toLowerCase() == normalizedItemId &&
          playlist.provider.trim().toLowerCase() == normalizedProvider) {
        return playlist;
      }
    }

    for (final Playlist playlist in playlists) {
      if (playlist.itemId.trim().toLowerCase() == normalizedItemId) {
        return playlist;
      }
    }

    return null;
  }
}
