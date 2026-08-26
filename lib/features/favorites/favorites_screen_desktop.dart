import 'package:flutter/material.dart';

import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../shared/userdata_controller.dart';
import '../shared/widgets/media_row_section_desktop.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import 'favorites_controller.dart';

/// Desktop favorites page: stacked track table plus album/playlist/artist rows,
/// each with its own context menu.
class FavoritesScreenDesktop extends StatefulWidget {
  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final UserDataController userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const FavoritesScreenDesktop({
    super.key,
    required this.client,
    required this.playbackCoordinator,
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
      client: widget.client,
      playbackCoordinator: widget.playbackCoordinator,
      userDataController: widget.userDataController,
    )..addListener(_onControllerChanged);
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
      if (artist.itemId.trim().isNotEmpty && artist.provider.trim().isNotEmpty) {
        return artist;
      }
    }
    return null;
  }

  List<ContextMenuAction> _buildPlaylistSubmenu(Track track) {
    return _controller.playlistsForAdding
        .map(
          (Playlist playlist) => ContextMenuAction(
            label: playlist.name,
            icon: Icons.playlist_play_outlined,
            onSelected: () => _controller.addTracksToPlaylist(playlist.itemId, <Track>[track]),
          ),
        )
        .toList(growable: false);
  }

  List<ContextMenuAction> _buildTrackMenuActions(Track track) {
    final bool isFavorite = _controller.isItemFavorite(track);
    final PlaybackCoordinator coordinator = widget.playbackCoordinator;
    final Artist? primaryArtist = _primaryArtistOf(track);
    final Album? primaryAlbum = track.album;
    final List<ContextMenuAction> playlistActions = _buildPlaylistSubmenu(track);

    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        icon: Icons.play_arrow,
        onSelected: () => coordinator.playMedia(<MediaItem>[track]),
      ),
      ContextMenuAction(
        label: 'Radio starten',
        icon: Icons.radio_outlined,
        onSelected: () => _controller.startTrackRadio(track),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => coordinator.playMediaNext(<MediaItem>[track]),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => coordinator.playMediaLast(<MediaItem>[track]),
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
        label: isFavorite ? 'Aus Favoriten entfernen' : 'Zu Favoriten hinzufügen',
        icon: isFavorite ? Icons.favorite_outlined : Icons.favorite_border_outlined,
        isDestructive: isFavorite,
        onSelected: () => _controller.setFavorite(track, !isFavorite),
      ),
    ];
  }

  List<ContextMenuAction> _buildAlbumMenuActions(Album album) {
    final bool isFavorite = _controller.isItemFavorite(album);
    final PlaybackCoordinator coordinator = widget.playbackCoordinator;
    final Artist? primaryArtist = _primaryArtistOf(album);

    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        icon: Icons.play_arrow,
        onSelected: () => coordinator.playMedia(<MediaItem>[album]),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => coordinator.playMediaNext(<MediaItem>[album]),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => coordinator.playMediaLast(<MediaItem>[album]),
      ),
      if ((primaryArtist != null && widget.onOpenArtist != null) || widget.onOpenAlbum != null) ...<ContextMenuAction>[
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
        label: isFavorite ? 'Aus Favoriten entfernen' : 'Zu Favoriten hinzufügen',
        icon: isFavorite ? Icons.favorite_outlined : Icons.favorite_border_outlined,
        isDestructive: isFavorite,
        onSelected: () => _controller.setFavorite(album, !isFavorite),
      ),
    ];
  }

  List<ContextMenuAction> _buildPlaylistMenuActions(Playlist playlist) {
    final bool isFavorite = _controller.isItemFavorite(playlist);
    final PlaybackCoordinator coordinator = widget.playbackCoordinator;

    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        icon: Icons.play_arrow,
        onSelected: () => coordinator.playMedia(<MediaItem>[playlist]),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => coordinator.playMediaNext(<MediaItem>[playlist]),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => coordinator.playMediaLast(<MediaItem>[playlist]),
      ),
      const ContextMenuAction.separator(),
      ContextMenuAction(
        label: isFavorite ? 'Aus Favoriten entfernen' : 'Zu Favoriten hinzufügen',
        icon: isFavorite ? Icons.favorite_outlined : Icons.favorite_border_outlined,
        isDestructive: isFavorite,
        onSelected: () => _controller.setFavorite(playlist, !isFavorite),
      ),
    ];
  }

  List<ContextMenuAction> _buildArtistMenuActions(Artist artist) {
    final bool isFavorite = _controller.isItemFavorite(artist);
    final PlaybackCoordinator coordinator = widget.playbackCoordinator;

    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        icon: Icons.play_arrow,
        onSelected: () => coordinator.playMedia(<MediaItem>[artist]),
      ),
      ContextMenuAction(
        label: 'Radio starten',
        icon: Icons.radio_outlined,
        onSelected: () => _controller.startArtistRadio(artist),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => coordinator.playMediaNext(<MediaItem>[artist]),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => coordinator.playMediaLast(<MediaItem>[artist]),
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
        label: isFavorite ? 'Aus Favoriten entfernen' : 'Zu Favoriten hinzufügen',
        icon: isFavorite ? Icons.favorite_outlined : Icons.favorite_border_outlined,
        isDestructive: isFavorite,
        onSelected: () => _controller.setFavorite(artist, !isFavorite),
      ),
    ];
  }

  Future<void> _showMenu(BuildContext context, {BuildContext? anchorContext, Offset? globalPosition, required List<ContextMenuAction> actions}) {
    return const AppContextMenuController().show(
      context: context,
      anchorContext: anchorContext,
      globalPosition: globalPosition,
      actions: actions,
    );
  }

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.fromLTRB(20, 18, 20, 28),
      children: <Widget>[
        Text(
          'Favoriten',
          style: Theme.of(context).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w700),
        ),
        const SizedBox(height: 20),
        MediaTableSectionDesktop<Track>(
          title: 'Titel',
          items: _controller.tracks,
          isLoading: _controller.isLoading,
          emptyText: 'Keine favorisierten Titel.',
          maxItemsPerPage: 10,
          playbackCoordinator: widget.playbackCoordinator,
          isRowFavorite: (Track item, int _) => _controller.isItemFavorite(item),
          onRowFavoriteTap: (Track item, int _) => _controller.setFavorite(item, !_controller.isItemFavorite(item)),
          onRowMoreAnchorTap: (Track item, int _, BuildContext anchor) =>
              _showMenu(context, anchorContext: anchor, actions: _buildTrackMenuActions(item)),
          onRowContextMenu: (Track item, int _, Offset position) =>
              _showMenu(context, globalPosition: position, actions: _buildTrackMenuActions(item)),
          onArtistTap: widget.onOpenArtist,
          onAlbumTap: widget.onOpenAlbum,
        ),
        const SizedBox(height: 20),
        MediaRowSectionDesktop<Album>(
          title: 'Alben',
          isLoading: _controller.isLoading,
          items: _controller.albums,
          emptyText: 'Keine favorisierten Alben.',
          playbackCoordinator: widget.playbackCoordinator,
          isRowFavorite: (Album item, int _) => _controller.isItemFavorite(item),
          onRowFavoriteTap: (Album item, int _) => _controller.setFavorite(item, !_controller.isItemFavorite(item)),
          onRowMoreAnchorTap: (Album item, int _, BuildContext anchor) =>
              _showMenu(context, anchorContext: anchor, actions: _buildAlbumMenuActions(item)),
          onRowContextMenu: (Album item, int _, Offset position) =>
              _showMenu(context, globalPosition: position, actions: _buildAlbumMenuActions(item)),
          onArtistTap: widget.onOpenArtist,
          onAlbumTap: widget.onOpenAlbum,
        ),
        const SizedBox(height: 20),
        MediaRowSectionDesktop<Playlist>(
          title: 'Playlists',
          isLoading: _controller.isLoading,
          items: _controller.playlists,
          emptyText: 'Keine favorisierten Playlists.',
          playbackCoordinator: widget.playbackCoordinator,
          isRowFavorite: (Playlist item, int _) => _controller.isItemFavorite(item),
          onRowFavoriteTap: (Playlist item, int _) => _controller.setFavorite(item, !_controller.isItemFavorite(item)),
          onRowMoreAnchorTap: (Playlist item, int _, BuildContext anchor) =>
              _showMenu(context, anchorContext: anchor, actions: _buildPlaylistMenuActions(item)),
          onRowContextMenu: (Playlist item, int _, Offset position) =>
              _showMenu(context, globalPosition: position, actions: _buildPlaylistMenuActions(item)),
          onPlaylistTap: widget.onOpenPlaylist,
        ),
        const SizedBox(height: 20),
        MediaRowSectionDesktop<Artist>(
          title: 'Artists',
          isLoading: _controller.isLoading,
          items: _controller.artists,
          emptyText: 'Keine favorisierten Artists.',
          playbackCoordinator: widget.playbackCoordinator,
          isRowFavorite: (Artist item, int _) => _controller.isItemFavorite(item),
          onRowFavoriteTap: (Artist item, int _) => _controller.setFavorite(item, !_controller.isItemFavorite(item)),
          onRowMoreAnchorTap: (Artist item, int _, BuildContext anchor) =>
              _showMenu(context, anchorContext: anchor, actions: _buildArtistMenuActions(item)),
          onRowContextMenu: (Artist item, int _, Offset position) =>
              _showMenu(context, globalPosition: position, actions: _buildArtistMenuActions(item)),
          onArtistTap: widget.onOpenArtist,
        ),
      ],
    );
  }
}
