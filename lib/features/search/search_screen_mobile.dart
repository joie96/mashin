import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_row_section_mobile.dart';
import '../shared/widgets/media_table_section_mobile.dart';
import 'search_controller.dart' as feature_search;

/// Mobile search page with local search input and stacked result sections.
class SearchScreenMobile extends StatefulWidget {
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const SearchScreenMobile({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  State<SearchScreenMobile> createState() => _SearchScreenMobileState();
}

class _SearchScreenMobileState extends State<SearchScreenMobile> {
  final TextEditingController _queryController = TextEditingController();
  final FocusNode _searchFocusNode = FocusNode();

  late final feature_search.SearchController _controller;

  @override
  void initState() {
    super.initState();
    _controller = feature_search.SearchController(
      coreBridge: widget.coreBridge,
      userDataController: widget.userDataController,
    )..addListener(_onControllerChanged);
    widget.userDataController?.addListener(_onUserDataChanged);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        _searchFocusNode.requestFocus();
      }
    });
  }

  void _onControllerChanged() {
    if (mounted) {
      setState(() {});
    }
  }

  @override
  void dispose() {
    _queryController.dispose();
    _searchFocusNode.dispose();
    _controller.removeListener(_onControllerChanged);
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
    final UserData? userData = widget.userDataController;
    final bool allFavorite = targets.every(
      (MediaItem target) => userData?.isFavorite(target) ?? target.favorite,
    );
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
      final Album? primaryAlbum = item is Track
          ? item.album
          : (item is Album ? item : null);
      final bool canOpenPlaylist =
          item is Playlist && widget.onOpenPlaylist != null;
      if ((primaryArtist != null && widget.onOpenArtist != null) ||
          (primaryAlbum != null && widget.onOpenAlbum != null) ||
          canOpenPlaylist) {
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
        if (canOpenPlaylist) {
          actions.add(
            ContextMenuAction(
              label: 'Wiedergabeliste öffnen',
              icon: Icons.queue_music_outlined,
              onSelected: () => widget.onOpenPlaylist!(item),
            ),
          );
        }
      }
    }

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
          isEnabled: userData != null,
          onSelected: () => userData?.setFavorite(targets, !allFavorite),
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
    final colors = context.mashinColors;

    return ListView(
      padding: const EdgeInsets.fromLTRB(18, 20, 18, 28),
      children: <Widget>[
        Text(
          'Suche',
          style: Theme.of(
            context,
          ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w700),
        ),
        const SizedBox(height: 20),
        TextField(
          controller: _queryController,
          focusNode: _searchFocusNode,
          textInputAction: TextInputAction.search,
          decoration: InputDecoration(
            hintText: 'Tracks, Alben, Playlists, Artists',
            prefixIcon: const Icon(Icons.search_outlined),
            suffixIcon: _queryController.text.isEmpty
                ? null
                : Row(
                    mainAxisSize: MainAxisSize.min,
                    children: <Widget>[
                      IconButton(
                        tooltip: 'Suchen',
                        onPressed: _submitSearch,
                        icon: const Icon(Icons.search_outlined),
                      ),
                      IconButton(
                        tooltip: 'Leeren',
                        onPressed: _clearSearch,
                        icon: const Icon(Icons.close_outlined),
                      ),
                    ],
                  ),
          ),
          onChanged: (_) => setState(() {}),
          onSubmitted: (_) => _submitSearch(),
          onTapOutside: (_) => FocusScope.of(context).unfocus(),
        ),
        const SizedBox(height: 20),
        if (!_controller.hasSearchRequest)
          Container(
            padding: const EdgeInsets.all(16),
            decoration: BoxDecoration(
              color: colors.bgSecondary,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: colors.border),
            ),
            child: Text(
              'Gib einen Suchbegriff ein, um Ergebnisse anzuzeigen.',
              style: TextStyle(color: colors.textSecondary),
            ),
          )
        else ...<Widget>[
          SizedBox(
            height: 234,
            child: CustomScrollView(
              primary: false,
              physics: const NeverScrollableScrollPhysics(),
              slivers: <Widget>[
                MediaTableSectionMobile<Track>(
                  title: 'Tracks',
                  startsTrackRadio: true,
                  items: _controller.tracks,
                  isLoading: _controller.isLoadingTracks,
                  emptyText: 'Keine Tracks gefunden.',
                  maxItemsPerPage: 3,
                  coreBridge: widget.coreBridge,
                  coreEvents: widget.coreEvents,
                  onSelectionMenuTap: (List<Track> items) =>
                      unawaited(_showSelectionMenu(items)),
                ),
              ],
            ),
          ),
          const SizedBox(height: 32),
          MediaRowSectionMobile<Album>(
            title: 'Alben',
            isLoading: _controller.isLoadingAlbums,
            items: _controller.albums,
            emptyText: 'Keine Alben gefunden.',
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            onItemTap: widget.onOpenAlbum,
            onSelectionMenuTap: (List<Album> items) =>
                unawaited(_showSelectionMenu(items)),
          ),
          const SizedBox(height: 32),
          MediaRowSectionMobile<Playlist>(
            title: 'Playlists',
            isLoading: _controller.isLoadingPlaylists,
            items: _controller.playlists,
            emptyText: 'Keine Playlists gefunden.',
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            onItemTap: widget.onOpenPlaylist,
            onSelectionMenuTap: (List<Playlist> items) =>
                unawaited(_showSelectionMenu(items)),
          ),
          const SizedBox(height: 32),
          MediaRowSectionMobile<Artist>(
            title: 'Artists',
            isLoading: _controller.isLoadingArtists,
            items: _controller.artists,
            emptyText: 'Keine Artists gefunden.',
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            onItemTap: widget.onOpenArtist,
            onSelectionMenuTap: (List<Artist> items) =>
                unawaited(_showSelectionMenu(items)),
          ),
        ],
      ],
    );
  }

  void _submitSearch() {
    unawaited(_controller.runSearch(_queryController.text.trim()));
  }

  void _clearSearch() {
    _queryController.clear();
    _controller.resetResults();
    setState(() {});
  }
}
