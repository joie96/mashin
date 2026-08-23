import 'package:flutter/material.dart';

import '../../app/config/app_config.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import 'album_detail_page_desktop.dart';
import 'album_detail_page_mobile.dart';

class AlbumDetailPage extends StatelessWidget {
  const AlbumDetailPage({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.onOpenArtist,
    this.onOpenAlbum,
  });

  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

  @override
  Widget build(BuildContext context) {
    if (AppConfig.isMobile()) {
      return AlbumDetailPageMobile(
        client: client,
        playbackCoordinator: playbackCoordinator,
        itemId: itemId,
        providerInstanceIdOrDomain: providerInstanceIdOrDomain,
        onOpenArtist: onOpenArtist,
        onOpenAlbum: onOpenAlbum,
      );
    }

    return AlbumDetailPageDesktop(
      client: client,
      playbackCoordinator: playbackCoordinator,
      itemId: itemId,
      providerInstanceIdOrDomain: providerInstanceIdOrDomain,
      onOpenArtist: onOpenArtist,
      onOpenAlbum: onOpenAlbum,
    );
  }
}
