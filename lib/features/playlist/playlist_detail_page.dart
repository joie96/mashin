import 'package:flutter/material.dart';

import '../../app/config/app_config.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import 'playlist_detail_page_desktop.dart';
import 'playlist_detail_page_mobile.dart';

/// Entry point that renders [PlaylistDetailPageDesktop] or [PlaylistDetailPageMobile] depending on the current viewport
class PlaylistDetailPage extends StatelessWidget {
  const PlaylistDetailPage({
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
      return PlaylistDetailPageMobile(
        client: client,
        playbackCoordinator: playbackCoordinator,
        itemId: itemId,
        providerInstanceIdOrDomain: providerInstanceIdOrDomain,
        onOpenArtist: onOpenArtist,
        onOpenAlbum: onOpenAlbum,
      );
    }

    return PlaylistDetailPageDesktop(
      client: client,
      playbackCoordinator: playbackCoordinator,
      itemId: itemId,
      providerInstanceIdOrDomain: providerInstanceIdOrDomain,
      onOpenArtist: onOpenArtist,
      onOpenAlbum: onOpenAlbum,
    );
  }
}
