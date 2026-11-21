import 'dart:async';

import 'package:flutter/material.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../core/core_userdata.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../shared/widgets/media_list_section_mobile.dart';
import '../shared/widgets/media_row_section_mobile.dart';
import '../shared/widgets/mood_playlist_cover.dart';
import 'explore_controller.dart';

class ExploreScreenMobile extends StatefulWidget {
  const ExploreScreenMobile({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  @override
  State<ExploreScreenMobile> createState() => _ExploreScreenMobileState();
}

class _ExploreScreenMobileState extends State<ExploreScreenMobile> {
  late ExploreController _controller;
  bool _isPageVisible = true;

  // Controller lifecycle keeps observations active while the route is covered.
  @override
  void initState() {
    super.initState();
    _initializeController();
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
  void didUpdateWidget(covariant ExploreScreenMobile oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.userDataController != widget.userDataController ||
        oldWidget.coreBridge != widget.coreBridge) {
      oldWidget.userDataController?.favoritesChanges.removeListener(
        _onUserDataChanged,
      );
      _controller.dispose();
      _initializeController();
    }
  }

  @override
  void dispose() {
    widget.userDataController?.favoritesChanges.removeListener(
      _onUserDataChanged,
    );
    _controller.dispose();
    super.dispose();
  }

  void _initializeController() {
    _controller = ExploreController(
      coreBridge: widget.coreBridge,
      userDataController: widget.userDataController,
    );
    widget.userDataController?.favoritesChanges.addListener(_onUserDataChanged);
    final controller = _controller;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && identical(controller, _controller))
        unawaited(controller.load());
    });
  }

  // Media navigation and commands.
  void _openItem(MediaItem item) {
    switch (item) {
      case Playlist playlist:
        widget.onOpenPlaylist?.call(playlist);
      case Album album:
        widget.onOpenAlbum?.call(album);
      case Artist artist:
        widget.onOpenArtist?.call(artist);
      case Track track:
        final album = track.album;
        if (album != null) widget.onOpenAlbum?.call(album);
      default:
        break;
    }
  }

  Future<void> _showMediaMenu(
    List<MediaItem> items, {
    BuildContext? anchor,
    Offset? position,
  }) async {
    if (items.isEmpty) return;
    final hasPlayableItems = items.any((item) => item.canPlay);
    final areAllFavorites = items.every((item) => item.favorite);
    final tracks = items.whereType<Track>().toList(growable: false);
    final userData = widget.userDataController;
    await const AppContextMenuController().showContextMenu(
      context: context,
      anchorContext: anchor,
      globalPosition: position,
      actions: [
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
                  (playlist) => ContextMenuAction(
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
            for (final item in items) {
              await _controller.setFavorite(item, !areAllFavorites);
            }
          },
        ),
      ],
    );
  }

  // Mobile page layout and typed media sections.
  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: _controller,
    builder: (context, child) => CustomScrollView(
      slivers: [
        const SliverPadding(padding: EdgeInsets.only(top: 20)),
        _sectionSliver(
          Text(
            'Entdecken',
            style: Theme.of(
              context,
            ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w700),
          ),
        ),
        if (_controller.hasLoadError)
          _sectionSliver(
            const Padding(
              padding: EdgeInsets.only(top: 12),
              child: Text('Empfehlungen konnten nicht geladen werden.'),
            ),
          ),
        const SliverPadding(padding: EdgeInsets.only(top: 20)),
        _sectionSliver(
          _mediaRow<Playlist>(
            'global_artist_radio_playlists',
            'Das hören andere',
            _controller.itemsForFolder<Playlist>(
              'global_artist_radio_playlists',
            ),
            'Keine Mixe verfügbar.',
          ),
        ),
        const SliverPadding(padding: EdgeInsets.only(top: 32)),
        _sectionSliver(
          _trackCharts('global_top_tracks', 'Track-Charts weltweit'),
        ),
        for (final section in <Widget>[
          _mediaRow<Artist>(
            'global_top_artists',
            'Künstler-Charts weltweit',
            _controller.itemsForFolder<Artist>('global_top_artists'),
            'Keine Artist-Charts verfügbar.',
          ),
          _trackCharts('de_top_tracks', 'Track-Charts Deutschland'),
          _mediaRow<Artist>(
            'de_top_artists',
            'Künstler-Charts Deutschland',
            _controller.itemsForFolder<Artist>('de_top_artists'),
            'Keine Künstler-Charts verfügbar.',
          ),
          _mediaRow<Playlist>(
            'global_mood_playlists',
            'Musik für deine Stimmung',
            _controller.itemsForFolder<Playlist>('global_mood_playlists'),
            'Keine Stimmungs-Playlists verfügbar.',
            playlistCoverBuilder: buildMoodPlaylistCover,
          ),
          _genrePlaylists(),
          for (final folder in _controller.foldersWithPrefix(
            'global_genre_artists_',
          ))
            _mediaRow<Artist>(
              folder.itemId,
              folder.title,
              folder.items.whereType<Artist>().toList(growable: false),
              'Keine Künstler:innen verfügbar.',
            ),
          _mediaRow<Playlist>(
            'global_decade_playlists',
            'Jahrzehnt-Playlists',
            _controller.itemsForFolder<Playlist>('global_decade_playlists'),
            'Keine Jahrzehnt-Playlists verfügbar.',
          ),
          _mediaRow<Album>(
            'global_popular_artist_albums',
            'Alben weltweit beliebter Künstler:innen',
            _controller.itemsForFolder<Album>('global_popular_artist_albums'),
            'Keine Alben verfügbar.',
          ),
          _mediaRow<Album>(
            'de_popular_artist_albums',
            'Alben in Deutschland beliebter Künstler:innen',
            _controller.itemsForFolder<Album>('de_popular_artist_albums'),
            'Keine Alben verfügbar.',
          ),
          for (final folder in _controller.foldersWithPrefix(
            'global_genre_albums_',
          ))
            _mediaRow<Album>(
              folder.itemId,
              folder.title,
              folder.items.whereType<Album>().toList(growable: false),
              'Keine Alben verfügbar.',
            ),
        ]) ...[
          const SliverPadding(padding: EdgeInsets.only(top: 32)),
          _sectionSliver(section),
        ],
        const SliverPadding(padding: EdgeInsets.only(bottom: 28)),
      ],
    ),
  );

  Widget _sectionSliver(Widget child) => SliverPadding(
    padding: const EdgeInsets.symmetric(horizontal: 18),
    sliver: SliverToBoxAdapter(child: child),
  );

  Widget _trackCharts(String folderId, String title) =>
      MediaListSectionMobile<Track>(
        title: title,
        items: _controller.itemsForFolder<Track>(folderId),
        isLoading: _controller.isFolderLoading(folderId),
        emptyText: 'Keine Track-Charts verfügbar.',
        coreBridge: widget.coreBridge,
        coreEvents: widget.coreEvents,
        onItemTap: (item) => unawaited(_showMediaMenu([item])),
        onSelectionMenuTap: (items) => unawaited(_showMediaMenu(items)),
      );

  Widget _mediaRow<T extends MediaItem>(
    String folderId,
    String title,
    List<T> items,
    String emptyText, {
    Widget? Function(Playlist playlist)? playlistCoverBuilder,
  }) => MediaRowSectionMobile<T>(
    title: title,
    items: items,
    emptyText: emptyText,
    isLoading: _controller.isFolderLoading(folderId),
    coreBridge: widget.coreBridge,
    coreEvents: widget.coreEvents,
    onItemTap: _openItem,
    onSelectionMenuTap: (items) => unawaited(_showMediaMenu(items)),
    showPlaylistDetails: false,
    playlistCoverBuilder: playlistCoverBuilder,
  );

  Widget _genrePlaylists() => MediaListSectionMobile<Playlist>(
    title: 'Genres entdecken',
    items: _controller.itemsForFolder<Playlist>('global_genre_playlists'),
    isLoading: _controller.isFolderLoading('global_genre_playlists'),
    emptyText: 'Keine Genre-Playlists verfügbar.',
    coreBridge: widget.coreBridge,
    onItemTap: widget.onOpenPlaylist,
    onSelectionMenuTap: (items) => unawaited(_showMediaMenu(items)),
  );

  // Favorite changes refresh only the controller's rendered media projection.
  void _onUserDataChanged() => _controller.refreshFavoriteStatuses();
}
