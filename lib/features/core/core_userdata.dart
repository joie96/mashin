import 'package:flutter/foundation.dart';

import 'core_bridge.dart';
import 'core_models.dart';

/// Projects the Rust-owned favorites and local playlists for Flutter views.
class UserData extends ChangeNotifier {
  static const String localPlaylistProvider = 'mashin';

  UserData({required this._coreBridge});

  final MashinCoreBridgeApi _coreBridge;
  bool _isLoadingPreferences = false;
  bool _isDisposed = false;
  List<Track> _favoriteTracks = const <Track>[];
  List<Album> _favoriteAlbums = const <Album>[];
  List<Playlist> _favoritePlaylists = const <Playlist>[];
  List<Artist> _favoriteArtists = const <Artist>[];
  List<Playlist> _playlists = const <Playlist>[];
  Map<String, List<Track>> _playlistTracksByPlaylistId =
      const <String, List<Track>>{};

  bool get isLoadingPreferences => _isLoadingPreferences;
  List<Track> get favoriteTracks => _favoriteTracks;
  List<Album> get favoriteAlbums => _favoriteAlbums;
  List<Playlist> get favoritePlaylists => _favoritePlaylists;
  List<Artist> get favoriteArtists => _favoriteArtists;
  List<Playlist> get playlists => _playlists;

  List<Track> tracksForPlaylist(String playlistId) =>
      _playlistTracksByPlaylistId[playlistId.trim()] ?? const <Track>[];

  Future<void> loadPreferences() async {
    _isLoadingPreferences = true;
    notifyListeners();
    try {
      _applySnapshot(await _coreBridge.loadUserData());
    } finally {
      _isLoadingPreferences = false;
      notifyListeners();
    }
  }

  bool isFavorite(MediaItem item) {
    final String uri = item.uri?.trim() ?? '';
    if (uri.isEmpty) return false;
    return switch (item.mediaType) {
      MediaType.track => _favoriteTracks.any((item) => _sameUri(item.uri, uri)),
      MediaType.album => _favoriteAlbums.any((item) => _sameUri(item.uri, uri)),
      MediaType.artist => _favoriteArtists.any((item) => _sameUri(item.uri, uri)),
      MediaType.playlist => _favoritePlaylists.any((item) => _sameUri(item.uri, uri)),
      _ => false,
    };
  }

  T withFavoriteStatus<T extends MediaItem>(T item) {
    final bool favorite = isFavorite(item);
    if (item is Track) return _copyTrack(item, favorite) as T;
    if (item is Album) return _copyAlbum(item, favorite) as T;
    if (item is Artist) return _copyArtist(item, favorite) as T;
    if (item is Playlist) return _copyPlaylist(item, favorite) as T;
    return item;
  }

  Future<void> setFavorite(Iterable<MediaItem> items, bool favorite) async {
    await _coreBridge.setUserDataFavorites(items.toList(growable: false), favorite);
    await _refreshSnapshot();
  }

  Future<Playlist?> createLocalPlaylist(String name) async {
    final String normalizedName = name.trim();
    if (normalizedName.isEmpty) return null;
    await _coreBridge.createLocalPlaylist(normalizedName);
    await _refreshSnapshot();
    return _playlists.lastWhere(
      (playlist) => playlist.name == normalizedName,
      orElse: () => throw StateError('Rust did not return the created local playlist'),
    );
  }

  Future<bool> renamePlaylist(Playlist playlist, String name) async {
    if (playlist.itemId.trim().isEmpty || name.trim().isEmpty) return false;
    await _coreBridge.renameLocalPlaylist(playlist.itemId, name.trim());
    await _refreshSnapshot();
    return true;
  }

  Future<bool> removePlaylist(Playlist playlist) async {
    if (playlist.itemId.trim().isEmpty) return false;
    await _coreBridge.removeLocalPlaylist(playlist.itemId);
    await _refreshSnapshot();
    return true;
  }

  Future<bool> addPlaylistTracks(String playlistId, Iterable<Track> tracks) async {
    final List<Track> values = tracks
        .where((track) => (track.uri ?? '').trim().isNotEmpty)
        .toList(growable: false);
    if (playlistId.trim().isEmpty || values.isEmpty) return false;
    await _coreBridge.addLocalPlaylistTracks(playlistId, values);
    await _refreshSnapshot();
    return true;
  }

  Future<bool> removePlaylistTracks(String playlistId, Iterable<Track> tracks) async {
    final List<String> uris = tracks
        .map((track) => track.uri?.trim() ?? '')
        .where((uri) => uri.isNotEmpty)
        .toList(growable: false);
    if (playlistId.trim().isEmpty || uris.isEmpty) return false;
    await _coreBridge.removeLocalPlaylistTracks(playlistId, uris);
    await _refreshSnapshot();
    return true;
  }

  Future<void> _refreshSnapshot() async =>
      _applySnapshot(await _coreBridge.getUserData());

  void _applySnapshot(Map<String, dynamic> snapshot) {
    final Map<String, dynamic> favorites = _map(snapshot['favorites']);
    _favoriteTracks = _readTracks(favorites['tracks'], true);
    _favoriteAlbums = _readAlbums(favorites['albums'], true);
    _favoritePlaylists = _readPlaylists(favorites['playlists'], true);
    _favoriteArtists = _readArtists(favorites['artists'], true);

    final Map<String, List<Track>> tracksByPlaylist = <String, List<Track>>{};
    final List<Playlist> playlists = <Playlist>[];
    for (final Map<String, dynamic> value in _listOfMaps(snapshot['playlists'])) {
      try {
        final Playlist playlist = _copyPlaylist(Playlist.fromJson(value), false);
        playlists.add(playlist);
        tracksByPlaylist[playlist.itemId] = _readTracks(value['items'], false);
      } on FormatException {
        continue;
      }
    }
    playlists.sort(_comparePlaylists);
    _playlists = List<Playlist>.unmodifiable(playlists);
    _playlistTracksByPlaylistId = Map<String, List<Track>>.unmodifiable(tracksByPlaylist);
    notifyListeners();
  }

  List<Track> _readTracks(Object? value, bool favorite) =>
      List<Track>.unmodifiable(_listOfMaps(value).map((json) => _copyTrack(Track.fromJson(json), favorite)));
  List<Album> _readAlbums(Object? value, bool favorite) =>
      List<Album>.unmodifiable(_listOfMaps(value).map((json) => _copyAlbum(Album.fromJson(json), favorite)));
  List<Playlist> _readPlaylists(Object? value, bool favorite) =>
      List<Playlist>.unmodifiable(_listOfMaps(value).map((json) => _copyPlaylist(Playlist.fromJson(json), favorite)));
  List<Artist> _readArtists(Object? value, bool favorite) =>
      List<Artist>.unmodifiable(_listOfMaps(value).map((json) => _copyArtist(Artist.fromJson(json), favorite)));

  Map<String, dynamic> _map(Object? value) =>
      value is Map ? Map<String, dynamic>.from(value) : const <String, dynamic>{};
  List<Map<String, dynamic>> _listOfMaps(Object? value) => value is List
      ? value.whereType<Map>().map((item) => Map<String, dynamic>.from(item)).toList(growable: false)
      : const <Map<String, dynamic>>[];
  bool _sameUri(String? left, String right) =>
      (left ?? '').trim().toLowerCase() == right.toLowerCase();
  int _comparePlaylists(Playlist left, Playlist right) {
    final int sortComparison = (left.sortName ?? '').toLowerCase().compareTo((right.sortName ?? '').toLowerCase());
    return sortComparison != 0 ? sortComparison : left.name.toLowerCase().compareTo(right.name.toLowerCase());
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
    super.dispose();
  }

  @override
  void notifyListeners() {
    if (!_isDisposed) super.notifyListeners();
  }
}