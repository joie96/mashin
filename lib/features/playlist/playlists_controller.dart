import 'package:flutter/foundation.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_userdata.dart';

class PlaylistsController extends ChangeNotifier {
  PlaylistsController({
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
      if (!_isDisposed) notifyListeners();
    }
  }

  // Current page projection.
  List<Playlist> get playlists =>
      _hasReadSnapshot ? userDataController.playlists : const [];
  bool get isLoading =>
      !_hasLoadError &&
      (_isLoadingSnapshot || userDataController.isLoadingPlaylists);
  bool get hasLoadError => _hasLoadError;

  List<Track> tracksFor(List<Playlist> playlists) => playlists
      .expand(
        (Playlist playlist) =>
            userDataController.tracksForPlaylist(playlist.itemId),
      )
      .toList(growable: false);

  // Playlist playback commands.
  Future<void> play(List<Track> tracks) => coreBridge.queuePlayMedia(tracks);

  Future<void> shuffle(List<Track> tracks) =>
      coreBridge.queuePlayMedia(tracks, shuffle: true);

  Future<void> playNext(List<Track> tracks) =>
      coreBridge.queuePlayMedia(tracks, option: QueueOption.next);

  Future<void> playLast(List<Track> tracks) =>
      coreBridge.queuePlayMedia(tracks, option: QueueOption.add);

  Future<Playlist?> createPlaylist(String name) =>
      userDataController.createPrivatePlaylist(name);

  Future<bool> renamePlaylist(Playlist playlist, String name) =>
      userDataController.renamePlaylist(playlist, name);

  Future<bool> refreshPrivatePlaylistCover(Playlist playlist) =>
      userDataController.refreshPrivatePlaylistCover(playlist);

  Future<void> deletePlaylists(List<Playlist> playlists) async {
    for (final Playlist playlist in playlists) {
      await userDataController.removePlaylist(playlist);
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
}
