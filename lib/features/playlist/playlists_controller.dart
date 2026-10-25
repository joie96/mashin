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

  List<Playlist> get playlists => userDataController.playlists;
  bool get isLoading => userDataController.isLoadingPlaylists;

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
    notifyListeners();
  }

  @override
  void dispose() {
    userDataController.removeListener(_onUserDataChanged);
    super.dispose();
  }
}
