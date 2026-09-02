import 'package:flutter/foundation.dart';

import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/userdata_controller.dart';

class PlaylistsController extends ChangeNotifier {
  PlaylistsController({
    required this.playbackCoordinator,
    required this.userDataController,
  }) {
    userDataController.addListener(_onUserDataChanged);
  }

  final PlaybackCoordinator playbackCoordinator;
  final UserDataController userDataController;

  List<Playlist> get playlists => userDataController.playlists;
  bool get isLoading => userDataController.isLoadingPreferences;

  List<Track> tracksFor(List<Playlist> playlists) => playlists
      .expand(
        (Playlist playlist) =>
            userDataController.tracksForPlaylist(playlist.itemId),
      )
      .toList(growable: false);

  Future<void> play(List<Track> tracks) =>
      playbackCoordinator.playMedia(tracks);

  Future<void> shuffle(List<Track> tracks) =>
      playbackCoordinator.shufflePlayMedia(tracks);

  Future<void> playNext(List<Track> tracks) =>
      playbackCoordinator.playMediaNext(tracks);

  Future<void> playLast(List<Track> tracks) =>
      playbackCoordinator.playMediaLast(tracks);

  Future<Playlist?> createPlaylist(String name) =>
      userDataController.createLocalPlaylist(name);

  Future<bool> renamePlaylist(Playlist playlist, String name) =>
      userDataController.renamePlaylist(playlist, name);

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
