import 'package:flutter/material.dart';

import '../../app/layout/layout_utils.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../core/core_userdata.dart';
import 'album_detail_page_desktop.dart';
import 'album_detail_page_mobile.dart';

class AlbumDetailPage extends StatelessWidget {
  const AlbumDetailPage({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
  });

  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

  @override
  Widget build(BuildContext context) {
    if (isCompactLayout(context)) {
      return AlbumDetailPageMobile(
        coreBridge: coreBridge,
        coreEvents: coreEvents,
        itemId: itemId,
        providerInstanceIdOrDomain: providerInstanceIdOrDomain,
        userDataController: userDataController,
        onOpenArtist: onOpenArtist,
        onOpenAlbum: onOpenAlbum,
      );
    }

    return AlbumDetailPageDesktop(
      coreBridge: coreBridge,
      coreEvents: coreEvents,
      itemId: itemId,
      providerInstanceIdOrDomain: providerInstanceIdOrDomain,
      userDataController: userDataController,
      onOpenArtist: onOpenArtist,
      onOpenAlbum: onOpenAlbum,
    );
  }
}
