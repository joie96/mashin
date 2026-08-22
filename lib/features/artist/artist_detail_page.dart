import 'package:flutter/material.dart';

import '../../app/config/app_config.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import 'artist_detail_page_desktop.dart';
import 'artist_detail_page_mobile.dart';

/// Entry point that renders [ArtistDetailPageDesktop] or [ArtistDetailPageMobile] depending on the current viewport
class ArtistDetailPage extends StatelessWidget {
  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final ValueChanged<Artist>? onOpenArtist;

  const ArtistDetailPage({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.onOpenArtist,
  });

  @override
  Widget build(BuildContext context) {
    if (AppConfig.isMobile()) {
      return ArtistDetailPageMobile(
        client: client,
        playbackCoordinator: playbackCoordinator,
        itemId: itemId,
        providerInstanceIdOrDomain: providerInstanceIdOrDomain,
        onOpenArtist: onOpenArtist,
      );
    }

    return ArtistDetailPageDesktop(
      client: client,
      playbackCoordinator: playbackCoordinator,
      itemId: itemId,
      providerInstanceIdOrDomain: providerInstanceIdOrDomain,
      onOpenArtist: onOpenArtist,
    );
  }
}
