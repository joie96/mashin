import 'package:flutter/material.dart';

import '../../app/layout/layout_utils.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../core/core_userdata.dart';
import 'search_screen_desktop.dart';
import 'search_screen_mobile.dart';

/// Entry point that renders [SearchScreenDesktop] or [SearchScreenMobile] depending on the current viewport.
class SearchScreen extends StatelessWidget {
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData? userDataController;
  final bool desktopSearchDriven;
  final String desktopSearchQuery;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const SearchScreen({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    this.userDataController,
    this.desktopSearchDriven = false,
    this.desktopSearchQuery = '',
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  Widget build(BuildContext context) {
    if (isCompactLayout(context)) {
      return SearchScreenMobile(
        coreBridge: coreBridge,
        coreEvents: coreEvents,
        userDataController: userDataController,
        onOpenArtist: onOpenArtist,
        onOpenAlbum: onOpenAlbum,
        onOpenPlaylist: onOpenPlaylist,
      );
    }

    return SearchScreenDesktop(
      coreBridge: coreBridge,
      coreEvents: coreEvents,
      userDataController: userDataController,
      desktopSearchDriven: desktopSearchDriven,
      desktopSearchQuery: desktopSearchQuery,
      onOpenArtist: onOpenArtist,
      onOpenAlbum: onOpenAlbum,
      onOpenPlaylist: onOpenPlaylist,
    );
  }
}
