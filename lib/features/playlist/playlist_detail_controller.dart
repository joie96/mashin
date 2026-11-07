import 'dart:async';

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
    userDataController?.addListener(_onUserDataChanged);
    _catalogSubscription = coreBridge.catalogUpdates.listen(_onCatalogUpdate);
  }

  final MashinCoreBridgeApi coreBridge;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;
  final DetailProviderSelection sectionProvider;
  StreamSubscription<CatalogUpdate>? _catalogSubscription;
  bool _isSelectingProvider = false;

  Playlist? _playlist;
  List<Track> _tracks = const <Track>[];
  final Map<String, bool> _favoriteOverrides = <String, bool>{};

  bool _isLoadingMetadata = true;
  bool _isLoadingTracks = true;
  bool _isLoadingTrackAlbums = false;
  bool _isPlaylistFavorite = false;
  bool _hasLoadedPrivatePlaylist = false;
  bool _isPlaylistDeleted = false;
  bool _isDisposed = false;
  bool _isReadingSnapshot = false;
  int _loadGeneration = 0;
  bool _isPageVisible = true;
  bool _requiresReload = false;
  CoreQueryScope _queryScope = CoreQueryScope();
  Future<void>? _loadingFuture;

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

  @override
  void notifyListeners() {
    if (!_isLoadCanceled) {
      super.notifyListeners();
    }
  }

  @override
  void dispose() {
    _isDisposed = true;
    unawaited(_catalogSubscription?.cancel());
    _queryScope.cancel();
    _loadGeneration++;
    userDataController?.removeListener(_onUserDataChanged);
    super.dispose();
  }

  Playlist? get playlist => _playlist;
  List<Track> get tracks => _tracks;

  bool get isLoadingMetadata => _isLoadingMetadata;
  bool get isLoadingTracks => _isLoadingTracks;
  bool get isLoadingSections => _isSelectingProvider || _isLoadingTracks;
  bool get isPlaylistFavorite => _isPlaylistFavorite;
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
    if (update.matches(
          CatalogQueryKind.playlist,
          itemId,
          providerInstanceIdOrDomain,
        ) &&
        update.item is Playlist) {
      final playlist = update.item! as Playlist;
      _playlist = userDataController?.withFavoriteStatus(playlist) ?? playlist;
      _isPlaylistFavorite = isItemFavorite(_playlist!);
    } else if (update.matches(
      CatalogQueryKind.playlistTracks,
      sectionProvider.selectedItemId,
      sectionProvider.selectedProvider,
    )) {
      _tracks = update.tracks
          .map(
            (track) => userDataController?.withFavoriteStatus(track) ?? track,
          )
          .toList(growable: false);
    } else {
      return;
    }
    notifyListeners();
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
        _isReadingSnapshot = true;
        try {
          await _activeQueryScope.execute(
            userData.refreshLibrarySnapshot,
            onCancel: () {},
          );
        } catch (_) {
          if (!_isDisposed && loadGeneration == _loadGeneration) {
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

      await _loadMetadata();
      final Playlist? originalPlaylist = _playlist;
      if (!_isDisposed && originalPlaylist != null) {
        await sectionProvider.initialize(originalPlaylist);
      }
      if (!_isDisposed && loadGeneration == _loadGeneration) {
        await _loadTracks(loadGeneration);
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
      _favoriteOverrides.clear();
      _isPlaylistFavorite = false;
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
    _hasLoadedPrivatePlaylist = true;
    _playlist = userDataController?.withFavoriteStatus(playlist) ?? playlist;

    _tracks = List<Track>.unmodifiable(
      storedTracks
          .map((Track track) {
            final Track withFavorite =
                userDataController?.withFavoriteStatus(track) ?? track;
            _favoriteOverrides[_favoriteKey(track)] = withFavorite.favorite;
            return withFavorite;
          })
          .toList(growable: false),
    );

    _isPlaylistFavorite = _playlist?.favorite ?? false;
  }

  Future<void> _loadMetadata() async {
    try {
      final Playlist online = await _activeQueryScope.execute(
        () => coreBridge.getPlaylist(itemId, providerInstanceIdOrDomain),
        onCancel: () {},
      );
      if (_isLoadCanceled) return;
      _playlist = userDataController?.withFavoriteStatus(online) ?? online;
      _isPlaylistFavorite = _playlist?.favorite ?? false;
    } catch (_) {
      if (!_isLoadCanceled) _playlist = null;
    } finally {
      if (!_isLoadCanceled) {
        _isLoadingMetadata = false;
        notifyListeners();
      }
    }
  }

  Future<void> _loadTracks(int loadGeneration) async {
    try {
      final List<Track> loaded = await _activeQueryScope.execute(
        () => coreBridge.getPlaylistTracks(
          sectionProvider.selectedItemId,
          sectionProvider.selectedProvider,
          forceRefresh: false,
        ),
        onCancel: () {},
      );
      if (_isDisposed || loadGeneration != _loadGeneration) {
        return;
      }

      _tracks = List<Track>.unmodifiable(
        loaded
            .map((Track track) {
              final Track withFavorite =
                  userDataController?.withFavoriteStatus(track) ?? track;
              _favoriteOverrides[_favoriteKey(track)] = withFavorite.favorite;
              return withFavorite;
            })
            .toList(growable: false),
      );
      unawaited(_loadTrackAlbums(loadGeneration, _tracks));
    } catch (_) {
      if (!_isDisposed && loadGeneration == _loadGeneration) {
        _tracks = const <Track>[];
      }
    }
  }

  // Enriches online tracks without delaying the initial playlist display.
  Future<void> _loadTrackAlbums(
    int loadGeneration,
    List<Track> initialTracks,
  ) async {
    final Set<(String, String)> artistKeys = <(String, String)>{};
    for (final Track track in initialTracks) {
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
      return;
    }

    final pendingArtists = artistKeys.iterator;
    final queryScope = _activeQueryScope;
    Future<List<Album>> loadArtistAlbums() async {
      final albums = <Album>[];
      while (!_isDisposed &&
          loadGeneration == _loadGeneration &&
          !queryScope.isCanceled &&
          pendingArtists.moveNext()) {
        final artistKey = pendingArtists.current;
        try {
          albums.addAll(
            await queryScope.execute(
              () => coreBridge.getArtistAlbums(artistKey.$2, artistKey.$1),
              onCancel: () {},
            ),
          );
        } catch (_) {}
      }
      return albums;
    }

    _isLoadingTrackAlbums = true;
    final List<List<Album>> albumGroups =
        await Future.wait(
          List.generate(3, (_) => loadArtistAlbums()),
        ).whenComplete(() {
          if (!_isDisposed && loadGeneration == _loadGeneration) {
            _isLoadingTrackAlbums = false;
          }
        });
    if (_isDisposed || loadGeneration != _loadGeneration) {
      return;
    }

    final Map<(String, String), Album> albumsByKey =
        <(String, String), Album>{};
    for (final List<Album> albums in albumGroups) {
      for (final Album album in albums) {
        albumsByKey[(
              album.provider.toLowerCase(),
              album.itemId.toLowerCase(),
            )] =
            album;
      }
    }
    bool hasChanges = false;
    final List<Track> enriched = _tracks
        .map((Track track) {
          final Album? reference = track.album;
          if (reference == null || reference.year != null) {
            return track;
          }
          final Album? album =
              albumsByKey[(
                reference.provider.toLowerCase(),
                reference.itemId.toLowerCase(),
              )];
          if (album == null) {
            return track;
          }
          hasChanges = true;
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
    if (hasChanges) {
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
  bool isItemFavorite(MediaItem item) =>
      _favoriteOverrides[_favoriteKey(item)] ?? item.favorite;

  Future<void> toggleItemFavorite(MediaItem item) async {
    final String key = _favoriteKey(item);
    final bool next = !isItemFavorite(item);
    _favoriteOverrides[key] = next;
    notifyListeners();
    try {
      final UserData? controller = userDataController;
      if (controller == null) {
        _favoriteOverrides.remove(key);
        notifyListeners();
        return;
      }
      await controller.setFavorite(<MediaItem>[item], next);
    } catch (_) {
      _favoriteOverrides[key] = !next;
      notifyListeners();
    }
  }

  Future<void> togglePlaylistFavorite() async {
    final Playlist? current = _playlist;
    if (current == null) {
      return;
    }
    final bool next = !_isPlaylistFavorite;
    _isPlaylistFavorite = next;
    notifyListeners();
    try {
      final UserData? controller = userDataController;
      if (controller == null) {
        _isPlaylistFavorite = !next;
        notifyListeners();
        return;
      }
      await controller.setFavorite(<MediaItem>[current], next);
    } catch (_) {
      _isPlaylistFavorite = !next;
      notifyListeners();
    }
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

  String _favoriteKey(MediaItem item) =>
      '${item.mediaType.name}|${item.provider}|${item.itemId}';
}
