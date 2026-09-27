import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_link_text.dart';
import '../shared/widgets/media_row_section_desktop.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import '../shared/widgets/media_image.dart';
import 'album_detail_controller.dart';

class AlbumDetailPageDesktop extends StatefulWidget {
  const AlbumDetailPageDesktop({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
  });

  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

  @override
  State<AlbumDetailPageDesktop> createState() => _AlbumDetailPageDesktopState();
}

class _AlbumDetailPageDesktopState extends State<AlbumDetailPageDesktop> {
  static const double _coverSize = 200;

  late final AlbumDetailController _controller;
  final GlobalKey _headerMenuAnchorKey = GlobalKey();

  @override
  void initState() {
    super.initState();
    _controller = AlbumDetailController(
      coreBridge: widget.coreBridge,
      itemId: widget.itemId,
      providerInstanceIdOrDomain: widget.providerInstanceIdOrDomain,
      userDataController: widget.userDataController,
    );
    unawaited(_controller.loadAlbum());
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  List<ContextMenuAction> _buildPlaylistSubmenu(List<Track> tracks) {
    final UserData? controller = widget.userDataController;
    if (controller == null || tracks.isEmpty) {
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

  List<ContextMenuAction> _buildItemMenuActions(
    MediaItem item, [
    List<Track> selectedTracks = const <Track>[],
  ]) {
    final List<Track> tracks = selectedTracks.isEmpty && item is Track
        ? <Track>[item]
        : selectedTracks;
    final bool isFavorite = _controller.isItemFavorite(item);
    final List<ContextMenuAction> actions = <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        icon: Icons.play_arrow,
        onSelected: () => widget.coreBridge.playMedia(
          tracks.isEmpty ? <MediaItem>[item] : tracks,
        ),
      ),
      if (item is Album || item is Playlist)
        ContextMenuAction(
          label: 'Zufällig abspielen',
          icon: Icons.shuffle_outlined,
          onSelected: () =>
              widget.coreBridge.playShuffledMedia(<MediaItem>[item]),
        ),
      ContextMenuAction(
        label: 'Radio starten',
        icon: Icons.radio_outlined,
        onSelected: () => item is Track
            ? widget.coreBridge.startSendspinRadio(
                tracks.isEmpty ? <MediaItem>[item] : tracks,
              )
            : _controller.startItemRadio(item),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => widget.coreBridge.playMediaNext(
          tracks.isEmpty ? <MediaItem>[item] : tracks,
        ),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => widget.coreBridge.playMediaLast(
          tracks.isEmpty ? <MediaItem>[item] : tracks,
        ),
      ),
    ];

    final Artist? primaryArtist = _primaryArtistOf(item);
    final Album? primaryAlbum = item is Track
        ? item.album
        : (item is Album ? item : null);
    final bool hasNavigation =
        (primaryArtist != null && widget.onOpenArtist != null) ||
        primaryAlbum != null ||
        item is Artist;

    if (hasNavigation) {
      actions.add(const ContextMenuAction.separator());
      if (item is Artist && widget.onOpenArtist != null) {
        actions.add(
          ContextMenuAction(
            label: 'Künstler:in öffnen',
            icon: Icons.person_outline,
            onSelected: () => widget.onOpenArtist!(item),
          ),
        );
      } else if (primaryArtist != null && widget.onOpenArtist != null) {
        actions.add(
          ContextMenuAction(
            label: 'Künstler:in öffnen',
            icon: Icons.person_outline,
            onSelected: () => widget.onOpenArtist!(primaryArtist),
          ),
        );
      }
      if (primaryAlbum != null) {
        actions.add(
          ContextMenuAction(
            label: 'Album öffnen',
            icon: Icons.album_outlined,
            onSelected: () => widget.onOpenAlbum?.call(primaryAlbum),
          ),
        );
      }
    }

    final List<ContextMenuAction> playlistActions = _buildPlaylistSubmenu(
      tracks,
    );
    if (playlistActions.isNotEmpty) {
      actions.add(const ContextMenuAction.separator());
      actions.add(
        ContextMenuAction.submenu(
          label: 'Zu Wiedergabeliste hinzufügen',
          icon: Icons.playlist_add_outlined,
          children: playlistActions,
        ),
      );
    }

    actions.add(const ContextMenuAction.separator());
    actions.add(
      ContextMenuAction(
        label: isFavorite
            ? 'Aus Favoriten entfernen'
            : 'Zu Favoriten hinzufügen',
        icon: isFavorite
            ? Icons.favorite_outlined
            : Icons.favorite_border_outlined,
        onSelected: () => _controller.toggleItemFavorite(item),
      ),
    );
    return actions;
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

  List<ContextMenuAction> _buildHeaderMenuActions(Album album) {
    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => widget.coreBridge.playMediaNext(<MediaItem>[album]),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => widget.coreBridge.playMediaLast(<MediaItem>[album]),
      ),
      const ContextMenuAction.separator(),
      ContextMenuAction(
        label: _controller.isAlbumFavorite
            ? 'Aus Favoriten entfernen'
            : 'Zu Favoriten hinzufügen',
        icon: _controller.isAlbumFavorite
            ? Icons.favorite_outlined
            : Icons.favorite_border_outlined,
        onSelected: _controller.toggleAlbumFavorite,
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

  Future<void> _showItemMenuAtPosition(
    MediaItem item,
    Offset position, [
    List<Track> selectedTracks = const <Track>[],
  ]) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      globalPosition: position,
      actions: _buildItemMenuActions(item, selectedTracks),
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return Container(
      color: colors.bgPrimary,
      child: CustomScrollView(
        slivers: <Widget>[
          SliverToBoxAdapter(
            child: ListenableBuilder(
              listenable: Listenable.merge(<Listenable>[
                _controller.metadataListenable,
                _controller.tracksListenable,
              ]),
              builder: (BuildContext context, Widget? child) =>
                  _buildHeader(context, colors),
            ),
          ),
          SliverToBoxAdapter(
            child: ListenableBuilder(
              listenable: _controller.metadataListenable,
              builder: (BuildContext context, Widget? child) {
                if (!_controller.isLoadingMetadata &&
                    !_controller.hasDescription) {
                  return const SizedBox.shrink();
                }
                return Padding(
                  padding: const EdgeInsets.fromLTRB(24, 12, 24, 0),
                  child: _buildDescription(context, colors),
                );
              },
            ),
          ),
          SliverPadding(
            padding: const EdgeInsets.fromLTRB(24, 12, 24, 0),
            sliver: SliverToBoxAdapter(
              child: ListenableBuilder(
                listenable: _controller.tracksListenable,
                builder: (BuildContext context, Widget? child) =>
                    MediaTableSectionDesktop<Track>(
                      title: 'Titel',
                      isLoading: _controller.isLoadingTracks,
                      items: _controller.tracks,
                      emptyText: 'Keine Titel gefunden.',
                      maxItemsPerPage: 15,
                      coreBridge: widget.coreBridge,
                      coreEvents: widget.coreEvents,
                      isRowFavorite: (Track item, int _) =>
                          _controller.isItemFavorite(item),
                      onRowFavoriteTap: (Track item, int _) =>
                          _controller.toggleItemFavorite(item),
                      onRowMoreAnchorTap:
                          (Track item, int _, BuildContext anchor) =>
                              _showItemMenu(item, anchorContext: anchor),
                      onRowContextMenu: (Track item, int _, Offset position) =>
                          _showItemMenuAtPosition(item, position),
                      onRowSelectionContextMenu:
                          (List<Track> items, Offset position) =>
                              _showItemMenuAtPosition(
                                items.first,
                                position,
                                items,
                              ),
                      onArtistTap: widget.onOpenArtist,
                      onAlbumTap: widget.onOpenAlbum,
                    ),
              ),
            ),
          ),
          SliverPadding(
            padding: const EdgeInsets.fromLTRB(24, 24, 24, 0),
            sliver: SliverToBoxAdapter(
              child: ListenableBuilder(
                listenable: _controller.relatedListenable,
                builder: (BuildContext context, Widget? child) =>
                    MediaRowSectionDesktop<Album>(
                      title: 'Weitere Alben von ${_controller.artistName}',
                      isLoading: _controller.isLoadingOtherAlbums,
                      items: _controller.otherAlbums,
                      emptyText: 'Keine weiteren Alben gefunden.',
                      coreBridge: widget.coreBridge,
                      coreEvents: widget.coreEvents,
                      isRowFavorite: (Album item, int _) =>
                          _controller.isItemFavorite(item),
                      onRowFavoriteTap: (Album item, int _) =>
                          _controller.toggleItemFavorite(item),
                      onRowMoreAnchorTap:
                          (Album item, int _, BuildContext anchor) =>
                              _showItemMenu(item, anchorContext: anchor),
                      onRowContextMenu: (Album item, int _, Offset position) =>
                          _showItemMenuAtPosition(item, position),
                      onArtistTap: widget.onOpenArtist,
                      onAlbumTap: widget.onOpenAlbum,
                    ),
              ),
            ),
          ),
          SliverPadding(
            padding: const EdgeInsets.fromLTRB(24, 24, 24, 24),
            sliver: SliverToBoxAdapter(
              child: ListenableBuilder(
                listenable: _controller.relatedListenable,
                builder: (BuildContext context, Widget? child) =>
                    MediaRowSectionDesktop<Artist>(
                      title: 'Ähnliche Interpreten',
                      isLoading: _controller.isLoadingSimilarArtists,
                      items: _controller.similarArtists,
                      emptyText: 'Keine ähnlichen Interpreten gefunden.',
                      coreBridge: widget.coreBridge,
                      coreEvents: widget.coreEvents,
                      isRowFavorite: (Artist item, int _) =>
                          _controller.isItemFavorite(item),
                      onRowFavoriteTap: (Artist item, int _) =>
                          _controller.toggleItemFavorite(item),
                      onRowMoreAnchorTap:
                          (Artist item, int _, BuildContext anchor) =>
                              _showItemMenu(item, anchorContext: anchor),
                      onRowContextMenu: (Artist item, int _, Offset position) =>
                          _showItemMenuAtPosition(item, position),
                      onArtistTap: widget.onOpenArtist,
                    ),
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildHeader(BuildContext context, MashinColors colors) {
    final Album? album = _controller.album;
    final bool isLoading = _controller.isLoadingMetadata;
    final String? imageUri = album?.imageUri;
    final bool showBlurredBackground =
        !isLoading && imageUri != null && imageUri.trim().isNotEmpty;
    return SizedBox(
      height: _coverSize + 32,
      child: Stack(
        fit: StackFit.expand,
        children: <Widget>[
          if (showBlurredBackground)
            IgnorePointer(
              child: ClipRect(
                child: ShaderMask(
                  blendMode: BlendMode.dstIn,
                  shaderCallback: (Rect bounds) => const LinearGradient(
                    begin: Alignment.topCenter,
                    end: Alignment.bottomCenter,
                    colors: <Color>[
                      Colors.white,
                      Colors.white,
                      Colors.transparent,
                    ],
                    stops: <double>[0, 0.42, 1],
                  ).createShader(bounds),
                  child: ImageFiltered(
                    imageFilter: ui.ImageFilter.blur(sigmaX: 32, sigmaY: 32),
                    child: Transform.scale(
                      scale: 1.18,
                      child: MediaImage(
                        source: imageUri,
                        fit: BoxFit.cover,
                        fallback: const SizedBox.expand(),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          if (showBlurredBackground)
            IgnorePointer(
              child: DecoratedBox(
                decoration: BoxDecoration(
                  color: colors.bgPrimary.withValues(
                    alpha: colors.blurredImageOverlayOpacity,
                  ),
                ),
              ),
            ),
          Padding(
            padding: const EdgeInsets.fromLTRB(24, 16, 24, 16),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                _buildCover(colors, album, isLoading),
                const SizedBox(width: 30),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisAlignment: MainAxisAlignment.end,
                    children: <Widget>[
                      Text(
                        'ALBUM',
                        style: TextStyle(
                          color: colors.textSecondary,
                          fontWeight: FontWeight.bold,
                          fontSize: 12,
                        ),
                      ),
                      const SizedBox(height: 16),
                      Text(
                        _controller.albumName,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context).textTheme.headlineMedium
                            ?.copyWith(fontWeight: FontWeight.bold),
                      ),
                      const SizedBox(height: 8),
                      Row(
                        children: <Widget>[
                          Flexible(
                            child: MediaLinkText(
                              items:
                                  album?.artists.take(1).toList() ??
                                  const <Artist>[],
                              fallbackText: _controller.artistName,
                              onItemTap: widget.onOpenArtist == null
                                  ? null
                                  : (MediaItem item) =>
                                        widget.onOpenArtist!(item as Artist),
                              style: TextStyle(color: colors.textSecondary),
                            ),
                          ),
                          Text(
                            '${_controller.yearText.isEmpty ? '' : ' • ${_controller.yearText}'} • ${_controller.tracks.length} Titel',
                            style: TextStyle(color: colors.textSecondary),
                          ),
                        ],
                      ),
                      const SizedBox(height: 16),
                      if (!isLoading)
                        Row(
                          children: <Widget>[
                            _HeaderActionButton(
                              icon: Icons.play_arrow,
                              label: 'Abspielen',
                              onTap: _controller.playAlbum,
                            ),
                            const SizedBox(width: 12),
                            _HeaderActionButton(
                              icon: Icons.shuffle,
                              label: 'Zufällig',
                              onTap: _controller.shuffleAlbum,
                            ),
                            const SizedBox(width: 24),
                            _HeaderIconButton(
                              tooltip: _controller.isAlbumFavorite
                                  ? 'Favorit entfernen'
                                  : 'Zu Favoriten hinzufügen',
                              icon: _controller.isAlbumFavorite
                                  ? Icons.favorite
                                  : Icons.favorite_border_outlined,
                              onTap: _controller.toggleAlbumFavorite,
                            ),
                            const SizedBox(width: 12),
                            _HeaderIconButton(
                              key: _headerMenuAnchorKey,
                              tooltip: 'Weitere Optionen',
                              icon: Icons.more_horiz_outlined,
                              onTap: album == null
                                  ? null
                                  : () => unawaited(
                                      const AppContextMenuController()
                                          .showContextMenu(
                                            context: context,
                                            anchorContext: _headerMenuAnchorKey
                                                .currentContext,
                                            actions: _buildHeaderMenuActions(
                                              album,
                                            ),
                                          ),
                                    ),
                            ),
                          ],
                        ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildCover(MashinColors colors, Album? album, bool isLoading) {
    final String? uri = album?.imageUri;
    return Container(
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(8),
        boxShadow: <BoxShadow>[
          BoxShadow(
            color: Colors.black.withValues(alpha: 0.28),
            blurRadius: 14,
            offset: const Offset(0, 4),
          ),
        ],
      ),
      child: ClipRRect(
        borderRadius: BorderRadius.circular(8),
        child: Container(
          width: _coverSize,
          height: _coverSize,
          color: colors.bgTertiary,
          child: isLoading
              ? null
              : uri == null || uri.trim().isEmpty
              ? Icon(Icons.album_outlined, color: colors.textSecondary, size: 72)
              : MediaImage(
                  source: uri,
                  fit: BoxFit.cover,
                  fallback: Icon(
                    Icons.album_outlined,
                    color: colors.textSecondary,
                    size: 72,
                  ),
                ),
        ),
      ),
    );
  }

  Widget _buildDescription(BuildContext context, MashinColors colors) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text(
          'Über ${_controller.albumName}',
          style: Theme.of(
            context,
          ).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
        ),
        const SizedBox(height: 16),
        GestureDetector(
          onTap: _controller.toggleDescription,
          child: Text(
            _controller.description ?? '',
            maxLines: _controller.descriptionMaxLines,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(color: colors.textSecondary),
          ),
        ),
      ],
    );
  }
}

class _HeaderActionButton extends StatelessWidget {
  const _HeaderActionButton({
    required this.icon,
    required this.label,
    required this.onTap,
  });
  final IconData icon;
  final String label;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return Material(
      color: colors.textPrimary,
      borderRadius: BorderRadius.circular(26),
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(26),
        hoverColor: colors.bgPrimary.withValues(alpha: 0.45),
        splashColor: Colors.transparent,
        highlightColor: Colors.transparent,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: <Widget>[
              Icon(icon, size: 20, color: colors.bgPrimary),
              const SizedBox(width: 8),
              Text(
                label,
                style: TextStyle(
                  color: colors.bgPrimary,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _HeaderIconButton extends StatelessWidget {
  const _HeaderIconButton({
    super.key,
    required this.tooltip,
    required this.icon,
    required this.onTap,
  });
  final String tooltip;
  final IconData icon;
  final VoidCallback? onTap;
  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return Tooltip(
      message: tooltip,
      child: Material(
        color: Colors.transparent,
        borderRadius: BorderRadius.circular(14),
        child: InkWell(
          onTap: onTap,
          borderRadius: BorderRadius.circular(14),
          hoverColor: colors.hover,
          splashColor: Colors.transparent,
          highlightColor: Colors.transparent,
          child: SizedBox(
            width: 28,
            height: 28,
            child: Icon(icon, size: 18, color: colors.textSecondary),
          ),
        ),
      ),
    );
  }
}
