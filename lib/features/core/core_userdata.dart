import 'dart:async';

import 'package:flutter/foundation.dart';

import '../shared/item_provider_selection.dart';
import 'core_bridge.dart';
import 'core_models.dart';

/// Projects the Rust-owned MA cache for Flutter views.
class UserData extends ChangeNotifier {
  UserData({required MashinCoreBridgeApi coreBridge})
    : _coreBridge = coreBridge {
    _eventSubscription = coreBridge.events.listen(_onCoreEvent);
  }

  final MashinCoreBridgeApi _coreBridge;
  late final StreamSubscription<Map<String, dynamic>> _eventSubscription;
  Future<void>? _snapshotRefresh;
  bool _needsSnapshotRefresh = false;
  bool _isLoadingLibraryData = false;
  bool _isLoadingFavorites = false;
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
  List<Track> get favoriteTracks => _favoriteTracks;
  List<Album> get favoriteAlbums => _favoriteAlbums;
  List<Playlist> get favoritePlaylists => _favoritePlaylists;
  List<Artist> get favoriteArtists => _favoriteArtists;
  List<Playlist> get playlists => _playlists;

  bool isPrivatePlaylist(Playlist playlist) => _playlists.any(
    (candidate) =>
        candidate.itemId == playlist.itemId &&
        candidate.provider == playlist.provider,
  );

  List<Track> tracksForPlaylist(String playlistId) =>
      _playlistTracksByPlaylistId[playlistId.trim()] ?? const <Track>[];

  Future<void> loadLibraryData() async {
    _isLoadingLibraryData = true;
    _isLoadingFavorites = true;
    _isLoadingPlaylists = true;
    notifyListeners();
    try {
      if (_isDisposed) return;
      final snapshot = await _coreBridge.loadUserData();
      if (!_isDisposed) _applySnapshot(snapshot);
    } catch (_) {
      _loadingPlaylistIds = <String>{};
      rethrow;
    } finally {
      _isLoadingLibraryData = false;
      _isLoadingFavorites = false;
      _isLoadingPlaylists = false;
      if (!_isDisposed) notifyListeners();
    }
  }

  // Reads the current Rust snapshot without restarting its session-owned loading.
  Future<void> refreshLibrarySnapshot() => _refreshSnapshot();

  bool isFavorite(MediaItem item) =>
      _favoriteUriKeys.contains((item.mediaType, item.uri ?? '')) ||
      _itemIdentityKeys(item).any(_favoriteIdentityKeys.contains);

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
    await _coreBridge.setUserDataFavorites(
      items.toList(growable: false),
      favorite,
    );
    await _refreshSnapshot();
  }

  Future<Playlist?> createPrivatePlaylist(String name) async {
    final String normalizedName = name.trim();
    if (normalizedName.isEmpty) return null;
    await _coreBridge.createPrivatePlaylist(normalizedName);
    await _refreshSnapshot();
    return _playlists.lastWhere(
      (playlist) => playlist.name == normalizedName,
      orElse: () =>
          throw StateError('Rust did not return the created private playlist'),
    );
  }

  Future<bool> renamePlaylist(Playlist playlist, String name) async {
    if (playlist.itemId.trim().isEmpty || name.trim().isEmpty) return false;
    await _coreBridge.renamePrivatePlaylist(playlist.itemId, name.trim());
    await _refreshSnapshot();
    return true;
  }

  Future<bool> refreshPrivatePlaylistCover(Playlist playlist) async {
    if (!isPrivatePlaylist(playlist) || !playlist.isEditable) return false;
    await _coreBridge.refreshPrivatePlaylistCover(playlist.itemId);
    await _refreshSnapshot();
    return true;
  }

  Future<bool> removePlaylist(Playlist playlist) async {
    if (playlist.itemId.trim().isEmpty) return false;
    await _coreBridge.removePrivatePlaylist(playlist.itemId);
    await _refreshSnapshot();
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
    await _coreBridge.addPrivatePlaylistTracks(playlistId, values);
    await _refreshSnapshot();
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
    await _coreBridge.removePrivatePlaylistTracks(playlistId, uris);
    await _refreshSnapshot();
    return true;
  }

  // Snapshot refresh and event processing.
  Future<void> _onCoreEvent(Map<String, dynamic> event) async {
    if (_isDisposed) return;
    try {
      if (event['type'] == 'user_data_changed') {
        await _refreshSnapshot();
      }
    } catch (error, stackTrace) {
      FlutterError.reportError(
        FlutterErrorDetails(exception: error, stack: stackTrace),
      );
    }
  }

  Future<void> _refreshSnapshot() {
    _needsSnapshotRefresh = true;
    return _snapshotRefresh ??= Future<void>.microtask(_drainSnapshotRefreshes);
  }

  Future<void> _drainSnapshotRefreshes() async {
    try {
      while (_needsSnapshotRefresh && !_isDisposed) {
        _needsSnapshotRefresh = false;
        final snapshot = await _coreBridge.getUserData();
        if (!_isDisposed && !_needsSnapshotRefresh) _applySnapshot(snapshot);
      }
    } finally {
      _snapshotRefresh = null;
    }
  }

  void _applySnapshot(Map<String, dynamic> snapshot) {
    _isLoadingFavorites = snapshot['is_loading_favorites'] == true;
    _isLoadingPlaylists = snapshot['is_loading_playlists'] == true;
    final Map<String, dynamic> favorites = _map(snapshot['favorites']);
    _favoriteTracks = _readTracks(favorites['tracks'], true);
    _favoriteAlbums = _readAlbums(favorites['albums'], true);
    _favoritePlaylists = _readPlaylists(favorites['playlists'], true);
    _favoriteArtists = _readArtists(favorites['artists'], true);
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

    final Map<String, List<Track>> tracksByPlaylist = <String, List<Track>>{};
    final loadingPlaylistIds = <String>{};
    final List<Playlist> playlists = <Playlist>[];
    for (final Map<String, dynamic> value in _listOfMaps(
      snapshot['playlists'],
    )) {
      try {
        final Playlist playlist = _copyPlaylist(
          Playlist.fromJson(value),
          false,
        );
        playlists.add(playlist);
        if (value['is_loading_tracks'] == true) {
          loadingPlaylistIds.add(playlist.itemId);
        }
        tracksByPlaylist[playlist.itemId] = _readTracks(value['items'], false);
      } on FormatException {
        continue;
      }
    }
    playlists.sort(_comparePlaylists);
    _playlists = List<Playlist>.unmodifiable(playlists);
    _loadingPlaylistIds = loadingPlaylistIds;
    _playlistTracksByPlaylistId = Map<String, List<Track>>.unmodifiable(
      tracksByPlaylist,
    );
    notifyListeners();
  }

  List<Track> _readTracks(Object? value, bool favorite) =>
      List<Track>.unmodifiable(
        _listOfMaps(
          value,
        ).map((json) => _copyTrack(Track.fromJson(json), favorite)),
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
    super.dispose();
  }

  @override
  void notifyListeners() {
    if (!_isDisposed) super.notifyListeners();
  }
}
