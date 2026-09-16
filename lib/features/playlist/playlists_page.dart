import 'package:flutter/material.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../core/core_userdata.dart';
import 'playlists_page_mobile.dart';

class PlaylistsPage extends StatelessWidget {
  const PlaylistsPage({
    required this.coreBridge,
    required this.coreEvents,
    required this.userDataController,
    required this.onOpenPlaylist,
    super.key,
  });

  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData userDataController;
  final ValueChanged<Playlist> onOpenPlaylist;

  @override
  Widget build(BuildContext context) {
    return PlaylistsPageMobile(
      coreBridge: coreBridge,
      coreEvents: coreEvents,
      userDataController: userDataController,
      onOpenPlaylist: onOpenPlaylist,
    );
  }
}
