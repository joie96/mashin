import 'dart:async';

import 'package:flutter/material.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_row_section_desktop.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import 'favorites_controller.dart';

/// Desktop favorites page: stacked track table plus album/playlist/artist rows,
/// each with its own context menu.
class FavoritesScreenDesktop extends StatefulWidget {
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const FavoritesScreenDesktop({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    required this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  State<FavoritesScreenDesktop> createState() => _FavoritesScreenDesktopState();
}

class _FavoritesScreenDesktopState extends State<FavoritesScreenDesktop> {
  late final FavoritesController _controller;

  @override
  void initState() {
    super.initState();
    _controller = FavoritesController(
      coreBridge: widget.coreBridge,
      userDataController: widget.userDataController,
    )..addListener(_onControllerChanged);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) unawaited(_controller.load());
    });
  }

  void _onControllerChanged() {
    if (mounted) {
      setState(() {});
    }
  }

  @override
  void dispose() {
    _controller.removeListener(_onControllerChanged);
    _controller.dispose();
    super.dispose();
  }

  Artist? _primaryArtistOf(MediaItem item) {
    final List<Artist> candidates = switch (item) {
      Track track => track.artists,
      Album album => album.artists,
      _ => const <Artist>[],
    };
    for (final Artist artist in candidates) {
      if (artist.itemId.trim().isNotEmpty &&
          artist.provider.trim().isNotEmpty) {
        return artist;
      }
    }
    return null;
  }

  List<ContextMenuAction> _buildPlaylistSubmenu(List<Track> tracks) {
    return _controller.playlistsForAdding
        .map(
          (Playlist playlist) => ContextMenuAction(
            label: playlist.name,
            icon: Icons.playlist_play_outlined,
            onSelected: () =>
                _controller.addTracksToPlaylist(playlist.itemId, tracks),
          ),
        )
        .toList(growable: false);
  }

  List<ContextMenuAction> _buildTrackMenuActions(
    Track track, [
    List<Track> selectedTracks = const <Track>[],
  ]) {
    final List<Track> tracks = selectedTracks.isEmpty
        ? <Track>[track]
        : selectedTracks;
    final bool isFavorite = _controller.isItemFavorite(track);
    final Artist? primaryArtist = _primaryArtistOf(track);
    final Album? primaryAlbum = track.album;
    final List<ContextMenuAction> playlistActions = _buildPlaylistSubmenu(
      tracks,
    );

    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        isEnabled: tracks.any((track) => track.canPlay),
        icon: Icons.play_arrow,
        onSelected: () => widget.coreBridge.queuePlayMedia(tracks),
      ),
      ContextMenuAction(
        label: 'Radio starten',
        isEnabled: tracks.any((track) => track.canPlay),
        icon: Icons.radio_outlined,
        onSelected: () => widget.coreBridge.startRadio(tracks),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        isEnabled: tracks.any((track) => track.canPlay),
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () =>
            widget.coreBridge.queuePlayMedia(tracks, option: QueueOption.next),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        isEnabled: tracks.any((track) => track.canPlay),
        icon: Icons.last_page,
        onSelected: () =>
            widget.coreBridge.queuePlayMedia(tracks, option: QueueOption.add),
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
      if (playlistActions.isNotEmpty) ...<ContextMenuAction>[
        const ContextMenuAction.separator(),
        ContextMenuAction.submenu(
          label: 'Zu Wiedergabeliste hinzufügen',
          icon: Icons.playlist_add_outlined,
          children: playlistActions,
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
        onSelected: () async {
          for (final Track selectedTrack in tracks) {
            await _controller.setFavorite(selectedTrack, !isFavorite);
          }
        },
      ),
    ];
  }

  List<ContextMenuAction> _buildAlbumMenuActions(Album album) {
    final bool isFavorite = _controller.isItemFavorite(album);
    final Artist? primaryArtist = _primaryArtistOf(album);

    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        isEnabled: album.canPlay,
        icon: Icons.play_arrow,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[album]),
      ),
      ContextMenuAction(
        label: 'Zufällig abspielen',
        isEnabled: album.canPlay,
        icon: Icons.shuffle_outlined,
        onSelected: () =>
            widget.coreBridge.queuePlayMedia(<MediaItem>[album], shuffle: true),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        isEnabled: album.canPlay,
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[
          album,
        ], option: QueueOption.next),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        isEnabled: album.canPlay,
        icon: Icons.last_page,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[
          album,
        ], option: QueueOption.add),
      ),
      if ((primaryArtist != null && widget.onOpenArtist != null) ||
          widget.onOpenAlbum != null) ...<ContextMenuAction>[
        const ContextMenuAction.separator(),
        if (primaryArtist != null && widget.onOpenArtist != null)
          ContextMenuAction(
            label: 'Künstler:in öffnen',
            icon: Icons.person_outline,
            onSelected: () => widget.onOpenArtist!(primaryArtist),
          ),
        if (widget.onOpenAlbum != null)
          ContextMenuAction(
            label: 'Album öffnen',
            icon: Icons.album_outlined,
            onSelected: () => widget.onOpenAlbum!(album),
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
        onSelected: () => _controller.setFavorite(album, !isFavorite),
      ),
    ];
  }

  List<ContextMenuAction> _buildPlaylistMenuActions(Playlist playlist) {
    final bool isFavorite = _controller.isItemFavorite(playlist);

    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        isEnabled: playlist.canPlay,
        icon: Icons.play_arrow,
        onSelected: () =>
            widget.coreBridge.queuePlayMedia(<MediaItem>[playlist]),
      ),
      ContextMenuAction(
        label: 'Zufällig abspielen',
        isEnabled: playlist.canPlay,
        icon: Icons.shuffle_outlined,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[
          playlist,
        ], shuffle: true),
      ),
      ContextMenuAction(
        label: 'Radio starten',
        isEnabled: playlist.canPlay,
        icon: Icons.radio_outlined,
        onSelected: () => widget.coreBridge.startRadio(<MediaItem>[playlist]),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        isEnabled: playlist.canPlay,
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[
          playlist,
        ], option: QueueOption.next),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        isEnabled: playlist.canPlay,
        icon: Icons.last_page,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[
          playlist,
        ], option: QueueOption.add),
      ),
      const ContextMenuAction.separator(),
      ContextMenuAction(
        label: isFavorite
            ? 'Aus Favoriten entfernen'
            : 'Zu Favoriten hinzufügen',
        icon: isFavorite
            ? Icons.favorite_outlined
            : Icons.favorite_border_outlined,
        onSelected: () => _controller.setFavorite(playlist, !isFavorite),
      ),
    ];
  }

  List<ContextMenuAction> _buildArtistMenuActions(Artist artist) {
    final bool isFavorite = _controller.isItemFavorite(artist);

    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        isEnabled: artist.canPlay,
        icon: Icons.play_arrow,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[artist]),
      ),
      ContextMenuAction(
        label: 'Radio starten',
        isEnabled: artist.canPlay,
        icon: Icons.radio_outlined,
        onSelected: () => _controller.startArtistRadio(artist),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        isEnabled: artist.canPlay,
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[
          artist,
        ], option: QueueOption.next),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        isEnabled: artist.canPlay,
        icon: Icons.last_page,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[
          artist,
        ], option: QueueOption.add),
      ),
      if (widget.onOpenArtist != null) ...<ContextMenuAction>[
        const ContextMenuAction.separator(),
        ContextMenuAction(
          label: 'Künstler:in öffnen',
          icon: Icons.person_outline,
          onSelected: () => widget.onOpenArtist!(artist),
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
        onSelected: () => _controller.setFavorite(artist, !isFavorite),
      ),
    ];
  }

  Future<void> _showMenu(
    BuildContext context, {
    BuildContext? anchorContext,
    Offset? globalPosition,
    required List<ContextMenuAction> actions,
  }) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      anchorContext: anchorContext,
      globalPosition: globalPosition,
      actions: actions,
    );
  }

  @override
  Widget build(BuildContext context) {
    return CustomScrollView(
      slivers: <Widget>[
        const SliverPadding(padding: EdgeInsets.only(top: 18)),
        _sectionSliver(
          Text(
            'Favoriten',
            style: Theme.of(
              context,
            ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w700),
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: 20)),
        if (_controller.hasLoadError)
          _sectionSliver(const Text('Favoriten konnten nicht geladen werden.')),
        _sectionSliver(
          MediaTableSectionDesktop<Track>(
            title: 'Titel',
            startsTrackRadio: true,
            items: _controller.tracks,
            isLoading: _controller.isLoading,
            emptyText: 'Keine favorisierten Titel.',
            maxItemsPerPage: 10,
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            isRowFavorite: (Track item, int _) =>
                _controller.isItemFavorite(item),
            onRowFavoriteTap: (Track item, int _) => _controller.setFavorite(
              item,
              !_controller.isItemFavorite(item),
            ),
            onRowMoreAnchorTap: (Track item, int _, BuildContext anchor) =>
                _showMenu(
                  context,
                  anchorContext: anchor,
                  actions: _buildTrackMenuActions(item),
                ),
            onRowContextMenu: (Track item, int _, Offset position) => _showMenu(
              context,
              globalPosition: position,
              actions: _buildTrackMenuActions(item),
            ),
            onRowSelectionContextMenu: (List<Track> items, Offset position) =>
                _showMenu(
                  context,
                  globalPosition: position,
                  actions: _buildTrackMenuActions(items.first, items),
                ),
            onArtistTap: widget.onOpenArtist,
            onAlbumTap: widget.onOpenAlbum,
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: 20)),
        _sectionSliver(
          MediaRowSectionDesktop<Album>(
            title: 'Alben',
            isLoading: _controller.isLoading,
            items: _controller.albums,
            emptyText: 'Keine favorisierten Alben.',
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            isRowFavorite: (Album item, int _) =>
                _controller.isItemFavorite(item),
            onRowFavoriteTap: (Album item, int _) => _controller.setFavorite(
              item,
              !_controller.isItemFavorite(item),
            ),
            onRowMoreAnchorTap: (Album item, int _, BuildContext anchor) =>
                _showMenu(
                  context,
                  anchorContext: anchor,
                  actions: _buildAlbumMenuActions(item),
                ),
            onRowContextMenu: (Album item, int _, Offset position) => _showMenu(
              context,
              globalPosition: position,
              actions: _buildAlbumMenuActions(item),
            ),
            onArtistTap: widget.onOpenArtist,
            onAlbumTap: widget.onOpenAlbum,
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: 20)),
        _sectionSliver(
          MediaRowSectionDesktop<Playlist>(
            title: 'Playlists',
            isLoading: _controller.isLoading,
            items: _controller.playlists,
            emptyText: 'Keine favorisierten Playlists.',
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            isRowFavorite: (Playlist item, int _) =>
                _controller.isItemFavorite(item),
            onRowFavoriteTap: (Playlist item, int _) => _controller.setFavorite(
              item,
              !_controller.isItemFavorite(item),
            ),
            onRowMoreAnchorTap: (Playlist item, int _, BuildContext anchor) =>
                _showMenu(
                  context,
                  anchorContext: anchor,
                  actions: _buildPlaylistMenuActions(item),
                ),
            onRowContextMenu: (Playlist item, int _, Offset position) =>
                _showMenu(
                  context,
                  globalPosition: position,
                  actions: _buildPlaylistMenuActions(item),
                ),
            onPlaylistTap: widget.onOpenPlaylist,
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: 20)),
        _sectionSliver(
          MediaRowSectionDesktop<Artist>(
            title: 'Artists',
            isLoading: _controller.isLoading,
            items: _controller.artists,
            emptyText: 'Keine favorisierten Artists.',
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            isRowFavorite: (Artist item, int _) =>
                _controller.isItemFavorite(item),
            onRowFavoriteTap: (Artist item, int _) => _controller.setFavorite(
              item,
              !_controller.isItemFavorite(item),
            ),
            onRowMoreAnchorTap: (Artist item, int _, BuildContext anchor) =>
                _showMenu(
                  context,
                  anchorContext: anchor,
                  actions: _buildArtistMenuActions(item),
                ),
            onRowContextMenu: (Artist item, int _, Offset position) =>
                _showMenu(
                  context,
                  globalPosition: position,
                  actions: _buildArtistMenuActions(item),
                ),
            onArtistTap: widget.onOpenArtist,
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(bottom: 28)),
      ],
    );
  }

  Widget _sectionSliver(Widget child) {
    return SliverPadding(
      padding: const EdgeInsets.symmetric(horizontal: 20),
      sliver: SliverToBoxAdapter(child: child),
    );
  }
}
