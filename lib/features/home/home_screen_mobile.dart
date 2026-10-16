import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';

import '../core/core_models.dart';
import '../core/core_bridge.dart';
import '../core/core_events.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_list_section_mobile.dart';
import '../shared/widgets/media_row_section_mobile.dart';
import '../shared/widgets/media_slide_section_mobile.dart';
import 'home_controller.dart';

class HomeScreenMobile extends StatefulWidget {
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const HomeScreenMobile({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  State<HomeScreenMobile> createState() => _HomeScreenMobileState();
}

class _HomeScreenMobileState extends State<HomeScreenMobile> {
  static const double _sectionGap = 32;
  static const double _headingToFirstSectionGap = 20;

  late final HomeController _controller;

  @override
  void initState() {
    super.initState();
    _controller = HomeController(
      coreBridge: widget.coreBridge,
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

  Artist? _primaryArtistOf(MediaItem item) {
    final List<Artist> artists = switch (item) {
      Track track => track.artists,
      Album album => album.artists,
      _ => const <Artist>[],
    };
    return artists.firstOrNull;
  }

  List<ContextMenuAction> _buildSelectionMenuActions<T extends MediaItem>(
    List<T> selectedItems,
  ) {
    final List<MediaItem> targets = selectedItems.cast<MediaItem>();
    final MediaItem item = targets.first;
    final List<Track> tracks = targets.whereType<Track>().toList(
      growable: false,
    );
    final bool isSingleTarget = targets.length == 1;
    final bool allFavorite = targets.every(_controller.isItemFavorite);
    final List<ContextMenuAction> actions = <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.play_arrow,
        onSelected: () => widget.coreBridge.queuePlayMedia(targets),
      ),
      if (item is Album || item is Playlist || item is Artist)
        ContextMenuAction(
          label: 'Zufällig abspielen',
          isEnabled: targets.any((target) => target.canPlay),
          icon: Icons.shuffle_outlined,
          onSelected: () =>
              widget.coreBridge.queuePlayMedia(targets, shuffle: true),
        ),
      ContextMenuAction(
        label: 'Radio starten',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.radio_outlined,
        onSelected: () => widget.coreBridge.startRadio(targets),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () =>
            widget.coreBridge.queuePlayMedia(targets, option: QueueOption.next),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.last_page,
        onSelected: () =>
            widget.coreBridge.queuePlayMedia(targets, option: QueueOption.add),
      ),
    ];

    if (isSingleTarget) {
      final Artist? primaryArtist = item is Artist
          ? item
          : _primaryArtistOf(item);
      final Album? primaryAlbum = item is Track ? item.album : null;
      if ((primaryArtist != null && widget.onOpenArtist != null) ||
          (primaryAlbum != null && widget.onOpenAlbum != null)) {
        actions.add(const ContextMenuAction.separator());
        if (primaryArtist != null && widget.onOpenArtist != null) {
          actions.add(
            ContextMenuAction(
              label: 'Künstler:in öffnen',
              icon: Icons.person_outline,
              onSelected: () => widget.onOpenArtist!(primaryArtist),
            ),
          );
        }
        if (primaryAlbum != null && widget.onOpenAlbum != null) {
          actions.add(
            ContextMenuAction(
              label: 'Album öffnen',
              icon: Icons.album_outlined,
              onSelected: () => widget.onOpenAlbum!(primaryAlbum),
            ),
          );
        }
      }
    }

    final UserData? userData = widget.userDataController;
    if (tracks.isNotEmpty &&
        userData != null &&
        userData.playlists.isNotEmpty) {
      actions
        ..add(const ContextMenuAction.separator())
        ..add(
          ContextMenuAction.submenu(
            label: 'Zu Wiedergabeliste hinzufügen',
            icon: Icons.playlist_add_outlined,
            children: userData.playlists
                .map(
                  (Playlist playlist) => ContextMenuAction(
                    label: playlist.name,
                    icon: Icons.playlist_play_outlined,
                    onSelected: () =>
                        userData.addPlaylistTracks(playlist.itemId, tracks),
                  ),
                )
                .toList(growable: false),
          ),
        );
    }

    actions
      ..add(const ContextMenuAction.separator())
      ..add(
        ContextMenuAction(
          label: allFavorite
              ? 'Aus Favoriten entfernen'
              : 'Zu Favoriten hinzufügen',
          icon: allFavorite
              ? Icons.favorite_outlined
              : Icons.favorite_border_outlined,
          isEnabled: widget.userDataController != null,
          onSelected: () async {
            for (final MediaItem target in targets) {
              await _controller.setFavorite(target, !allFavorite);
            }
          },
        ),
      );
    return actions;
  }

  Future<void> _showSelectionMenu<T extends MediaItem>(List<T> items) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      actions: _buildSelectionMenuActions(items),
    );
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
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            onItemTap: _openTrackContext,
            onSelectionMenuTap: (List<Track> items) =>
                unawaited(_showSelectionMenu(items)),
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: _sectionGap)),
        _sectionSliver(
          _GenrePlaylistsSection(
            controller: _controller,
            coreBridge: widget.coreBridge,
            onItemTap: widget.onOpenPlaylist,
            onSelectionMenuTap: (List<Playlist> items) =>
                unawaited(_showSelectionMenu(items)),
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: _sectionGap)),
        _sectionSliver(
          _ArtistPlaylistsSection(
            controller: _controller,
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            onItemTap: widget.onOpenPlaylist,
            onSelectionMenuTap: (List<Playlist> items) =>
                unawaited(_showSelectionMenu(items)),
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: _sectionGap)),
        _sectionSliver(
          _TopTracksSection(
            controller: _controller,
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            onItemTap: _openTrackContext,
            onSelectionMenuTap: (List<Track> items) =>
                unawaited(_showSelectionMenu(items)),
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: _sectionGap)),
        _sectionSliver(
          _TopArtistsSection(
            controller: _controller,
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            onItemTap: widget.onOpenArtist,
            onSelectionMenuTap: (List<Artist> items) =>
                unawaited(_showSelectionMenu(items)),
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: _sectionGap)),
        _sectionSliver(
          _SimilarArtistsSections(
            controller: _controller,
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            onItemTap: widget.onOpenArtist,
            onSelectionMenuTap: (List<Artist> items) =>
                unawaited(_showSelectionMenu(items)),
            sectionGap: _sectionGap,
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: _sectionGap)),
        _sectionSliver(
          _RecentListensSection(
            controller: _controller,
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            onItemTap: _openTrackContext,
            onSelectionMenuTap: (List<Track> items) =>
                unawaited(_showSelectionMenu(items)),
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
    required this.coreBridge,
    required this.coreEvents,
    required this.onItemTap,
    required this.onSelectionMenuTap,
  });

  final HomeController controller;
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final ValueChanged<Track> onItemTap;
  final ValueChanged<List<Track>> onSelectionMenuTap;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller.recommendationsListenable,
      builder: (BuildContext context, Widget? child) =>
          MediaSlideSectionMobile<Track>(
            title: 'Unsere Empfehlungen für dich',
            isLoading: controller.isLoadingRecommendations,
            items: controller.recommendationItems,
            emptyText: 'Keine Empfehlungen verfügbar.',
            coreBridge: coreBridge,
            coreEvents: coreEvents,
            onItemTap: onItemTap,
            onSelectionMenuTap: onSelectionMenuTap,
          ),
    );
  }
}

class _GenrePlaylistsSection extends StatelessWidget {
  const _GenrePlaylistsSection({
    required this.controller,
    required this.coreBridge,
    required this.onItemTap,
    required this.onSelectionMenuTap,
  });

  final HomeController controller;
  final MashinCoreBridgeApi coreBridge;
  final ValueChanged<Playlist>? onItemTap;
  final ValueChanged<List<Playlist>> onSelectionMenuTap;

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
            coreBridge: coreBridge,
            onItemTap: onItemTap,
            onSelectionMenuTap: onSelectionMenuTap,
          ),
    );
  }
}

class _SimilarArtistsSections extends StatelessWidget {
  const _SimilarArtistsSections({
    required this.controller,
    required this.coreBridge,
    required this.coreEvents,
    required this.onItemTap,
    required this.onSelectionMenuTap,
    required this.sectionGap,
  });

  final HomeController controller;
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final ValueChanged<Artist>? onItemTap;
  final ValueChanged<List<Artist>> onSelectionMenuTap;
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
                coreBridge: coreBridge,
                coreEvents: coreEvents,
                onItemTap: onItemTap,
                onSelectionMenuTap: onSelectionMenuTap,
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
    required this.coreBridge,
    required this.coreEvents,
    required this.onItemTap,
    required this.onSelectionMenuTap,
  });

  final HomeController controller;
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final ValueChanged<Playlist>? onItemTap;
  final ValueChanged<List<Playlist>> onSelectionMenuTap;

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
            coreBridge: coreBridge,
            coreEvents: coreEvents,
            onItemTap: onItemTap,
            onSelectionMenuTap: onSelectionMenuTap,
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
    required this.coreBridge,
    required this.coreEvents,
    required this.onItemTap,
    required this.onSelectionMenuTap,
  });

  final HomeController controller;
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final ValueChanged<MediaItem> onItemTap;
  final ValueChanged<List<Track>> onSelectionMenuTap;

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
            coreBridge: coreBridge,
            coreEvents: coreEvents,
            onItemTap: onItemTap,
            onSelectionMenuTap: onSelectionMenuTap,
          );
        },
      ),
    );
  }
}

class _TopArtistsSection extends StatelessWidget {
  const _TopArtistsSection({
    required this.controller,
    required this.coreBridge,
    required this.coreEvents,
    required this.onItemTap,
    required this.onSelectionMenuTap,
  });

  final HomeController controller;
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final ValueChanged<Artist>? onItemTap;
  final ValueChanged<List<Artist>> onSelectionMenuTap;

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
            coreBridge: coreBridge,
            coreEvents: coreEvents,
            onItemTap: onItemTap,
            onSelectionMenuTap: onSelectionMenuTap,
          );
        },
      ),
    );
  }
}

class _RecentListensSection extends StatelessWidget {
  const _RecentListensSection({
    required this.controller,
    required this.coreBridge,
    required this.coreEvents,
    required this.onItemTap,
    required this.onSelectionMenuTap,
  });

  final HomeController controller;
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final ValueChanged<MediaItem> onItemTap;
  final ValueChanged<List<Track>> onSelectionMenuTap;

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
            coreBridge: coreBridge,
            coreEvents: coreEvents,
            onItemTap: onItemTap,
            onSelectionMenuTap: onSelectionMenuTap,
          );
        },
      ),
    );
  }
}
