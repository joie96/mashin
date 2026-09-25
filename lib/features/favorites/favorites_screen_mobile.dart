import 'dart:async';

import 'package:flutter/material.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_row_section_mobile.dart';
import '../shared/widgets/media_table_section_mobile.dart';
import 'favorites_controller.dart';

/// Mobile favorites page: stacked track table plus album/playlist/artist rows.
/// Only tracks expose a tap-to-open context menu, mirroring the other migrated
/// mobile detail pages (album/artist/playlist).
class FavoritesScreenMobile extends StatefulWidget {
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const FavoritesScreenMobile({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    required this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  State<FavoritesScreenMobile> createState() => _FavoritesScreenMobileState();
}

class _FavoritesScreenMobileState extends State<FavoritesScreenMobile> {
  late final FavoritesController _controller;

  @override
  void initState() {
    super.initState();
    _controller = FavoritesController(
      coreBridge: widget.coreBridge,
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

  Artist? _primaryArtistOf(Track track) {
    for (final Artist artist in track.artists) {
      if (artist.itemId.trim().isNotEmpty &&
          artist.provider.trim().isNotEmpty) {
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
            onSelected: () => _controller.addTracksToPlaylist(
              playlist.itemId,
              <Track>[track],
            ),
          ),
        )
        .toList(growable: false);
  }

  List<ContextMenuAction> _buildTrackMenuActions(Track track) {
    final bool isFavorite = _controller.isItemFavorite(track);
    final Artist? primaryArtist = _primaryArtistOf(track);
    final Album? primaryAlbum = track.album;
    final List<ContextMenuAction> playlistActions = _buildPlaylistSubmenu(
      track,
    );

    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        icon: Icons.play_arrow,
        onSelected: () => widget.coreBridge.playMedia(<MediaItem>[track]),
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
        onSelected: () => widget.coreBridge.playMediaNext(<MediaItem>[track]),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => widget.coreBridge.playMediaLast(<MediaItem>[track]),
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
        onSelected: () => _controller.setFavorite(track, !isFavorite),
      ),
    ];
  }

  Future<void> _showTrackMenu(Track track) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      actions: _buildTrackMenuActions(track),
    );
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
        icon: Icons.play_arrow,
        onSelected: () => widget.coreBridge.playMedia(targets),
      ),
      if (item is Album || item is Playlist || item is Artist)
        ContextMenuAction(
          label: 'Zufällig abspielen',
          icon: Icons.shuffle_outlined,
          onSelected: () => widget.coreBridge.playShuffledMedia(targets),
        ),
      if (item is Track || item is Artist)
        ContextMenuAction(
          label: 'Radio starten',
          icon: Icons.radio_outlined,
          onSelected: () => item is Artist && isSingleTarget
              ? _controller.startArtistRadio(item)
              : widget.coreBridge.playSendspinRadioNext(targets),
        ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => widget.coreBridge.playMediaNext(targets),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => widget.coreBridge.playMediaLast(targets),
      ),
    ];

    if (isSingleTarget) {
      final Artist? primaryArtist = item is Artist
          ? item
          : (item is Track ? _primaryArtistOf(item) : null);
      final Album? primaryAlbum = item is Track
          ? item.album
          : (item is Album ? item : null);
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

    final List<ContextMenuAction> playlistActions = tracks.isEmpty
        ? const <ContextMenuAction>[]
        : _controller.playlistsForAdding
              .map(
                (Playlist playlist) => ContextMenuAction(
                  label: playlist.name,
                  icon: Icons.playlist_play_outlined,
                  onSelected: () =>
                      _controller.addTracksToPlaylist(playlist.itemId, tracks),
                ),
              )
              .toList(growable: false);
    if (playlistActions.isNotEmpty) {
      actions
        ..add(const ContextMenuAction.separator())
        ..add(
          ContextMenuAction.submenu(
            label: 'Zu Wiedergabeliste hinzufügen',
            icon: Icons.playlist_add_outlined,
            children: playlistActions,
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
    return ListView(
      padding: const EdgeInsets.fromLTRB(18, 20, 18, 28),
      children: <Widget>[
        Text(
          'Favoriten',
          style: Theme.of(
            context,
          ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w700),
        ),
        const SizedBox(height: 20),
        SizedBox(
          height: 234,
          child: CustomScrollView(
            primary: false,
            physics: const NeverScrollableScrollPhysics(),
            slivers: <Widget>[
              MediaTableSectionMobile<Track>(
                title: 'Titel',
                items: _controller.tracks,
                isLoading: _controller.isLoading,
                emptyText: 'Keine favorisierten Titel.',
                maxItemsPerPage: 3,
                coreBridge: widget.coreBridge,
                coreEvents: widget.coreEvents,
                onRowTap: (Track item, int _) =>
                    unawaited(_showTrackMenu(item)),
                onSelectionMenuTap: (List<Track> items) =>
                    unawaited(_showSelectionMenu(items)),
              ),
            ],
          ),
        ),
        const SizedBox(height: 32),
        MediaRowSectionMobile<Album>(
          title: 'Alben',
          isLoading: _controller.isLoading,
          items: _controller.albums,
          emptyText: 'Keine favorisierten Alben.',
          coreBridge: widget.coreBridge,
          coreEvents: widget.coreEvents,
          onItemTap: widget.onOpenAlbum,
          onSelectionMenuTap: (List<Album> items) =>
              unawaited(_showSelectionMenu(items)),
        ),
        const SizedBox(height: 32),
        MediaRowSectionMobile<Playlist>(
          title: 'Playlists',
          isLoading: _controller.isLoading,
          items: _controller.playlists,
          emptyText: 'Keine favorisierten Playlists.',
          coreBridge: widget.coreBridge,
          coreEvents: widget.coreEvents,
          onItemTap: widget.onOpenPlaylist,
          onSelectionMenuTap: (List<Playlist> items) =>
              unawaited(_showSelectionMenu(items)),
        ),
        const SizedBox(height: 32),
        MediaRowSectionMobile<Artist>(
          title: 'Artists',
          isLoading: _controller.isLoading,
          items: _controller.artists,
          emptyText: 'Keine favorisierten Artists.',
          coreBridge: widget.coreBridge,
          coreEvents: widget.coreEvents,
          onItemTap: widget.onOpenArtist,
          onSelectionMenuTap: (List<Artist> items) =>
              unawaited(_showSelectionMenu(items)),
        ),
      ],
    );
  }
}
