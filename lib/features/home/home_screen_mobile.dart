import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';

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
  static const double _sectionGap = 28;
  static const double _headingToFirstSectionGap = 20;

  late final HomeController _controller;

  @override
  void initState() {
    super.initState();
    _controller = HomeController(
      client: widget.client,
      playbackCoordinator: widget.playbackCoordinator,
      userDataController: widget.userDataController,
    );
    widget.userDataController?.addListener(_onUserDataChanged);
    unawaited(_controller.load());
  }

  @override
  void dispose() {
    _controller.dispose();
    widget.userDataController?.removeListener(_onUserDataChanged);
    super.dispose();
  }

  void _onUserDataChanged() {
    _controller.refreshFavoriteStatuses();
  }

  void _openTrackContext(MediaItem item) {
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
    return CustomScrollView(
      scrollCacheExtent: const ScrollCacheExtent.pixels(260),
      slivers: <Widget>[
        const SliverPadding(padding: EdgeInsets.only(top: 20)),
        SliverPadding(
          padding: const EdgeInsets.symmetric(horizontal: 18),
          sliver: SliverToBoxAdapter(
            child: Text(
              'Home',
              style: Theme.of(
                context,
              ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w700),
            ),
          ),
        ),
        const SliverPadding(
          padding: EdgeInsets.only(top: _headingToFirstSectionGap),
        ),
        _sectionSliver(
          _RecommendationsSection(
            controller: _controller,
            playbackCoordinator: widget.playbackCoordinator,
            onItemTap: _openTrackContext,
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: _sectionGap)),
        _sectionSliver(
          _GenrePlaylistsSection(
            controller: _controller,
            playbackCoordinator: widget.playbackCoordinator,
            onItemTap: widget.onOpenPlaylist,
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: _sectionGap)),
        _sectionSliver(
          _ArtistPlaylistsSection(
            controller: _controller,
            playbackCoordinator: widget.playbackCoordinator,
            onItemTap: widget.onOpenPlaylist,
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: _sectionGap)),
        _sectionSliver(
          _TopTracksSection(
            controller: _controller,
            playbackCoordinator: widget.playbackCoordinator,
            onItemTap: _openTrackContext,
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: _sectionGap)),
        _sectionSliver(
          _TopArtistsSection(
            controller: _controller,
            playbackCoordinator: widget.playbackCoordinator,
            onItemTap: widget.onOpenArtist,
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: _sectionGap)),
        _sectionSliver(
          _SimilarArtistsSections(
            controller: _controller,
            playbackCoordinator: widget.playbackCoordinator,
            onItemTap: widget.onOpenArtist,
            sectionGap: _sectionGap,
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: _sectionGap)),
        _sectionSliver(
          _RecentListensSection(
            controller: _controller,
            playbackCoordinator: widget.playbackCoordinator,
            onItemTap: _openTrackContext,
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(bottom: 24)),
      ],
    );
  }

  Widget _sectionSliver(Widget child) {
    return SliverPadding(
      padding: const EdgeInsets.symmetric(horizontal: 18),
      sliver: SliverToBoxAdapter(child: RepaintBoundary(child: child)),
    );
  }
}

class _RecommendationsSection extends StatelessWidget {
  const _RecommendationsSection({
    required this.controller,
    required this.playbackCoordinator,
    required this.onItemTap,
  });

  final HomeController controller;
  final PlaybackCoordinator playbackCoordinator;
  final ValueChanged<Track> onItemTap;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller.recommendationsListenable,
      builder: (BuildContext context, Widget? child) =>
          MediaSlideSectionMobile<Track>(
            title: 'Unsere Empfehlungen für dich',
            isLoading: controller.isLoadingRecommendations,
            items: controller.recommendationItems,
            emptyText: 'Keine ListenBrainz-Empfehlungen verfügbar.',
            playbackCoordinator: playbackCoordinator,
            onItemTap: onItemTap,
          ),
    );
  }
}

class _GenrePlaylistsSection extends StatelessWidget {
  const _GenrePlaylistsSection({
    required this.controller,
    required this.playbackCoordinator,
    required this.onItemTap,
  });

  final HomeController controller;
  final PlaybackCoordinator playbackCoordinator;
  final ValueChanged<Playlist>? onItemTap;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller.genrePlaylistsListenable,
      builder: (BuildContext context, Widget? child) =>
          MediaListSectionMobile<Playlist>(
            title: 'Genres, die du liebst',
            isLoading: controller.isLoadingGenrePlaylists,
            items: controller.genrePlaylists,
            emptyText: 'Keine Genre-Playlists verfügbar.',
            playbackCoordinator: playbackCoordinator,
            onItemTap: onItemTap,
          ),
    );
  }
}

class _SimilarArtistsSections extends StatelessWidget {
  const _SimilarArtistsSections({
    required this.controller,
    required this.playbackCoordinator,
    required this.onItemTap,
    required this.sectionGap,
  });

  final HomeController controller;
  final PlaybackCoordinator playbackCoordinator;
  final ValueChanged<Artist>? onItemTap;
  final double sectionGap;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller.similarArtistsListenable,
      builder: (BuildContext context, Widget? child) {
        final List<HomeSimilarArtistSection> sections =
            controller.similarArtistSections;
        final bool showPlaceholder =
            controller.isLoadingSimilarArtists || sections.isEmpty;

        return Column(
          children: <Widget>[
            for (
              int index = 0;
              index < (showPlaceholder ? 1 : sections.length);
              index++
            ) ...<Widget>[
              if (index > 0) SizedBox(height: sectionGap),
              MediaRowSectionMobile<Artist>(
                title: showPlaceholder
                    ? 'Ähnliche Künstler:innen'
                    : sections[index].title,
                isLoading:
                    showPlaceholder && controller.isLoadingSimilarArtists,
                items: showPlaceholder
                    ? const <Artist>[]
                    : sections[index].artists,
                emptyText: 'Keine ähnlichen Künstler:innen verfügbar.',
                playbackCoordinator: playbackCoordinator,
                onItemTap: onItemTap,
              ),
            ],
          ],
        );
      },
    );
  }
}

class _ArtistPlaylistsSection extends StatelessWidget {
  const _ArtistPlaylistsSection({
    required this.controller,
    required this.playbackCoordinator,
    required this.onItemTap,
  });

  final HomeController controller;
  final PlaybackCoordinator playbackCoordinator;
  final ValueChanged<Playlist>? onItemTap;

  @override
  Widget build(BuildContext context) {
    return RepaintBoundary(
      child: ListenableBuilder(
        listenable: controller.artistPlaylistsListenable,
        builder: (BuildContext context, Widget? child) {
          return MediaRowSectionMobile<Playlist>(
            title: 'Playlists, die du lieben wirst',
            isLoading: controller.isLoadingPlaylists,
            items: controller.artistPlaylists,
            emptyText: 'Keine Artist-Playlists verfügbar.',
            playbackCoordinator: playbackCoordinator,
            onItemTap: onItemTap,
            showPlaylistDetails: false,
          );
        },
      ),
    );
  }
}

class _TopTracksSection extends StatelessWidget {
  const _TopTracksSection({
    required this.controller,
    required this.playbackCoordinator,
    required this.onItemTap,
  });

  final HomeController controller;
  final PlaybackCoordinator playbackCoordinator;
  final ValueChanged<MediaItem> onItemTap;

  @override
  Widget build(BuildContext context) {
    return RepaintBoundary(
      child: ListenableBuilder(
        listenable: controller.topTracksListenable,
        builder: (BuildContext context, Widget? child) {
          return MediaRowSectionMobile<Track>(
            title: 'Deine Lieblingssongs',
            isLoading: controller.isLoadingTopTracks,
            items: controller.topTracks,
            emptyText: 'Keine Top-Tracks verfügbar.',
            playbackCoordinator: playbackCoordinator,
            onItemTap: onItemTap,
          );
        },
      ),
    );
  }
}

class _TopArtistsSection extends StatelessWidget {
  const _TopArtistsSection({
    required this.controller,
    required this.playbackCoordinator,
    required this.onItemTap,
  });

  final HomeController controller;
  final PlaybackCoordinator playbackCoordinator;
  final ValueChanged<Artist>? onItemTap;

  @override
  Widget build(BuildContext context) {
    return RepaintBoundary(
      child: ListenableBuilder(
        listenable: controller.topArtistsListenable,
        builder: (BuildContext context, Widget? child) {
          return MediaRowSectionMobile<Artist>(
            title: 'Deine Lieblingskünstler:innen',
            isLoading: controller.isLoadingTopArtists,
            items: controller.topArtists,
            emptyText: 'Keine Top-Artists verfügbar.',
            playbackCoordinator: playbackCoordinator,
            onItemTap: onItemTap,
          );
        },
      ),
    );
  }
}

class _RecentListensSection extends StatelessWidget {
  const _RecentListensSection({
    required this.controller,
    required this.playbackCoordinator,
    required this.onItemTap,
  });

  final HomeController controller;
  final PlaybackCoordinator playbackCoordinator;
  final ValueChanged<MediaItem> onItemTap;

  @override
  Widget build(BuildContext context) {
    return RepaintBoundary(
      child: ListenableBuilder(
        listenable: controller.recentListensListenable,
        builder: (BuildContext context, Widget? child) {
          return MediaRowSectionMobile<Track>(
            title: 'Kürzlich gespielt',
            isLoading: controller.isLoadingRecentListens,
            items: controller.recentListens,
            emptyText: 'Keine kürzlich gehörten Tracks verfügbar.',
            playbackCoordinator: playbackCoordinator,
            onItemTap: onItemTap,
          );
        },
      ),
    );
  }
}
