import 'package:flutter/foundation.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_userdata.dart';

/// Reads the Rust favorites snapshot before exposing its shared Flutter projection.
class FavoritesController extends ChangeNotifier {
  FavoritesController({
    required this.coreBridge,
    required this.userDataController,
  }) {
    userDataController.addListener(_onUserDataChanged);
  }

  final MashinCoreBridgeApi coreBridge;
  final UserData userDataController;

  bool _isDisposed = false;
  bool _isLoadingSnapshot = true;
  bool _hasReadSnapshot = false;
  bool _hasLoadError = false;

  // Initial snapshot loading; live updates preserve already visible contents.
  Future<void> load() async {
    if (_isDisposed) return;
    _isLoadingSnapshot = true;
    _hasLoadError = false;
    notifyListeners();
    try {
      await userDataController.refreshLibrarySnapshot();
      if (!_isDisposed) _hasReadSnapshot = true;
    } catch (_) {
      if (!_isDisposed) _hasLoadError = true;
    } finally {
      _isLoadingSnapshot = false;
      notifyListeners();
    }
  }

  @override
  void notifyListeners() {
    if (!_isDisposed) {
      super.notifyListeners();
    }
  }

  void _onUserDataChanged() {
    if (_hasReadSnapshot) notifyListeners();
  }

  @override
  void dispose() {
    _isDisposed = true;
    userDataController.removeListener(_onUserDataChanged);
    super.dispose();
  }

  bool get isLoading =>
      !_hasLoadError &&
      (_isLoadingSnapshot || userDataController.isLoadingFavorites);
  bool get hasLoadError => _hasLoadError;

  List<Track> get tracks =>
      _hasReadSnapshot ? userDataController.favoriteTracks : const [];
  List<Album> get albums =>
      _hasReadSnapshot ? userDataController.favoriteAlbums : const [];
  List<Playlist> get playlists =>
      _hasReadSnapshot ? userDataController.favoritePlaylists : const [];
  List<Artist> get artists =>
      _hasReadSnapshot ? userDataController.favoriteArtists : const [];

  List<Playlist> get playlistsForAdding => userDataController.playlists;

  // Radio
  Future<void> startTrackRadio(Track track) async {
    if (track.itemId.trim().isEmpty || track.provider.trim().isEmpty) {
      return;
    }

    try {
      await coreBridge.startRadio(<MediaItem>[track]);
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }

  Future<void> startItemRadio(MediaItem item) async {
    if (item.itemId.trim().isEmpty || item.provider.trim().isEmpty) {
      return;
    }

    try {
      await coreBridge.startRadio(<MediaItem>[item]);
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }

  Future<void> startArtistRadio(Artist artist) async {
    if (artist.itemId.trim().isEmpty || artist.provider.trim().isEmpty) {
      return;
    }

    try {
      await coreBridge.startRadio(<MediaItem>[artist]);
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }

  // Favorites
  bool isItemFavorite(MediaItem item) => item.favorite;

  Future<void> setFavorite(MediaItem item, bool favorite) async {
    await userDataController.setFavorite(<MediaItem>[item], favorite);
  }

  Future<void> addTracksToPlaylist(
    String playlistId,
    List<Track> tracksToAdd,
  ) async {
    if (playlistId.trim().isEmpty || tracksToAdd.isEmpty) {
      return;
    }
    await userDataController.addPlaylistTracks(playlistId, tracksToAdd);
  }
}
