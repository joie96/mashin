import 'package:flutter/material.dart';

import '../../app/layout/layout_utils.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../core/core_userdata.dart';
import 'playlist_detail_page_desktop.dart';
import 'playlist_detail_page_mobile.dart';

/// Entry point that renders [PlaylistDetailPageDesktop] or [PlaylistDetailPageMobile] depending on the current viewport
class PlaylistDetailPage extends StatelessWidget {
  const PlaylistDetailPage({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onPlaylistDeleted,
  });

  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final VoidCallback? onPlaylistDeleted;

  @override
  Widget build(BuildContext context) {
    if (isCompactLayout(context)) {
      return PlaylistDetailPageMobile(
        coreBridge: coreBridge,
        coreEvents: coreEvents,
        itemId: itemId,
        providerInstanceIdOrDomain: providerInstanceIdOrDomain,
        userDataController: userDataController,
        onOpenArtist: onOpenArtist,
        onOpenAlbum: onOpenAlbum,
        onPlaylistDeleted: onPlaylistDeleted,
      );
    }

    return PlaylistDetailPageDesktop(
      coreBridge: coreBridge,
      coreEvents: coreEvents,
      itemId: itemId,
      providerInstanceIdOrDomain: providerInstanceIdOrDomain,
      userDataController: userDataController,
      onOpenArtist: onOpenArtist,
      onOpenAlbum: onOpenAlbum,
      onPlaylistDeleted: onPlaylistDeleted,
    );
  }
}
