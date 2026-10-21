import 'package:flutter/material.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../core/core_userdata.dart';
import 'explore_feed.dart';

class ExploreScreenMobile extends StatelessWidget {
  const ExploreScreenMobile({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  @override
  Widget build(BuildContext context) {
    return ExploreFeed(
      isCompact: true,
      coreBridge: coreBridge,
      coreEvents: coreEvents,
      userDataController: userDataController,
      onOpenArtist: onOpenArtist,
      onOpenAlbum: onOpenAlbum,
      onOpenPlaylist: onOpenPlaylist,
    );
  }
}
