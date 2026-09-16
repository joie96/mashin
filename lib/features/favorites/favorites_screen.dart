import 'package:flutter/material.dart';

import '../../app/layout/layout_utils.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../core/core_userdata.dart';
import 'favorites_screen_desktop.dart';
import 'favorites_screen_mobile.dart';

/// Entry point that renders [FavoritesScreenDesktop] or [FavoritesScreenMobile]
/// depending on the current viewport.
class FavoritesScreen extends StatelessWidget {
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const FavoritesScreen({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    required this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  Widget build(BuildContext context) {
    if (isCompactLayout(context)) {
      return FavoritesScreenMobile(
        coreBridge: coreBridge,
        coreEvents: coreEvents,
        userDataController: userDataController,
        onOpenArtist: onOpenArtist,
        onOpenAlbum: onOpenAlbum,
        onOpenPlaylist: onOpenPlaylist,
      );
    }

    return FavoritesScreenDesktop(
      coreBridge: coreBridge,
      coreEvents: coreEvents,
      userDataController: userDataController,
      onOpenArtist: onOpenArtist,
      onOpenAlbum: onOpenAlbum,
      onOpenPlaylist: onOpenPlaylist,
    );
  }
}
