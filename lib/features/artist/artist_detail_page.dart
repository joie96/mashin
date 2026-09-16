import 'package:flutter/material.dart';

import '../../app/layout/layout_utils.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../core/core_userdata.dart';
import 'artist_detail_page_desktop.dart';
import 'artist_detail_page_mobile.dart';

/// Entry point that renders [ArtistDetailPageDesktop] or [ArtistDetailPageMobile] depending on the current viewport
class ArtistDetailPage extends StatelessWidget {
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

  const ArtistDetailPage({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
  });

  @override
  Widget build(BuildContext context) {
    if (isCompactLayout(context)) {
      return ArtistDetailPageMobile(
        coreBridge: coreBridge,
        coreEvents: coreEvents,
        itemId: itemId,
        providerInstanceIdOrDomain: providerInstanceIdOrDomain,
        userDataController: userDataController,
        onOpenArtist: onOpenArtist,
        onOpenAlbum: onOpenAlbum,
      );
    }

    return ArtistDetailPageDesktop(
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
