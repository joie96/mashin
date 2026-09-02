import 'package:flutter/material.dart';

import '../../app/config/app_config.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/userdata_controller.dart';
import 'explore_screen_desktop.dart';
import 'explore_screen_mobile.dart';

class ExploreScreen extends StatelessWidget {
  const ExploreScreen({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    this.userDataController,
    this.onOpenArtist,
  });

  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final UserDataController? userDataController;
  final ValueChanged<Artist>? onOpenArtist;

  @override
  Widget build(BuildContext context) {
    if (AppConfig.isMobile()) {
      return ExploreScreenMobile(
        client: client,
        playbackCoordinator: playbackCoordinator,
        userDataController: userDataController,
        onOpenArtist: onOpenArtist,
      );
    }

    return ExploreScreenDesktop(
      client: client,
      playbackCoordinator: playbackCoordinator,
      userDataController: userDataController,
      onOpenArtist: onOpenArtist,
    );
  }
}
