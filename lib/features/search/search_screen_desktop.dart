import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_row_section_desktop.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import 'search_controller.dart' as feature_search;

/// Desktop search page driven by the shell top-bar query.
class SearchScreenDesktop extends StatefulWidget {
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData? userDataController;
  final bool desktopSearchDriven;
  final String desktopSearchQuery;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const SearchScreenDesktop({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    this.userDataController,
    this.desktopSearchDriven = false,
    this.desktopSearchQuery = '',
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  State<SearchScreenDesktop> createState() => _SearchScreenDesktopState();
}

class _SearchScreenDesktopState extends State<SearchScreenDesktop> {
  late final feature_search.SearchController _controller;

  @override
  void initState() {
    super.initState();
    _controller = feature_search.SearchController(
      coreBridge: widget.coreBridge,
      userDataController: widget.userDataController,
    )..addListener(_onControllerChanged);
    widget.userDataController?.addListener(_onUserDataChanged);

    if (widget.desktopSearchDriven) {
      _syncDesktopQuery(widget.desktopSearchQuery);
    }
  }

  @override
  void didUpdateWidget(covariant SearchScreenDesktop oldWidget) {
    super.didUpdateWidget(oldWidget);

    if (!widget.desktopSearchDriven) {
      return;
    }

    if (oldWidget.desktopSearchQuery != widget.desktopSearchQuery) {
      _syncDesktopQuery(widget.desktopSearchQuery);
    }
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
    widget.userDataController?.removeListener(_onUserDataChanged);
    super.dispose();
  }

  void _onUserDataChanged() {
    _controller.refreshFavoriteStatuses();
  }

  bool _isItemFavorite(MediaItem item) => item.favorite;

  void _toggleFavorite(MediaItem item) {
    final UserData? userDataController = widget.userDataController;
    if (userDataController == null) {
      return;
    }
    unawaited(
      userDataController.setFavorite(<MediaItem>[item], !_isItemFavorite(item)),
    );
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

  List<ContextMenuAction> _buildPlaylistSubmenu(List<Track> tracks) {
    final UserData? controller = widget.userDataController;
    if (controller == null) {
      return const <ContextMenuAction>[];
    }

    return controller.playlists
        .map(
          (Playlist playlist) => ContextMenuAction(
            label: playlist.name,
            icon: Icons.playlist_play_outlined,
            onSelected: () =>
                controller.addPlaylistTracks(playlist.itemId, tracks),
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
    final bool isFavorite = _isItemFavorite(track);
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
      if ((primaryArtist != null && widget.onOpenArtist != null) ||
          (primaryAlbum != null &&
              widget.onOpenAlbum != null)) ...<ContextMenuAction>[
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
            _toggleFavorite(selectedTrack);
          }
        },
      ),
    ];
  }

  List<ContextMenuAction> _buildAlbumMenuActions(Album album) {
    final bool isFavorite = _isItemFavorite(album);
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
        onSelected: () => _toggleFavorite(album),
      ),
    ];
  }

  List<ContextMenuAction> _buildPlaylistMenuActions(Playlist playlist) {
    final bool isFavorite = _isItemFavorite(playlist);

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
      if (widget.onOpenPlaylist != null) ...<ContextMenuAction>[
        const ContextMenuAction.separator(),
        ContextMenuAction(
          label: 'Wiedergabeliste öffnen',
          icon: Icons.queue_music_outlined,
          onSelected: () => widget.onOpenPlaylist!(playlist),
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
        onSelected: () => _toggleFavorite(playlist),
      ),
    ];
  }

  List<ContextMenuAction> _buildArtistMenuActions(Artist artist) {
    final bool isFavorite = _isItemFavorite(artist);

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
        onSelected: () => widget.coreBridge.startRadio(<MediaItem>[artist]),
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
        onSelected: () => _toggleFavorite(artist),
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
    final colors = context.mashinColors;

    return CustomScrollView(
      slivers: <Widget>[
        const SliverPadding(padding: EdgeInsets.only(top: 18)),
        _sectionSliver(
          Text(
            'Suche',
            style: Theme.of(
              context,
            ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w700),
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: 20)),
        if (!_controller.hasSearchRequest)
          _sectionSliver(
            Container(
              padding: const EdgeInsets.all(16),
              decoration: BoxDecoration(
                color: colors.bgSecondary,
                borderRadius: BorderRadius.circular(12),
                border: Border.all(color: colors.border),
              ),
              child: Text(
                'Nutze die Suchleiste oben, um Ergebnisse anzuzeigen.',
                style: TextStyle(color: colors.textSecondary),
              ),
            ),
          )
        else ...<Widget>[
          _sectionSliver(
            MediaTableSectionDesktop<Track>(
              title: 'Tracks',
              startsTrackRadio: true,
              items: _controller.tracks,
              isLoading: _controller.isLoadingTracks,
              emptyText: 'Keine Tracks gefunden.',
              maxItemsPerPage: 10,
              coreBridge: widget.coreBridge,
              coreEvents: widget.coreEvents,
              isRowFavorite: (Track item, int _) => _isItemFavorite(item),
              onRowFavoriteTap: widget.userDataController == null
                  ? null
                  : (Track item, int _) => _toggleFavorite(item),
              onRowMoreAnchorTap:
                  (Track item, int _, BuildContext anchorContext) => _showMenu(
                    context,
                    anchorContext: anchorContext,
                    actions: _buildTrackMenuActions(item),
                  ),
              onRowContextMenu: (Track item, int _, Offset globalPosition) =>
                  _showMenu(
                    context,
                    globalPosition: globalPosition,
                    actions: _buildTrackMenuActions(item),
                  ),
              onRowSelectionContextMenu:
                  (List<Track> items, Offset globalPosition) => _showMenu(
                    context,
                    globalPosition: globalPosition,
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
              isLoading: _controller.isLoadingAlbums,
              items: _controller.albums,
              emptyText: 'Keine Alben gefunden.',
              coreBridge: widget.coreBridge,
              coreEvents: widget.coreEvents,
              isRowFavorite: (Album item, int _) => _isItemFavorite(item),
              onRowFavoriteTap: widget.userDataController == null
                  ? null
                  : (Album item, int _) => _toggleFavorite(item),
              onRowMoreAnchorTap:
                  (Album item, int _, BuildContext anchorContext) => _showMenu(
                    context,
                    anchorContext: anchorContext,
                    actions: _buildAlbumMenuActions(item),
                  ),
              onRowContextMenu: (Album item, int _, Offset globalPosition) =>
                  _showMenu(
                    context,
                    globalPosition: globalPosition,
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
              isLoading: _controller.isLoadingPlaylists,
              items: _controller.playlists,
              emptyText: 'Keine Playlists gefunden.',
              coreBridge: widget.coreBridge,
              coreEvents: widget.coreEvents,
              isRowFavorite: (Playlist item, int _) => _isItemFavorite(item),
              onRowFavoriteTap: widget.userDataController == null
                  ? null
                  : (Playlist item, int _) => _toggleFavorite(item),
              onRowMoreAnchorTap:
                  (Playlist item, int _, BuildContext anchorContext) =>
                      _showMenu(
                        context,
                        anchorContext: anchorContext,
                        actions: _buildPlaylistMenuActions(item),
                      ),
              onRowContextMenu: (Playlist item, int _, Offset globalPosition) =>
                  _showMenu(
                    context,
                    globalPosition: globalPosition,
                    actions: _buildPlaylistMenuActions(item),
                  ),
              onPlaylistTap: widget.onOpenPlaylist,
            ),
          ),
          const SliverPadding(padding: EdgeInsets.only(top: 20)),
          _sectionSliver(
            MediaRowSectionDesktop<Artist>(
              title: 'Artists',
              isLoading: _controller.isLoadingArtists,
              items: _controller.artists,
              emptyText: 'Keine Artists gefunden.',
              coreBridge: widget.coreBridge,
              coreEvents: widget.coreEvents,
              isRowFavorite: (Artist item, int _) => _isItemFavorite(item),
              onRowFavoriteTap: widget.userDataController == null
                  ? null
                  : (Artist item, int _) => _toggleFavorite(item),
              onRowMoreAnchorTap:
                  (Artist item, int _, BuildContext anchorContext) => _showMenu(
                    context,
                    anchorContext: anchorContext,
                    actions: _buildArtistMenuActions(item),
                  ),
              onRowContextMenu: (Artist item, int _, Offset globalPosition) =>
                  _showMenu(
                    context,
                    globalPosition: globalPosition,
                    actions: _buildArtistMenuActions(item),
                  ),
              onArtistTap: widget.onOpenArtist,
            ),
          ),
        ],
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

  void _syncDesktopQuery(String query) {
    unawaited(_controller.runSearch(query.trim()));
  }
}
