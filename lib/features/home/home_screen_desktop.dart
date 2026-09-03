import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../shared/userdata_controller.dart';
import '../shared/widgets/media_list_section_desktop.dart';
import '../shared/widgets/media_row_section_desktop.dart';
import '../shared/widgets/media_slide_section_desktop.dart';
import 'home_controller.dart';

class HomeScreenDesktop extends StatefulWidget {
  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final UserDataController? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const HomeScreenDesktop({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  State<HomeScreenDesktop> createState() => _HomeScreenDesktopState();
}

class _HomeScreenDesktopState extends State<HomeScreenDesktop> {
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

  Artist? _primaryArtistOf(MediaItem item) {
    final List<Artist> artists = switch (item) {
      Track track => track.artists,
      Album album => album.artists,
      _ => const <Artist>[],
    };
    for (final Artist artist in artists) {
      if (artist.itemId.trim().isNotEmpty &&
          artist.provider.trim().isNotEmpty) {
        return artist;
      }
    }
    return null;
  }

  List<ContextMenuAction> _buildItemMenuActions(MediaItem item) {
    final bool isFavorite = _controller.isItemFavorite(item);
    final Artist? primaryArtist = _primaryArtistOf(item);
    final Album? primaryAlbum = item is Track ? item.album : null;

    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        icon: Icons.play_arrow,
        onSelected: () =>
            widget.playbackCoordinator.playMedia(<MediaItem>[item]),
      ),
      if (item is Album || item is Playlist)
        ContextMenuAction(
          label: 'Zufällig abspielen',
          icon: Icons.shuffle_outlined,
          onSelected: () =>
              widget.playbackCoordinator.shufflePlayMedia(<MediaItem>[item]),
        ),
      if (item is! Playlist)
        ContextMenuAction(
          label: 'Radio starten',
          icon: Icons.radio_outlined,
          onSelected: () =>
              widget.playbackCoordinator.playMediaRadioNext(<MediaItem>[item]),
        ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () =>
            widget.playbackCoordinator.playMediaNext(<MediaItem>[item]),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () =>
            widget.playbackCoordinator.playMediaLast(<MediaItem>[item]),
      ),
      if (primaryArtist != null || primaryAlbum != null) ...<ContextMenuAction>[
        const ContextMenuAction.separator(),
        if (primaryArtist != null && widget.onOpenArtist != null)
          ContextMenuAction(
            label: 'Künstler:in öffnen',
            icon: Icons.person_outline,
            onSelected: () => widget.onOpenArtist!(primaryArtist),
          ),
        if (primaryAlbum != null && widget.onOpenAlbum != null)
          ContextMenuAction(
            label: 'Album öffnen',
            icon: Icons.album_outlined,
            onSelected: () => widget.onOpenAlbum!(primaryAlbum),
          ),
      ],
      const ContextMenuAction.separator(),
      ContextMenuAction(
        label: isFavorite
            ? 'Aus Favoriten entfernen'
            : 'Zu Favoriten hinzufügen',
        icon: isFavorite
            ? Icons.favorite_outlined
            : Icons.favorite_border_outlined,
        onSelected: () => _controller.setFavorite(item, !isFavorite),
      ),
    ];
  }

  Future<void> _showItemMenu(MediaItem item, {BuildContext? anchorContext}) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      anchorContext: anchorContext,
      actions: _buildItemMenuActions(item),
    );
  }

  Future<void> _showItemMenuAtPosition(MediaItem item, Offset globalPosition) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      globalPosition: globalPosition,
      actions: _buildItemMenuActions(item),
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Container(
      color: colors.bgPrimary,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(24, 24, 24, 28),
        children: <Widget>[
          Text(
            'Willkommen zurück!',
            style: Theme.of(
              context,
            ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w800),
          ),
          const SizedBox(height: 18),
          ListenableBuilder(
            listenable: _controller.recommendationsListenable,
            builder: (BuildContext context, Widget? child) =>
                MediaSlideSectionDesktop<Track>(
                  title: 'Unsere Empfehlungen für dich',
                  isLoading: _controller.isLoadingRecommendations,
                  items: _controller.recommendationItems,
                  emptyText: 'Keine ListenBrainz-Empfehlungen verfügbar.',
                  coverSize: 175,
                  height: 215,
                  playbackCoordinator: widget.playbackCoordinator,
                  isRowFavorite: (MediaItem item, int _) =>
                      _controller.isItemFavorite(item),
                  onRowFavoriteTap: (MediaItem item, int _) => _controller
                      .setFavorite(item, !_controller.isItemFavorite(item)),
                  onRowMoreAnchorTap:
                      (MediaItem item, int _, BuildContext anchor) =>
                          _showItemMenu(item, anchorContext: anchor),
                  onRowContextMenu: (MediaItem item, int _, Offset position) =>
                      _showItemMenuAtPosition(item, position),
                  onArtistTap: widget.onOpenArtist,
                  onAlbumTap: widget.onOpenAlbum,
                ),
          ),
          const SizedBox(height: 24),
          ListenableBuilder(
            listenable: _controller.genrePlaylistsListenable,
            builder: (BuildContext context, Widget? child) =>
                MediaListSectionDesktop<Playlist>(
                  title: 'Genres, die du liebst',
                  isLoading: _controller.isLoadingGenrePlaylists,
                  items: _controller.genrePlaylists,
                  emptyText: 'Keine Genre-Playlists verfügbar.',
                  maxItems: 9,
                  playbackCoordinator: widget.playbackCoordinator,
                  onItemTap: widget.onOpenPlaylist,
                  onItemContextMenu: (Playlist item, int _, Offset position) =>
                      _showItemMenuAtPosition(item, position),
                ),
          ),
          const SizedBox(height: 24),
          ListenableBuilder(
            listenable: _controller.artistPlaylistsListenable,
            builder: (BuildContext context, Widget? child) =>
                MediaRowSectionDesktop<Playlist>(
                  title: 'Playlists, die du lieben wirst',
                  isLoading: _controller.isLoadingPlaylists,
                  items: _controller.artistPlaylists,
                  emptyText: 'Keine Artist-Playlists verfügbar.',
                  playbackCoordinator: widget.playbackCoordinator,
                  isRowFavorite: (Playlist item, int _) =>
                      _controller.isItemFavorite(item),
                  onRowFavoriteTap: (Playlist item, int _) => _controller
                      .setFavorite(item, !_controller.isItemFavorite(item)),
                  onRowMoreAnchorTap:
                      (Playlist item, int _, BuildContext anchor) =>
                          _showItemMenu(item, anchorContext: anchor),
                  onRowContextMenu: (Playlist item, int _, Offset position) =>
                      _showItemMenuAtPosition(item, position),
                  onPlaylistTap: widget.onOpenPlaylist,
                  showPlaylistDetails: false,
                ),
          ),
          const SizedBox(height: 24),
          ListenableBuilder(
            listenable: _controller.topTracksListenable,
            builder: (BuildContext context, Widget? child) =>
                MediaRowSectionDesktop<Track>(
                  title: 'Deine Lieblingssongs',
                  isLoading: _controller.isLoadingTopTracks,
                  items: _controller.topTracks,
                  emptyText: 'Keine Top-Tracks verfügbar.',
                  playbackCoordinator: widget.playbackCoordinator,
                  isRowFavorite: (Track item, int _) =>
                      _controller.isItemFavorite(item),
                  onRowFavoriteTap: (Track item, int _) => _controller
                      .setFavorite(item, !_controller.isItemFavorite(item)),
                  onRowMoreAnchorTap:
                      (Track item, int _, BuildContext anchor) =>
                          _showItemMenu(item, anchorContext: anchor),
                  onRowContextMenu: (Track item, int _, Offset position) =>
                      _showItemMenuAtPosition(item, position),
                  onArtistTap: widget.onOpenArtist,
                  onAlbumTap: widget.onOpenAlbum,
                ),
          ),
          const SizedBox(height: 24),
          ListenableBuilder(
            listenable: _controller.topArtistsListenable,
            builder: (BuildContext context, Widget? child) =>
                MediaRowSectionDesktop<Artist>(
                  title: 'Deine Lieblingskünstler:innen',
                  isLoading: _controller.isLoadingTopArtists,
                  items: _controller.topArtists,
                  emptyText: 'Keine Top-Artists verfügbar.',
                  playbackCoordinator: widget.playbackCoordinator,
                  isRowFavorite: (Artist item, int _) =>
                      _controller.isItemFavorite(item),
                  onRowFavoriteTap: (Artist item, int _) => _controller
                      .setFavorite(item, !_controller.isItemFavorite(item)),
                  onRowMoreAnchorTap:
                      (Artist item, int _, BuildContext anchor) =>
                          _showItemMenu(item, anchorContext: anchor),
                  onRowContextMenu: (Artist item, int _, Offset position) =>
                      _showItemMenuAtPosition(item, position),
                  onArtistTap: widget.onOpenArtist,
                ),
          ),
          const SizedBox(height: 24),
          ListenableBuilder(
            listenable: _controller.recentListensListenable,
            builder: (BuildContext context, Widget? child) =>
                MediaRowSectionDesktop<Track>(
                  title: 'Kürzlich gespielt',
                  isLoading: _controller.isLoadingRecentListens,
                  items: _controller.recentListens,
                  emptyText: 'Keine kürzlich gehörten Tracks verfügbar.',
                  playbackCoordinator: widget.playbackCoordinator,
                  isRowFavorite: (Track item, int _) =>
                      _controller.isItemFavorite(item),
                  onRowFavoriteTap: (Track item, int _) => _controller
                      .setFavorite(item, !_controller.isItemFavorite(item)),
                  onRowMoreAnchorTap:
                      (Track item, int _, BuildContext anchor) =>
                          _showItemMenu(item, anchorContext: anchor),
                  onRowContextMenu: (Track item, int _, Offset position) =>
                      _showItemMenuAtPosition(item, position),
                  onArtistTap: widget.onOpenArtist,
                  onAlbumTap: widget.onOpenAlbum,
                ),
          ),
        ],
      ),
    );
  }
}
