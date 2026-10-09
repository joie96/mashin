import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_row_section_desktop.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import '../shared/widgets/media_image.dart';
import 'artist_detail_controller.dart';

/// Desktop artist detail page: header with cover/name/actions, description and
/// paginated album/similar-artist rows plus a top-tracks table
class ArtistDetailPageDesktop extends StatefulWidget {
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

  const ArtistDetailPageDesktop({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
  });

  @override
  State<ArtistDetailPageDesktop> createState() =>
      _ArtistDetailPageDesktopState();
}

class _ArtistDetailPageDesktopState extends State<ArtistDetailPageDesktop>
    with TickerProviderStateMixin {
  static const double _coverSize = 200;

  late final ArtistDetailController _controller;
  final GlobalKey _headerMenuAnchorKey = GlobalKey();

  @override
  void initState() {
    super.initState();
    _controller = ArtistDetailController(
      coreBridge: widget.coreBridge,
      itemId: widget.itemId,
      providerInstanceIdOrDomain: widget.providerInstanceIdOrDomain,
      userDataController: widget.userDataController,
    );
    unawaited(_controller.loadArtist());
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _openAlbum(Album album) {
    widget.onOpenAlbum?.call(album);
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
        isEnabled: tracks.isEmpty ? item.canPlay : tracks.any((track) => track.canPlay),
        icon: Icons.play_arrow,
        onSelected: () => widget.coreBridge.queuePlayMedia(
          tracks.isEmpty ? <MediaItem>[item] : tracks,
          option: QueueOption.next,
        ),
      ),
      if (item is Album || item is Playlist)
        ContextMenuAction(
          label: 'Zufällig abspielen',
          isEnabled: item.canPlay,
          icon: Icons.shuffle_outlined,
          onSelected: () =>
              widget.coreBridge.queuePlayMedia(<MediaItem>[item], shuffle: true),
        ),
      ContextMenuAction(
        label: 'Radio starten',
        isEnabled: tracks.isEmpty ? item.canPlay : tracks.any((track) => track.canPlay),
        icon: Icons.radio_outlined,
        onSelected: () => item is Track
            ? widget.coreBridge.startSendspinRadio(
                tracks.isEmpty ? <MediaItem>[item] : tracks,
              )
            : _controller.startItemRadio(item),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        isEnabled: tracks.isEmpty ? item.canPlay : tracks.any((track) => track.canPlay),
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => widget.coreBridge.queuePlayMedia(
          tracks.isEmpty ? <MediaItem>[item] : tracks,
          option: QueueOption.add,
        ),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        isEnabled: tracks.isEmpty ? item.canPlay : tracks.any((track) => track.canPlay),
        icon: Icons.last_page,
        onSelected: () => widget.coreBridge.queuePlayMedia(
          tracks.isEmpty ? <MediaItem>[item] : tracks,
        ),
      ),
    ];

    // Single-target navigation entries, mirroring the MAUI context menus.
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
            onSelected: () => _openAlbum(primaryAlbum),
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

  List<ContextMenuAction> _buildHeaderMenuActions(Artist artist) {
    final bool isFavorite = _controller.isArtistFavorite;
    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Radio starten',
        icon: Icons.radio_outlined,
        onSelected: _controller.startArtistRadio,
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[artist], option: QueueOption.next),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[artist], option: QueueOption.add),
      ),
      const ContextMenuAction.separator(),
      ContextMenuAction(
        label: isFavorite
            ? 'Aus Favoriten entfernen'
            : 'Zu Favoriten hinzufügen',
        icon: isFavorite
            ? Icons.favorite_outlined
            : Icons.favorite_border_outlined,
        onSelected: _controller.toggleArtistFavorite,
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
    Offset globalPosition, [
    List<Track> selectedTracks = const <Track>[],
  ]) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      globalPosition: globalPosition,
      actions: _buildItemMenuActions(item, selectedTracks),
    );
  }

  Future<void> _showHeaderMenu(Artist artist, {BuildContext? anchorContext}) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      anchorContext: anchorContext,
      actions: _buildHeaderMenuActions(artist),
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
                _controller.albumsListenable,
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
                listenable: _controller.albumsListenable,
                builder: (BuildContext context, Widget? child) =>
                    MediaRowSectionDesktop<Album>(
                      title: 'Veröffentlichungen',
                      isLoading: _controller.isLoadingAlbums,
                      items: _controller.albums,
                      emptyText: 'Keine Veröffentlichungen gefunden.',
                      coreBridge: widget.coreBridge,
                      coreEvents: widget.coreEvents,
                      isRowFavorite: (Album item, int index) =>
                          _controller.isItemFavorite(item),
                      onRowFavoriteTap: (Album item, int index) =>
                          _controller.toggleItemFavorite(item),
                      onRowMoreAnchorTap:
                          (Album item, int index, BuildContext anchorContext) =>
                              _showItemMenu(item, anchorContext: anchorContext),
                      onRowContextMenu:
                          (Album item, int index, Offset globalPosition) =>
                              _showItemMenuAtPosition(item, globalPosition),
                      onArtistTap: _openArtist,
                      onAlbumTap: _openAlbum,
                    ),
              ),
            ),
          ),
          SliverPadding(
            padding: const EdgeInsets.fromLTRB(24, 24, 24, 0),
            sliver: SliverToBoxAdapter(
              child: ListenableBuilder(
                listenable: _controller.similarArtistsListenable,
                builder: (BuildContext context, Widget? child) =>
                    MediaRowSectionDesktop<Artist>(
                      title: 'Ähnliche Interpreten',
                      isLoading: _controller.isLoadingSimilarArtists,
                      items: _controller.similarArtists,
                      emptyText: 'Keine ähnlichen Interpreten gefunden.',
                      coreBridge: widget.coreBridge,
                      coreEvents: widget.coreEvents,
                      isRowFavorite: (Artist item, int index) =>
                          _controller.isItemFavorite(item),
                      onRowFavoriteTap: (Artist item, int index) =>
                          _controller.toggleItemFavorite(item),
                      onRowMoreAnchorTap:
                          (
                            Artist item,
                            int index,
                            BuildContext anchorContext,
                          ) =>
                              _showItemMenu(item, anchorContext: anchorContext),
                      onRowContextMenu:
                          (Artist item, int index, Offset globalPosition) =>
                              _showItemMenuAtPosition(item, globalPosition),
                      onArtistTap: widget.onOpenArtist,
                    ),
              ),
            ),
          ),
          SliverPadding(
            padding: const EdgeInsets.fromLTRB(24, 24, 24, 24),
            sliver: SliverToBoxAdapter(
              child: ListenableBuilder(
                listenable: _controller.topTracksListenable,
                builder: (BuildContext context, Widget? child) =>
                    MediaTableSectionDesktop<Track>(
                      title: 'Toplieder',
                      isLoading: _controller.isLoadingTopTracks,
                      items: _controller.topTracks,
                      emptyText: 'Keine Toplieder gefunden.',
                      maxItemsPerPage: 15,
                      coreBridge: widget.coreBridge,
                      coreEvents: widget.coreEvents,
                      isRowFavorite: (Track item, int index) =>
                          _controller.isItemFavorite(item),
                      onRowFavoriteTap: (Track item, int index) =>
                          _controller.toggleItemFavorite(item),
                      onRowMoreAnchorTap:
                          (Track item, int index, BuildContext anchorContext) =>
                              _showItemMenu(item, anchorContext: anchorContext),
                      onRowContextMenu:
                          (Track item, int index, Offset globalPosition) =>
                              _showItemMenuAtPosition(item, globalPosition),
                      onRowSelectionContextMenu:
                          (List<Track> items, Offset globalPosition) =>
                              _showItemMenuAtPosition(
                                items.first,
                                globalPosition,
                                items,
                              ),
                      onArtistTap: _openArtist,
                      onAlbumTap: _openAlbum,
                    ),
              ),
            ),
          ),
        ],
      ),
    );
  }

  void _openArtist(Artist artist) {
    widget.onOpenArtist?.call(artist);
  }

  Widget _buildHeader(BuildContext context, MashinColors colors) {
    final Artist? artist = _controller.artist;
    final bool isLoading = _controller.isLoadingMetadata;
    final String? imageUri = artist?.imageUri;
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
                _buildArtistImage(colors, artist, isLoading),
                const SizedBox(width: 30),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisAlignment: MainAxisAlignment.end,
                    children: <Widget>[
                      isLoading
                          ? _buildSkeletonLine(colors, width: 120, height: 12)
                          : Text(
                              'ARTIST/INTERPRET',
                              style: TextStyle(
                                color: colors.textSecondary,
                                fontWeight: FontWeight.bold,
                                fontSize: 12,
                              ),
                            ),
                      const SizedBox(height: 16),
                      isLoading
                          ? _buildSkeletonLine(colors, width: 320, height: 32)
                          : Row(
                              crossAxisAlignment: CrossAxisAlignment.center,
                              children: <Widget>[
                                Flexible(
                                  child: Text(
                                    _controller.artistName,
                                    maxLines: 2,
                                    overflow: TextOverflow.ellipsis,
                                    style: Theme.of(context)
                                        .textTheme
                                        .headlineMedium
                                        ?.copyWith(fontWeight: FontWeight.bold),
                                  ),
                                ),
                                if (artist != null) ...<Widget>[
                                  const SizedBox(width: 16),
                                  MediaProviderIcon(
                                    providerLabel: artist.provider,
                                    iconSvg:
                                        ProviderManifestRegistry.iconSvgForProvider(
                                          artist.provider,
                                        ),
                                    size: 28,
                                  ),
                                ],
                              ],
                            ),
                      const SizedBox(height: 8),
                      isLoading
                          ? _buildSkeletonLine(colors, width: 150, height: 14)
                          : Text(
                              '${_controller.albums.length} Alben',
                              style: TextStyle(color: colors.textSecondary),
                            ),
                      const SizedBox(height: 16),
                      if (!isLoading)
                        Row(
                          children: <Widget>[
                            _HeaderActionButton(
                              icon: Icons.play_arrow,
                              label: 'Abspielen',
                              onTap: _controller.playArtist,
                            ),
                            const SizedBox(width: 12),
                            _HeaderActionButton(
                              icon: Icons.shuffle,
                              label: 'Zufällig',
                              onTap: _controller.shuffleArtist,
                            ),
                            const SizedBox(width: 24),
                            _HeaderHoverIconButton(
                              tooltip: _controller.isArtistFavorite
                                  ? 'Favorit entfernen'
                                  : 'Als Favorit markieren',
                              onTap: _controller.toggleArtistFavorite,
                              icon: _controller.isArtistFavorite
                                  ? Icons.favorite
                                  : Icons.favorite_border_outlined,
                            ),
                            const SizedBox(width: 12),
                            _HeaderHoverIconButton(
                              key: _headerMenuAnchorKey,
                              tooltip: 'Weitere Optionen',
                              onTap: artist == null
                                  ? null
                                  : () => unawaited(
                                      _showHeaderMenu(
                                        artist,
                                        anchorContext:
                                            _headerMenuAnchorKey.currentContext,
                                      ),
                                    ),
                              icon: Icons.more_horiz_outlined,
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

  Widget _buildArtistImage(
    MashinColors colors,
    Artist? artist,
    bool isLoading,
  ) {
    if (isLoading) {
      return ClipOval(
        child: Container(
          width: _coverSize,
          height: _coverSize,
          color: colors.bgTertiary,
        ),
      );
    }

    final String? imageUri = artist?.imageUri;
    return Container(
      decoration: BoxDecoration(
        shape: BoxShape.circle,
        boxShadow: <BoxShadow>[
          BoxShadow(
            color: Colors.black.withValues(alpha: 0.28),
            blurRadius: 14,
            offset: const Offset(0, 4),
          ),
        ],
      ),
      child: ClipOval(
        child: Container(
          width: _coverSize,
          height: _coverSize,
          color: colors.bgTertiary,
          child: imageUri == null || imageUri.trim().isEmpty
              ? Icon(Icons.person_outlined, color: colors.textSecondary, size: 72)
              : MediaImage(
                  source: imageUri,
                  fit: BoxFit.cover,
                  fallback: Icon(
                    Icons.person_outlined,
                    color: colors.textSecondary,
                    size: 72,
                  ),
                ),
        ),
      ),
    );
  }

  Widget _buildDescription(BuildContext context, MashinColors colors) {
    if (_controller.isLoadingMetadata) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          _buildSkeletonLine(colors, width: 200, height: 18),
          const SizedBox(height: 16),
          _buildSkeletonLine(colors, width: double.infinity, height: 14),
          const SizedBox(height: 16),
          _buildSkeletonLine(colors, width: double.infinity, height: 14),
          const SizedBox(height: 16),
          _buildSkeletonLine(colors, width: double.infinity, height: 14),
        ],
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text(
          'Über ${_controller.artistName}',
          style: Theme.of(
            context,
          ).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
        ),
        const SizedBox(height: 16),
        AnimatedSize(
          duration: const Duration(milliseconds: 260),
          curve: Curves.easeInOutCubic,
          alignment: Alignment.topCenter,
          clipBehavior: Clip.hardEdge,
          child: GestureDetector(
            onTap: _controller.toggleDescription,
            child: Text(
              _controller.description ?? '',
              maxLines: _controller.descriptionMaxLines,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(color: colors.textSecondary),
            ),
          ),
        ),
        const SizedBox(height: 8),
        Center(
          child: _DescriptionToggleButton(
            isExpanded: _controller.isDescriptionExpanded,
            onTap: _controller.toggleDescription,
          ),
        ),
      ],
    );
  }

  Widget _buildSkeletonLine(
    MashinColors colors, {
    required double width,
    required double height,
  }) {
    return Container(
      width: width,
      height: height,
      decoration: BoxDecoration(
        color: colors.bgTertiary,
        borderRadius: BorderRadius.circular(6),
      ),
    );
  }
}

class _HeaderActionButton extends StatelessWidget {
  final IconData icon;
  final String label;
  final VoidCallback onTap;

  const _HeaderActionButton({
    required this.icon,
    required this.label,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Material(
      color: colors.textPrimary,
      borderRadius: BorderRadius.circular(26),
      child: InkWell(
        borderRadius: BorderRadius.circular(26),
        onTap: onTap,
        hoverColor: colors.bgPrimary.withValues(alpha: 0.45),
        splashColor: Colors.transparent,
        highlightColor: Colors.transparent,
        child: SizedBox(
          height: 40,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            child: Center(
              child: SizedBox(
                height: 20,
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.center,
                  children: <Widget>[
                    Icon(icon, size: 20, color: colors.bgPrimary),
                    const SizedBox(width: 8),
                    Text(
                      label,
                      strutStyle: const StrutStyle(
                        fontSize: 15,
                        height: 1.3333333333,
                        forceStrutHeight: true,
                      ),
                      style: TextStyle(
                        color: colors.bgPrimary,
                        fontSize: 15,
                        height: 1.3333333333,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _HeaderHoverIconButton extends StatelessWidget {
  final String tooltip;
  final IconData icon;
  final VoidCallback? onTap;

  const _HeaderHoverIconButton({
    super.key,
    required this.tooltip,
    required this.icon,
    required this.onTap,
  });

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

class _DescriptionToggleButton extends StatelessWidget {
  final bool isExpanded;
  final VoidCallback onTap;

  const _DescriptionToggleButton({
    required this.isExpanded,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Material(
      color: Colors.transparent,
      borderRadius: BorderRadius.circular(12),
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(12),
        hoverColor: colors.hover,
        splashColor: Colors.transparent,
        highlightColor: Colors.transparent,
        child: SizedBox(
          width: 24,
          height: 24,
          child: AnimatedRotation(
            turns: isExpanded ? 0.5 : 0,
            duration: const Duration(milliseconds: 220),
            curve: Curves.easeOutCubic,
            child: Icon(
              Icons.expand_more_outlined,
              size: 14,
              color: colors.textPrimary,
            ),
          ),
        ),
      ),
    );
  }
}
