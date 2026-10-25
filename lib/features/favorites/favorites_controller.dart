import 'package:flutter/foundation.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_userdata.dart';

/// Exposes the favorited tracks/albums/playlists/artists held by [UserData]
/// plus the radio/favorite-toggle actions used by the desktop and mobile favorites pages.
/// favorites come from the already loaded user preferences snapshot instead of a dedicated network call.
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

  @override
  void notifyListeners() {
    if (!_isDisposed) {
      super.notifyListeners();
    }
  }

  void _onUserDataChanged() {
    notifyListeners();
  }

  @override
  void dispose() {
    _isDisposed = true;
    userDataController.removeListener(_onUserDataChanged);
    super.dispose();
  }

  bool get isLoading => userDataController.isLoadingFavorites;

  List<Track> get tracks => userDataController.favoriteTracks;
  List<Album> get albums => userDataController.favoriteAlbums;
  List<Playlist> get playlists => userDataController.favoritePlaylists;
  List<Artist> get artists => userDataController.favoriteArtists;

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
