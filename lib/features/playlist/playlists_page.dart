import 'package:flutter/material.dart';

import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/userdata_controller.dart';
import 'playlists_page_mobile.dart';

class PlaylistsPage extends StatelessWidget {
  const PlaylistsPage({
    required this.playbackCoordinator,
    required this.userDataController,
    required this.onOpenPlaylist,
    super.key,
  });

  final PlaybackCoordinator playbackCoordinator;
  final UserDataController userDataController;
  final ValueChanged<Playlist> onOpenPlaylist;

  @override
  Widget build(BuildContext context) {
    return PlaylistsPageMobile(
      playbackCoordinator: playbackCoordinator,
      userDataController: userDataController,
      onOpenPlaylist: onOpenPlaylist,
    );
  }
}
