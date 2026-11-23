import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../core/core_models.dart';
import '../core/core_bridge.dart';
import '../core/core_events.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_list_section_desktop.dart';
import '../shared/widgets/media_row_section_desktop.dart';
import '../shared/widgets/media_slide_section_desktop.dart';
import 'home_controller.dart';

class HomeScreenDesktop extends StatefulWidget {
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const HomeScreenDesktop({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
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
  bool _isPageVisible = true;
  List<Track> _selectedTopTracks = const <Track>[];
  List<Track> _selectedRediscoveryTracks = const <Track>[];

  // Session data keeps loading while the route is covered.
  @override
  void initState() {
    super.initState();
    _controller = HomeController(
      coreBridge: widget.coreBridge,
      userDataController: widget.userDataController,
    );
    widget.userDataController?.favoritesChanges.addListener(_onUserDataChanged);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) unawaited(_controller.load());
    });
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final isVisible = ModalRoute.isCurrentOf(context) ?? true;
    if (isVisible && !_isPageVisible) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && _isPageVisible) unawaited(_controller.load());
      });
    }
    _isPageVisible = isVisible;
  }

  @override
  void dispose() {
    _controller.dispose();
    widget.userDataController?.favoritesChanges.removeListener(
      _onUserDataChanged,
    );
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

  List<ContextMenuAction> _buildItemMenuActions(List<MediaItem> items) {
    final item = items.first;
    final isSingleItem = items.length == 1;
    final playableItems = items
        .where((item) => item.canPlay)
        .toList(growable: false);
    final bool isFavorite = items.every(_controller.isItemFavorite);
    final Artist? primaryArtist = isSingleItem ? _primaryArtistOf(item) : null;
    final Album? primaryAlbum = isSingleItem && item is Track
        ? item.album
        : null;

    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        isEnabled: playableItems.isNotEmpty,
        icon: Icons.play_arrow,
        onSelected: () => widget.coreBridge.queuePlayMedia(playableItems),
      ),
      if (!isSingleItem || item is Album || item is Playlist)
        ContextMenuAction(
          label: 'Zufällig abspielen',
          isEnabled: playableItems.isNotEmpty,
          icon: Icons.shuffle_outlined,
          onSelected: () =>
              widget.coreBridge.queuePlayMedia(playableItems, shuffle: true),
        ),
      ContextMenuAction(
        label: 'Radio starten',
        isEnabled: playableItems.isNotEmpty,
        icon: Icons.radio_outlined,
        onSelected: () => widget.coreBridge.startRadio(playableItems),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        isEnabled: playableItems.isNotEmpty,
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => widget.coreBridge.queuePlayMedia(
          playableItems,
          option: QueueOption.next,
        ),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        isEnabled: playableItems.isNotEmpty,
        icon: Icons.last_page,
        onSelected: () => widget.coreBridge.queuePlayMedia(
          playableItems,
          option: QueueOption.add,
        ),
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
        clearsSelectionOnSuccess: isFavorite,
        icon: isFavorite
            ? Icons.favorite_outlined
            : Icons.favorite_border_outlined,
        onSelected: () => Future.wait(
          items.map((item) => _controller.setFavorite(item, !isFavorite)),
        ),
      ),
    ];
  }

  Future<void> _showItemMenu(MediaItem item, {BuildContext? anchorContext}) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      anchorContext: anchorContext,
      actions: _buildItemMenuActions(<MediaItem>[item]),
    );
  }

  Future<void> _showItemMenuAtPosition(MediaItem item, Offset globalPosition) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      globalPosition: globalPosition,
      actions: _buildItemMenuActions(<MediaItem>[item]),
    );
  }

  // Selection actions operate on the complete list selection, not the clicked row.
  Future<void> _showSelectionMenu(
    List<Track> items, {
    Offset? globalPosition,
  }) async {
    if (items.isEmpty) return;
    await const AppContextMenuController().showContextMenu(
      context: context,
      globalPosition: globalPosition,
      actions: _buildItemMenuActions(items),
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Container(
      color: colors.bgPrimary,
      child: CustomScrollView(
        slivers: <Widget>[
          const SliverPadding(padding: EdgeInsets.only(top: 24)),
          _sectionSliver(
            Text(
              'Willkommen zurück!',
              style: Theme.of(
                context,
              ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w800),
            ),
          ),
          _sectionSliver(
            ListenableBuilder(
              listenable: _controller,
              builder: (context, child) => _controller.hasLoadError
                  ? const Padding(
                      padding: EdgeInsets.only(top: 12),
                      child: Text('Empfehlungen konnten nicht geladen werden.'),
                    )
                  : const SizedBox.shrink(),
            ),
          ),
          const SliverPadding(padding: EdgeInsets.only(top: 18)),
          _sectionSliver(
            ListenableBuilder(
              listenable: _controller.recommendationsListenable,
              builder: (BuildContext context, Widget? child) =>
                  MediaSlideSectionDesktop<Track>(
                    title: 'Unsere Empfehlungen für dich',
                    startsTrackRadio: true,
                    isLoading: _controller.isLoadingRecommendations,
                    items: _controller.recommendationItems,
                    emptyText: 'Keine Empfehlungen verfügbar.',
                    coverSize: 175,
                    height: 215,
                    coreBridge: widget.coreBridge,
                    coreEvents: widget.coreEvents,
                    isRowFavorite: (MediaItem item, int _) =>
                        _controller.isItemFavorite(item),
                    onRowFavoriteTap: (MediaItem item, int _) => _controller
                        .setFavorite(item, !_controller.isItemFavorite(item)),
                    onRowMoreAnchorTap:
                        (MediaItem item, int _, BuildContext anchor) =>
                            _showItemMenu(item, anchorContext: anchor),
                    onRowContextMenu:
                        (MediaItem item, int _, Offset position) =>
                            _showItemMenuAtPosition(item, position),
                    onArtistTap: widget.onOpenArtist,
                    onAlbumTap: widget.onOpenAlbum,
                  ),
            ),
          ),
          const SliverPadding(padding: EdgeInsets.only(top: 24)),
          _sectionSliver(
            ListenableBuilder(
              listenable: _controller.genrePlaylistsListenable,
              builder: (BuildContext context, Widget? child) =>
                  MediaListSectionDesktop<Playlist>(
                    title: 'Genres, die du liebst',
                    isLoading: _controller.isLoadingGenrePlaylists,
                    items: _controller.genrePlaylists,
                    emptyText: 'Keine Genre-Playlists verfügbar.',
                    coreBridge: widget.coreBridge,
                    onItemTap: widget.onOpenPlaylist,
                    onItemContextMenu:
                        (Playlist item, int _, Offset position) =>
                            _showItemMenuAtPosition(item, position),
                  ),
            ),
          ),
          const SliverPadding(padding: EdgeInsets.only(top: 24)),
          _sectionSliver(
            ListenableBuilder(
              listenable: _controller.artistPlaylistsListenable,
              builder: (BuildContext context, Widget? child) =>
                  MediaRowSectionDesktop<Playlist>(
                    title: 'Playlists, die du lieben wirst',
                    isLoading: _controller.isLoadingPlaylists,
                    items: _controller.artistPlaylists,
                    emptyText: 'Keine Artist-Playlists verfügbar.',
                    coreBridge: widget.coreBridge,
                    coreEvents: widget.coreEvents,
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
          ),
          const SliverPadding(padding: EdgeInsets.only(top: 24)),
          _sectionSliver(
            ListenableBuilder(
              listenable: _controller.topTracksListenable,
              builder: (BuildContext context, Widget? child) =>
                  MediaListSectionDesktop<Track>(
                    title: 'Deine meistgehörten Tracks',
                    isLoading: _controller.isLoadingTopTracks,
                    items: _controller.topTracks,
                    emptyText: 'Keine Top-Tracks verfügbar.',
                    coreBridge: widget.coreBridge,
                    coreEvents: widget.coreEvents,
                    onSelectionChanged: (items) => _selectedTopTracks = items,
                    onAlbumTap: widget.onOpenAlbum,
                    onSelectionMenuTap: () =>
                        _showSelectionMenu(_selectedTopTracks),
                    onItemSelectionContextMenu: (items, position) {
                      _selectedTopTracks = items;
                      unawaited(
                        _showSelectionMenu(items, globalPosition: position),
                      );
                    },
                    onItemMoreAnchorTap:
                        (Track item, int _, BuildContext anchor) =>
                            _showItemMenu(item, anchorContext: anchor),
                    onItemContextMenu: (Track item, int _, Offset position) =>
                        _showItemMenuAtPosition(item, position),
                  ),
            ),
          ),
          const SliverPadding(padding: EdgeInsets.only(top: 24)),
          _sectionSliver(
            ListenableBuilder(
              listenable: _controller.topArtistsListenable,
              builder: (BuildContext context, Widget? child) =>
                  MediaRowSectionDesktop<Artist>(
                    title: 'Deine meistgehörten Künstler:innen',
                    isLoading: _controller.isLoadingTopArtists,
                    items: _controller.topArtists,
                    emptyText: 'Keine Top-Artists verfügbar.',
                    coreBridge: widget.coreBridge,
                    coreEvents: widget.coreEvents,
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
          ),
          _sectionSliver(
            ListenableBuilder(
              listenable: _controller.personalSectionsListenable,
              builder: (context, child) => Column(
                children: [
                  _personalRow<Artist>(
                    'user_artist_recommendations',
                    'Künstler-Empfehlungen',
                    _controller.artistRecommendations,
                  ),
                  const SizedBox(height: 24),
                  _personalRow<Album>(
                    'user_top_albums_monthly',
                    'Deine meistgehörten Alben',
                    _controller.topAlbums,
                  ),
                  const SizedBox(height: 24),
                  _personalRow<Album>(
                    'user_album_recommendations',
                    'Album-Empfehlungen',
                    _controller.albumRecommendations,
                  ),
                  const SizedBox(height: 24),
                  MediaListSectionDesktop<Track>(
                    title: 'Lange nicht gehört',
                    items: _controller.rediscoveryTracks,
                    isLoading: _controller.isFolderLoading(
                      'user_rediscovery_tracks',
                    ),
                    emptyText: 'Keine Tracks verfügbar.',
                    coreBridge: widget.coreBridge,
                    coreEvents: widget.coreEvents,
                    onSelectionChanged: (items) =>
                        _selectedRediscoveryTracks = items,
                    onSelectionMenuTap: () =>
                        _showSelectionMenu(_selectedRediscoveryTracks),
                    onItemSelectionContextMenu: (items, position) {
                      _selectedRediscoveryTracks = items;
                      unawaited(
                        _showSelectionMenu(items, globalPosition: position),
                      );
                    },
                    onItemMoreAnchorTap: (item, index, anchor) =>
                        _showItemMenu(item, anchorContext: anchor),
                    onItemContextMenu: (item, index, position) =>
                        _showItemMenuAtPosition(item, position),
                    onAlbumTap: widget.onOpenAlbum,
                  ),
                  const SizedBox(height: 24),
                ],
              ),
            ),
          ),
          _sectionSliver(
            ListenableBuilder(
              listenable: _controller.artistSectionsListenable,
              builder: (BuildContext context, Widget? child) => Column(
                children: <Widget>[
                  for (final HomeArtistSection section
                      in _controller.artistSections)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 24),
                      child: MediaRowSectionDesktop<Artist>(
                        title: section.title,
                        items: section.artists,
                        isLoading: _controller.isLoadingArtistSections,
                        emptyText: 'Keine Künstlerempfehlungen verfügbar.',
                        coreBridge: widget.coreBridge,
                        coreEvents: widget.coreEvents,
                        isRowFavorite: (Artist item, int _) =>
                            _controller.isItemFavorite(item),
                        onRowFavoriteTap: (Artist item, int _) =>
                            _controller.setFavorite(item, !item.favorite),
                        onRowMoreAnchorTap:
                            (Artist item, int _, BuildContext anchor) =>
                                _showItemMenu(item, anchorContext: anchor),
                        onRowContextMenu:
                            (Artist item, int _, Offset position) =>
                                _showItemMenuAtPosition(item, position),
                        onArtistTap: widget.onOpenArtist,
                      ),
                    ),
                ],
              ),
            ),
          ),
          _sectionSliver(
            ListenableBuilder(
              listenable: _controller.recentListensListenable,
              builder: (BuildContext context, Widget? child) =>
                  MediaRowSectionDesktop<Track>(
                    title: 'Kürzlich gespielt',
                    startsTrackRadio: true,
                    isLoading: _controller.isLoadingRecentListens,
                    items: _controller.recentListens,
                    emptyText: 'Keine kürzlich gehörten Tracks verfügbar.',
                    coreBridge: widget.coreBridge,
                    coreEvents: widget.coreEvents,
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
          ),
          const SliverPadding(padding: EdgeInsets.only(bottom: 28)),
        ],
      ),
    );
  }

  Widget _sectionSliver(Widget child) {
    return SliverPadding(
      padding: const EdgeInsets.symmetric(horizontal: 24),
      sliver: SliverToBoxAdapter(child: child),
    );
  }

  // Personal recommendation rows share media navigation and favorite commands.
  Widget _personalRow<T extends MediaItem>(
    String folderId,
    String title,
    List<T> items,
  ) => MediaRowSectionDesktop<T>(
    title: title,
    items: items,
    isLoading: _controller.isFolderLoading(folderId),
    emptyText: 'Keine Empfehlungen verfügbar.',
    coreBridge: widget.coreBridge,
    coreEvents: widget.coreEvents,
    isRowFavorite: (item, index) => _controller.isItemFavorite(item),
    onRowFavoriteTap: (item, index) =>
        _controller.setFavorite(item, !_controller.isItemFavorite(item)),
    onRowMoreAnchorTap: (item, index, anchor) =>
        _showItemMenu(item, anchorContext: anchor),
    onRowContextMenu: (item, index, position) =>
        _showItemMenuAtPosition(item, position),
    onArtistTap: widget.onOpenArtist,
    onAlbumTap: widget.onOpenAlbum,
  );
}
