import 'package:flutter/material.dart';

import '../../app/config/app_config.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/userdata_controller.dart';
import 'favorites_screen_desktop.dart';
import 'favorites_screen_mobile.dart';

/// Entry point that renders [FavoritesScreenDesktop] or [FavoritesScreenMobile]
/// depending on the current viewport.
class FavoritesScreen extends StatelessWidget {
  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final UserDataController userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const FavoritesScreen({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    required this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  Widget build(BuildContext context) {
    if (AppConfig.isMobile()) {
      return FavoritesScreenMobile(
        client: client,
        playbackCoordinator: playbackCoordinator,
        userDataController: userDataController,
        onOpenArtist: onOpenArtist,
        onOpenAlbum: onOpenAlbum,
        onOpenPlaylist: onOpenPlaylist,
      );
    }

    return FavoritesScreenDesktop(
      client: client,
      playbackCoordinator: playbackCoordinator,
      userDataController: userDataController,
      onOpenArtist: onOpenArtist,
      onOpenAlbum: onOpenAlbum,
      onOpenPlaylist: onOpenPlaylist,
    );
  }
}
