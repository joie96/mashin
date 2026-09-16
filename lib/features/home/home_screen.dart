import 'package:flutter/material.dart';

import '../../app/layout/layout_utils.dart';
import '../core/core_models.dart';
import '../core/core_bridge.dart';
import '../core/core_events.dart';
import '../core/core_userdata.dart';
import 'home_screen_desktop.dart';
import 'home_screen_mobile.dart';

class HomeScreen extends StatelessWidget {
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const HomeScreen({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  Widget build(BuildContext context) {
    if (isCompactLayout(context)) {
      return HomeScreenMobile(
        coreBridge: coreBridge,
        coreEvents: coreEvents,
        userDataController: userDataController,
        onOpenArtist: onOpenArtist,
        onOpenAlbum: onOpenAlbum,
        onOpenPlaylist: onOpenPlaylist,
      );
    }

    return HomeScreenDesktop(
      coreBridge: coreBridge,
      coreEvents: coreEvents,
      userDataController: userDataController,
      onOpenArtist: onOpenArtist,
      onOpenAlbum: onOpenAlbum,
      onOpenPlaylist: onOpenPlaylist,
    );
  }
}
