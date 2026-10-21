import 'dart:async';

import 'package:flutter/material.dart';

import '../core/core_bridge.dart';
import '../core/core_events.dart';
import '../core/core_models.dart';
import '../core/core_userdata.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../shared/widgets/media_row_section_desktop.dart';
import '../shared/widgets/media_row_section_mobile.dart';
import '../shared/widgets/media_slide_section_desktop.dart';
import '../shared/widgets/media_slide_section_mobile.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import '../shared/widgets/media_table_section_mobile.dart';
import 'explore_controller.dart';

class ExploreFeed extends StatefulWidget {
  const ExploreFeed({
    super.key,
    required this.isCompact,
    required this.coreBridge,
    required this.coreEvents,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  final bool isCompact;
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  @override
  State<ExploreFeed> createState() => _ExploreFeedState();
}

class _ExploreFeedState extends State<ExploreFeed> {
  late ExploreController _controller;

  // Controller ownership and favorite updates.
  @override
  void initState() {
    super.initState();
    _initializeController();
  }

  void _initializeController() {
    _controller = ExploreController(
      coreBridge: widget.coreBridge,
      userDataController: widget.userDataController,
    );
    widget.userDataController?.addListener(_onUserDataChanged);
    unawaited(_controller.load());
  }

  @override
  void didUpdateWidget(covariant ExploreFeed oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.userDataController != widget.userDataController ||
        oldWidget.coreBridge != widget.coreBridge) {
      oldWidget.userDataController?.removeListener(_onUserDataChanged);
      _controller.dispose();
      _initializeController();
    }
  }

  @override
  void dispose() {
    widget.userDataController?.removeListener(_onUserDataChanged);
    _controller.dispose();
    super.dispose();
  }

  void _onUserDataChanged() => _controller.refreshFavoriteStatuses();

  // Feed layout.
  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: _controller,
    builder: (BuildContext context, Widget? child) => CustomScrollView(
      slivers: <Widget>[
        SliverPadding(
          padding: EdgeInsets.only(top: widget.isCompact ? 20 : 18),
        ),
        _section(
          Text(
            'Entdecken',
            style: Theme.of(
              context,
            ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w700),
          ),
        ),
        if (_controller.hasLoadError)
          _section(
            const Padding(
              padding: EdgeInsets.only(top: 12),
              child: Text('Empfehlungen konnten nicht geladen werden.'),
            ),
          ),
        const SliverPadding(padding: EdgeInsets.only(top: 20)),
        _section(_explorationSlider()),
        for (final Widget section in <Widget>[
          _section(
            _mediaRow<Album>(
              'Alben der Woche',
              _controller.weeklyAlbums,
              'Keine Album-Charts verfügbar.',
            ),
          ),
          _similarUsersTable(),
          _section(
            _mediaRow<Artist>(
              'Artists der Woche',
              _controller.weeklyArtists,
              'Keine Artist-Charts verfügbar.',
            ),
          ),
          _section(
            _mediaRow<Track>(
              'Songs der Woche',
              _controller.weeklyTracks,
              'Keine Track-Charts verfügbar.',
            ),
          ),
        ]) ...<Widget>[
          SliverPadding(
            padding: EdgeInsets.only(top: widget.isCompact ? 32 : 20),
          ),
          section,
        ],
        const SliverPadding(padding: EdgeInsets.only(bottom: 28)),
      ],
    ),
  );

  Widget _section(Widget child) => SliverPadding(
    padding: EdgeInsets.symmetric(horizontal: widget.isCompact ? 18 : 20),
    sliver: SliverToBoxAdapter(child: child),
  );

  Widget _explorationSlider() {
    if (widget.isCompact) {
      return MediaSlideSectionMobile<Track>(
        title: 'Entdecke etwas Neues',
        items: _controller.weeklyExploration,
        isLoading: _controller.isLoading,
        emptyText: 'Keine Weekly Exploration verfügbar.',
        coreBridge: widget.coreBridge,
        coreEvents: widget.coreEvents,
        onItemTap: _openItem,
        onSelectionMenuTap: (List<Track> items) => unawaited(_showMenu(items)),
      );
    }
    return MediaSlideSectionDesktop<Track>(
      title: 'Entdecke etwas Neues',
      items: _controller.weeklyExploration,
      isLoading: _controller.isLoading,
      emptyText: 'Keine Weekly Exploration verfügbar.',
      coreBridge: widget.coreBridge,
      coreEvents: widget.coreEvents,
      isRowFavorite: (Track item, int index) => item.favorite,
      onRowFavoriteTap: widget.userDataController == null
          ? null
          : (Track item, int index) =>
                _controller.setFavorite(item, !item.favorite),
      onRowMoreAnchorTap: (Track item, int index, BuildContext anchor) =>
          unawaited(_showMenu(<Track>[item], anchor: anchor)),
      onRowContextMenu: (Track item, int index, Offset position) =>
          unawaited(_showMenu(<Track>[item], position: position)),
      onArtistTap: widget.onOpenArtist,
      onAlbumTap: widget.onOpenAlbum,
    );
  }

  Widget _similarUsersTable() {
    const String title = 'Andere Nutzer hören';
    const String emptyText = 'Kein Weekly Jam ähnlicher Nutzer verfügbar.';
    if (widget.isCompact) {
      return SliverPadding(
        padding: const EdgeInsets.symmetric(horizontal: 18),
        sliver: MediaTableSectionMobile<Track>(
          title: title,
          items: _controller.similarUsersJam,
          isLoading: _controller.isLoading,
          emptyText: emptyText,
          maxItemsPerPage: 3,
          skeletonRowCount: 3,
          coreBridge: widget.coreBridge,
          coreEvents: widget.coreEvents,
          onRowTap: (Track item, int index) =>
              unawaited(_showMenu(<Track>[item])),
          onSelectionMenuTap: (List<Track> items) =>
              unawaited(_showMenu(items)),
        ),
      );
    }
    return _section(
      MediaTableSectionDesktop<Track>(
        title: title,
        items: _controller.similarUsersJam,
        isLoading: _controller.isLoading,
        emptyText: emptyText,
        maxItemsPerPage: 5,
        skeletonRowCount: 5,
        coreBridge: widget.coreBridge,
        coreEvents: widget.coreEvents,
        isRowFavorite: (Track item, int index) => item.favorite,
        onRowFavoriteTap: widget.userDataController == null
            ? null
            : (Track item, int index) =>
                  _controller.setFavorite(item, !item.favorite),
        onRowMoreAnchorTap: (Track item, int index, BuildContext anchor) =>
            unawaited(_showMenu(<Track>[item], anchor: anchor)),
        onRowContextMenu: (Track item, int index, Offset position) =>
            unawaited(_showMenu(<Track>[item], position: position)),
        onRowSelectionContextMenu: (List<Track> items, Offset position) =>
            unawaited(_showMenu(items, position: position)),
        onArtistTap: widget.onOpenArtist,
        onAlbumTap: widget.onOpenAlbum,
      ),
    );
  }

  Widget _mediaRow<T extends MediaItem>(
    String title,
    List<T> items,
    String emptyText,
  ) {
    if (widget.isCompact) {
      return MediaRowSectionMobile<T>(
        title: title,
        items: items,
        emptyText: emptyText,
        isLoading: _controller.isLoading,
        coreBridge: widget.coreBridge,
        coreEvents: widget.coreEvents,
        onItemTap: _openItem,
        onSelectionMenuTap: (List<T> selected) =>
            unawaited(_showMenu(selected)),
      );
    }
    return MediaRowSectionDesktop<T>(
      title: title,
      items: items,
      emptyText: emptyText,
      isLoading: _controller.isLoading,
      coreBridge: widget.coreBridge,
      coreEvents: widget.coreEvents,
      isRowFavorite: (T item, int index) => item.favorite,
      onRowFavoriteTap: widget.userDataController == null
          ? null
          : (T item, int index) =>
                _controller.setFavorite(item, !item.favorite),
      onRowMoreAnchorTap: (T item, int index, BuildContext anchor) =>
          unawaited(_showMenu(<T>[item], anchor: anchor)),
      onRowContextMenu: (T item, int index, Offset position) =>
          unawaited(_showMenu(<T>[item], position: position)),
      onArtistTap: widget.onOpenArtist,
      onAlbumTap: widget.onOpenAlbum,
      onPlaylistTap: widget.onOpenPlaylist,
    );
  }

  // Navigation and media commands.
  void _openItem(MediaItem item) {
    switch (item) {
      case Playlist playlist:
        widget.onOpenPlaylist?.call(playlist);
      case Album album:
        widget.onOpenAlbum?.call(album);
      case Artist artist:
        widget.onOpenArtist?.call(artist);
      case Track track:
        final Album? album = track.album;
        if (album != null) widget.onOpenAlbum?.call(album);
      default:
        break;
    }
  }

  Future<void> _showMenu(
    List<MediaItem> items, {
    BuildContext? anchor,
    Offset? position,
  }) async {
    if (items.isEmpty) return;
    final bool hasPlayableItems = items.any((MediaItem item) => item.canPlay);
    final bool areAllFavorites = items.every((MediaItem item) => item.favorite);
    final List<Track> tracks = items.whereType<Track>().toList(growable: false);
    final UserData? userData = widget.userDataController;
    await const AppContextMenuController().showContextMenu(
      context: context,
      anchorContext: anchor,
      globalPosition: position,
      actions: <ContextMenuAction>[
        ContextMenuAction(
          label: 'Abspielen',
          icon: Icons.play_arrow,
          isEnabled: hasPlayableItems,
          onSelected: () => widget.coreBridge.queuePlayMedia(items),
        ),
        ContextMenuAction(
          label: 'Zufällig abspielen',
          icon: Icons.shuffle,
          isEnabled: hasPlayableItems,
          onSelected: () =>
              widget.coreBridge.queuePlayMedia(items, shuffle: true),
        ),
        ContextMenuAction(
          label: 'Radio starten',
          icon: Icons.radio_outlined,
          isEnabled: hasPlayableItems,
          onSelected: () => widget.coreBridge.startRadio(items),
        ),
        ContextMenuAction(
          label: 'Als Nächstes spielen',
          icon: Icons.keyboard_return,
          iconQuarterTurns: 2,
          isEnabled: hasPlayableItems,
          onSelected: () =>
              widget.coreBridge.queuePlayMedia(items, option: QueueOption.next),
        ),
        ContextMenuAction(
          label: 'Als Letztes spielen',
          icon: Icons.last_page,
          isEnabled: hasPlayableItems,
          onSelected: () =>
              widget.coreBridge.queuePlayMedia(items, option: QueueOption.add),
        ),
        if (items.length == 1 && items.first is! Track)
          ContextMenuAction(
            label: 'Öffnen',
            icon: Icons.open_in_new,
            onSelected: () => _openItem(items.first),
          ),
        if (tracks.isNotEmpty &&
            userData != null &&
            userData.playlists.isNotEmpty)
          ContextMenuAction.submenu(
            label: 'Zu Wiedergabeliste hinzufügen',
            icon: Icons.playlist_add,
            children: userData.playlists
                .map(
                  (Playlist playlist) => ContextMenuAction(
                    label: playlist.name,
                    icon: Icons.playlist_play,
                    onSelected: () =>
                        userData.addPlaylistTracks(playlist.itemId, tracks),
                  ),
                )
                .toList(growable: false),
          ),
        const ContextMenuAction.separator(),
        ContextMenuAction(
          label: areAllFavorites
              ? 'Aus Favoriten entfernen'
              : 'Zu Favoriten hinzufügen',
          icon: areAllFavorites ? Icons.favorite : Icons.favorite_border,
          isEnabled: userData != null,
          onSelected: () async {
            for (final MediaItem item in items) {
              await _controller.setFavorite(item, !areAllFavorites);
            }
          },
        ),
      ],
    );
  }
}
