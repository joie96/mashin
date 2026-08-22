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

  const ArtistDetailPage({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
  });

  /// Navigates to the detail page of [artist], mirroring MAUI's ArtistTappedCommand.
  static Future<void> push(
    BuildContext context, {
    required MusicAssistantClient client,
    required PlaybackCoordinator playbackCoordinator,
    required Artist artist,
  }) {
    return Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => ArtistDetailPage(
          client: client,
          playbackCoordinator: playbackCoordinator,
          itemId: artist.itemId,
          providerInstanceIdOrDomain: artist.provider,
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    if (AppConfig.isMobile()) {
      return ArtistDetailPageMobile(
        client: client,
        playbackCoordinator: playbackCoordinator,
        itemId: itemId,
        providerInstanceIdOrDomain: providerInstanceIdOrDomain,
      );
    }

    return ArtistDetailPageDesktop(
      client: client,
      playbackCoordinator: playbackCoordinator,
      itemId: itemId,
      providerInstanceIdOrDomain: providerInstanceIdOrDomain,
    );
  }
}
