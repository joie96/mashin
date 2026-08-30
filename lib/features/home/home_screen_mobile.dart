import 'dart:async';

import 'package:flutter/material.dart';

import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/userdata_controller.dart';
import '../shared/widgets/media_list_section_mobile.dart';
import '../shared/widgets/media_row_section_mobile.dart';
import '../shared/widgets/media_slide_section_mobile.dart';
import 'home_controller.dart';

class HomeScreenMobile extends StatefulWidget {
  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final UserDataController? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const HomeScreenMobile({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  State<HomeScreenMobile> createState() => _HomeScreenMobileState();
}

class _HomeScreenMobileState extends State<HomeScreenMobile> {
  late final HomeController _controller;

  @override
  void initState() {
    super.initState();
    _controller = HomeController(
      client: widget.client,
      playbackCoordinator: widget.playbackCoordinator,
      userDataController: widget.userDataController,
    )..addListener(_onControllerChanged);
    widget.userDataController?.addListener(_onUserDataChanged);
    unawaited(_controller.load());
  }

  @override
  void dispose() {
    _controller
      ..removeListener(_onControllerChanged)
      ..dispose();
    widget.userDataController?.removeListener(_onUserDataChanged);
    super.dispose();
  }

  void _onControllerChanged() {
    if (mounted) {
      setState(() {});
    }
  }

  void _onUserDataChanged() {
    _controller.refreshFavoriteStatuses();
  }

  void _openRecommendation(MediaItem item) {
    switch (item) {
      case Track track:
        final Album? album = track.album;
        if (album != null) {
          widget.onOpenAlbum?.call(album);
        }
        break;
      case Album album:
        widget.onOpenAlbum?.call(album);
        break;
      case Artist artist:
        widget.onOpenArtist?.call(artist);
        break;
      case Playlist playlist:
        widget.onOpenPlaylist?.call(playlist);
        break;
      default:
        break;
    }
  }

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.fromLTRB(18, 20, 18, 24),
      children: <Widget>[
        Text(
          'Home',
          style: Theme.of(
            context,
          ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w400),
        ),
        const SizedBox(height: 14),
        MediaSlideSectionMobile<MediaItem>(
          title: 'Unsere Empfehlungen für dich',
          isLoading: _controller.isLoadingRecommendations,
          items: _controller.recommendationItems,
          emptyText: 'Keine ListenBrainz-Empfehlungen verfügbar.',
          playbackCoordinator: widget.playbackCoordinator,
          onItemTap: _openRecommendation,
        ),
        const SizedBox(height: 18),
        MediaListSectionMobile<Playlist>(
          title: 'Genres, die du liebst',
          isLoading: _controller.isLoadingGenrePlaylists,
          items: _controller.genrePlaylists,
          emptyText: 'Keine Genre-Playlists verfügbar.',
          playbackCoordinator: widget.playbackCoordinator,
          onItemTap: widget.onOpenPlaylist,
        ),
        const SizedBox(height: 18),
        MediaRowSectionMobile<Playlist>(
          title: 'Playlists, die du lieben wirst',
          isLoading: _controller.isLoadingPlaylists,
          items: _controller.artistPlaylists,
          emptyText: 'Keine Artist-Playlists verfügbar.',
          playbackCoordinator: widget.playbackCoordinator,
          onItemTap: widget.onOpenPlaylist,
        ),
        const SizedBox(height: 18),
        MediaRowSectionMobile<Track>(
          title: 'Deine Lieblingssongs',
          isLoading: _controller.isLoadingTopTracks,
          items: _controller.topTracks,
          emptyText: 'Keine Top-Tracks verfügbar.',
          playbackCoordinator: widget.playbackCoordinator,
          onItemTap: _openRecommendation,
        ),
        const SizedBox(height: 18),
        MediaRowSectionMobile<Artist>(
          title: 'Deine Lieblingskünstler:innen',
          isLoading: _controller.isLoadingTopArtists,
          items: _controller.topArtists,
          emptyText: 'Keine Top-Artists verfügbar.',
          playbackCoordinator: widget.playbackCoordinator,
          onItemTap: widget.onOpenArtist,
        ),
        ..._controller.similarArtistSections.map(
          (HomeSimilarArtistSection section) => Column(
            children: <Widget>[
              const SizedBox(height: 18),
              MediaRowSectionMobile<Artist>(
                title: section.title,
                isLoading: _controller.isLoadingSimilarArtists,
                items: section.artists,
                emptyText: 'Keine ähnlichen Künstler:innen verfügbar.',
                playbackCoordinator: widget.playbackCoordinator,
                onItemTap: widget.onOpenArtist,
              ),
            ],
          ),
        ),
        const SizedBox(height: 18),
        MediaRowSectionMobile<Track>(
          title: 'Kürzlich gespielt',
          isLoading: _controller.isLoadingRecentListens,
          items: _controller.recentListens,
          emptyText: 'Keine kürzlich gehörten Tracks verfügbar.',
          playbackCoordinator: widget.playbackCoordinator,
          onItemTap: _openRecommendation,
        ),
      ],
    );
  }
}
