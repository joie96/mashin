import 'package:flutter/material.dart';

import '../../app/config/app_config.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/userdata_controller.dart';
import 'search_screen_desktop.dart';
import 'search_screen_mobile.dart';

/// Entry point that renders [SearchScreenDesktop] or [SearchScreenMobile] depending on the current viewport.
class SearchScreen extends StatelessWidget {
  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final UserDataController? userDataController;
  final bool desktopSearchDriven;
  final String desktopSearchQuery;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const SearchScreen({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    this.userDataController,
    this.desktopSearchDriven = false,
    this.desktopSearchQuery = '',
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  Widget build(BuildContext context) {
    if (AppConfig.isMobile()) {
      return SearchScreenMobile(
        client: client,
        playbackCoordinator: playbackCoordinator,
        userDataController: userDataController,
        onOpenArtist: onOpenArtist,
        onOpenAlbum: onOpenAlbum,
        onOpenPlaylist: onOpenPlaylist,
      );
    }

    return SearchScreenDesktop(
      client: client,
      playbackCoordinator: playbackCoordinator,
      userDataController: userDataController,
      desktopSearchDriven: desktopSearchDriven,
      desktopSearchQuery: desktopSearchQuery,
      onOpenArtist: onOpenArtist,
      onOpenAlbum: onOpenAlbum,
      onOpenPlaylist: onOpenPlaylist,
    );
  }
}
