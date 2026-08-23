import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import 'playlist_detail_controller.dart';

/// Desktop playlist detail page: the header stays pinned (expandable/collapsible)
/// while only the track table scrolls internally.
class PlaylistDetailPageDesktop extends StatefulWidget {
  const PlaylistDetailPageDesktop({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.onOpenArtist,
    this.onOpenAlbum,
  });

  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

  @override
  State<PlaylistDetailPageDesktop> createState() =>
      _PlaylistDetailPageDesktopState();
}

class _PlaylistDetailPageDesktopState extends State<PlaylistDetailPageDesktop> {
  static const double _coverSize = 200;
  static const double _collapsedCoverSize = 48;

  late final PlaylistDetailController _controller;
  final GlobalKey _expandedMenuAnchorKey = GlobalKey();
  final GlobalKey _collapsedMenuAnchorKey = GlobalKey();

  @override
  void initState() {
    super.initState();
    _controller = PlaylistDetailController(
      client: widget.client,
      playbackCoordinator: widget.playbackCoordinator,
      itemId: widget.itemId,
      providerInstanceIdOrDomain: widget.providerInstanceIdOrDomain,
    )..addListener(_onControllerChanged);
    unawaited(_controller.loadPlaylist());
  }

  @override
  void dispose() {
    _controller
      ..removeListener(_onControllerChanged)
      ..dispose();
    super.dispose();
  }

  void _onControllerChanged() {
    if (mounted) setState(() {});
  }

  // Context menus
  List<ContextMenuAction> _buildItemMenuActions(MediaItem item) {
    final bool isFavorite = _controller.isItemFavorite(item);
    final PlaybackCoordinator coordinator = widget.playbackCoordinator;
    final Artist? primaryArtist = _primaryArtistOf(item);
    final Album? primaryAlbum = item is Track ? item.album : null;

    final List<ContextMenuAction> actions = <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        icon: Icons.play_arrow,
        onSelected: () => coordinator.playMedia(<MediaItem>[item]),
      ),
      ContextMenuAction(
        label: 'Radio starten',
        icon: Icons.radio_outlined,
        onSelected: () => item is Track
            ? _controller.startTrackRadio(item)
            : _controller.startItemRadio(item),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => coordinator.playMediaNext(<MediaItem>[item]),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => coordinator.playMediaLast(<MediaItem>[item]),
      ),
    ];

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

    if (item is Track && _controller.isEditable) {
      actions.add(const ContextMenuAction.separator());
      actions.add(
        ContextMenuAction(
          label: 'Aus Wiedergabeliste entfernen',
          icon: Icons.playlist_remove_outlined,
          isDestructive: true,
          onSelected: () => _controller.removeTrack(item),
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
        isDestructive: isFavorite,
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

  List<ContextMenuAction> _buildHeaderMenuActions(Playlist playlist) {
    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () =>
            widget.playbackCoordinator.playMediaNext(<MediaItem>[playlist]),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () =>
            widget.playbackCoordinator.playMediaLast(<MediaItem>[playlist]),
      ),
      const ContextMenuAction.separator(),
      ContextMenuAction(
        label: _controller.isPlaylistFavorite
            ? 'Aus Favoriten entfernen'
            : 'Zu Favoriten hinzufügen',
        icon: _controller.isPlaylistFavorite
            ? Icons.favorite_outlined
            : Icons.favorite_border_outlined,
        isDestructive: _controller.isPlaylistFavorite,
        onSelected: _controller.togglePlaylistFavorite,
      ),
    ];
  }

  Future<void> _showItemMenu(MediaItem item, {BuildContext? anchorContext}) {
    return const AppContextMenuController().show(
      context: context,
      anchorContext: anchorContext,
      actions: _buildItemMenuActions(item),
    );
  }

  Future<void> _showItemMenuAtPosition(MediaItem item, Offset position) {
    return const AppContextMenuController().show(
      context: context,
      globalPosition: position,
      actions: _buildItemMenuActions(item),
    );
  }

  Future<void> _showHeaderMenu(GlobalKey anchorKey) async {
    final Playlist? playlist = _controller.playlist;
    if (playlist == null) {
      return;
    }
    await const AppContextMenuController().show(
      context: context,
      anchorContext: anchorKey.currentContext,
      actions: _buildHeaderMenuActions(playlist),
    );
  }

  @override
  Widget build(BuildContext context) {
    final MashinColors colors = context.mashinColors;

    return Container(
      color: colors.bgPrimary,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          AnimatedSize(
            duration: const Duration(milliseconds: 220),
            curve: Curves.easeInOutCubic,
            alignment: Alignment.topCenter,
            child: _controller.isHeaderCollapsed
                ? _buildCollapsedHeader(context, colors)
                : _buildExpandedHeader(context, colors),
          ),
          SizedBox(height: _controller.isHeaderCollapsed ? 4 : 24),
          // Only the table scrolls; the bounded height puts the section into
          // its internal-scroll mode.
          Expanded(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 24),
              child: MediaTableSectionDesktop<Track>(
                title: ' ',
                isLoading: _controller.isLoadingTracks,
                items: _controller.tracks,
                emptyText: 'Keine Titel in dieser Wiedergabeliste.',
                maxItemsPerPage: 0,
                playbackCoordinator: widget.playbackCoordinator,
                isRowFavorite: (Track item, int _) =>
                    _controller.isItemFavorite(item),
                onRowFavoriteTap: (Track item, int _) =>
                    _controller.toggleItemFavorite(item),
                onRowMoreAnchorTap: (Track item, int _, BuildContext anchor) =>
                    _showItemMenu(item, anchorContext: anchor),
                onRowContextMenu: (Track item, int _, Offset position) =>
                    _showItemMenuAtPosition(item, position),
                onArtistTap: widget.onOpenArtist,
                onAlbumTap: widget.onOpenAlbum,
              ),
            ),
          ),
        ],
      ),
    );
  }

  // Header
  Widget _buildExpandedHeader(BuildContext context, MashinColors colors) {
    final Playlist? playlist = _controller.playlist;
    final bool isLoading = _controller.isLoadingMetadata;
    final String? imageUri = playlist?.imageUri;
    final bool showBlurredBackground =
        !isLoading && imageUri != null && imageUri.trim().isNotEmpty;

    return SizedBox(
      key: const ValueKey<String>('playlist-header-expanded'),
      height: _coverSize + 24,
      child: Stack(
        fit: StackFit.expand,
        children: <Widget>[
          if (showBlurredBackground) _buildBlurBackdrop(colors, imageUri),
          Padding(
            padding: const EdgeInsets.fromLTRB(24, 24, 24, 0),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                _buildCover(colors, imageUri, isLoading, _coverSize),
                const SizedBox(width: 30),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisAlignment: MainAxisAlignment.end,
                    children: <Widget>[
                      Text(
                        'WIEDERGABELISTE',
                        style: TextStyle(
                          color: colors.textSecondary,
                          fontWeight: FontWeight.bold,
                          fontSize: 12,
                        ),
                      ),
                      const SizedBox(height: 16),
                      Text(
                        _controller.playlistName,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context).textTheme.headlineMedium
                            ?.copyWith(fontWeight: FontWeight.bold),
                      ),
                      const SizedBox(height: 8),
                      _buildMetaLine(colors),
                      const SizedBox(height: 16),
                      if (!isLoading)
                        _buildActions(anchorKey: _expandedMenuAnchorKey),
                    ],
                  ),
                ),
                Align(
                  alignment: Alignment.bottomRight,
                  child: _CollapseToggleButton(
                    isCollapsed: false,
                    onTap: _controller.toggleHeaderCollapsed,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildCollapsedHeader(BuildContext context, MashinColors colors) {
    final Playlist? playlist = _controller.playlist;
    final bool isLoading = _controller.isLoadingMetadata;

    return SizedBox(
      key: const ValueKey<String>('playlist-header-collapsed'),
      height: _collapsedCoverSize + 18,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(24, 12, 24, 6),
        child: Row(
          children: <Widget>[
            _buildCover(
              colors,
              playlist?.imageUri,
              isLoading,
              _collapsedCoverSize,
              borderRadius: 4,
            ),
            const SizedBox(width: 12),
            Flexible(
              child: Text(
                _controller.playlistName,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(
                  context,
                ).textTheme.titleLarge?.copyWith(fontWeight: FontWeight.bold),
              ),
            ),
            const SizedBox(width: 12),
            _buildMetaLine(colors),
            const SizedBox(width: 24),
            if (!isLoading) _buildActions(anchorKey: _collapsedMenuAnchorKey),
            const Spacer(),
            _CollapseToggleButton(
              isCollapsed: true,
              onTap: _controller.toggleHeaderCollapsed,
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildMetaLine(MashinColors colors) {
    final String? owner = _controller.ownerName;
    return Text(
      <String>[
        ?owner,
        _controller.tracksCountText,
        _controller.totalDurationText,
      ].join(' • '),
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: TextStyle(color: colors.textSecondary),
    );
  }

  Widget _buildActions({required GlobalKey anchorKey}) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: <Widget>[
        _HeaderActionButton(
          icon: Icons.play_arrow,
          label: 'Abspielen',
          onTap: _controller.playPlaylist,
        ),
        const SizedBox(width: 12),
        _HeaderActionButton(
          icon: Icons.shuffle,
          label: 'Zufällig',
          onTap: _controller.shufflePlaylist,
        ),
        const SizedBox(width: 24),
        _HeaderIconButton(
          tooltip: _controller.isPlaylistFavorite
              ? 'Favorit entfernen'
              : 'Zu Favoriten hinzufügen',
          icon: _controller.isPlaylistFavorite
              ? Icons.favorite
              : Icons.favorite_border_outlined,
          onTap: _controller.togglePlaylistFavorite,
        ),
        const SizedBox(width: 12),
        _HeaderIconButton(
          key: anchorKey,
          tooltip: 'Weitere Optionen',
          icon: Icons.more_horiz_outlined,
          onTap: _controller.playlist == null
              ? null
              : () => unawaited(_showHeaderMenu(anchorKey)),
        ),
      ],
    );
  }

  Widget _buildBlurBackdrop(MashinColors colors, String imageUri) {
    return IgnorePointer(
      child: ClipRect(
        child: Stack(
          fit: StackFit.expand,
          children: <Widget>[
            ShaderMask(
              blendMode: BlendMode.dstIn,
              shaderCallback: (Rect bounds) => const LinearGradient(
                begin: Alignment.topCenter,
                end: Alignment.bottomCenter,
                colors: <Color>[Colors.white, Colors.white, Colors.transparent],
                stops: <double>[0, 0.42, 1],
              ).createShader(bounds),
              child: ImageFiltered(
                imageFilter: ui.ImageFilter.blur(sigmaX: 32, sigmaY: 32),
                child: Transform.scale(
                  scale: 1.18,
                  child: Image.network(
                    imageUri,
                    fit: BoxFit.cover,
                    errorBuilder: (_, _, _) => const SizedBox.expand(),
                  ),
                ),
              ),
            ),
            DecoratedBox(
              decoration: BoxDecoration(
                color: colors.bgPrimary.withValues(alpha: 0.68),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildCover(
    MashinColors colors,
    String? imageUri,
    bool isLoading,
    double size, {
    double borderRadius = 8,
  }) {
    return ClipRRect(
      borderRadius: BorderRadius.circular(borderRadius),
      child: Container(
        width: size,
        height: size,
        color: colors.bgTertiary,
        child: isLoading
            ? null
            : imageUri == null || imageUri.trim().isEmpty
            ? Icon(
                Icons.queue_music_outlined,
                color: colors.accent,
                size: size * 0.36,
              )
            : Image.network(
                imageUri,
                fit: BoxFit.cover,
                errorBuilder: (_, _, _) => Icon(
                  Icons.queue_music_outlined,
                  color: colors.accent,
                  size: size * 0.36,
                ),
              ),
      ),
    );
  }
}

class _CollapseToggleButton extends StatelessWidget {
  const _CollapseToggleButton({required this.isCollapsed, required this.onTap});

  final bool isCollapsed;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final MashinColors colors = context.mashinColors;
    return Tooltip(
      message: isCollapsed ? 'Header ausklappen' : 'Header einklappen',
      child: Material(
        color: Colors.transparent,
        shape: const CircleBorder(),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: onTap,
          customBorder: const CircleBorder(),
          hoverColor: colors.hover,
          splashColor: colors.hover,
          child: SizedBox(
            width: 32,
            height: 32,
            child: Icon(
              isCollapsed ? Icons.keyboard_arrow_down : Icons.keyboard_arrow_up,
              size: 22,
              color: colors.textPrimary,
            ),
          ),
        ),
      ),
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
    final MashinColors colors = context.mashinColors;
    return Material(
      color: colors.textPrimary,
      borderRadius: BorderRadius.circular(26),
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(26),
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
    final MashinColors colors = context.mashinColors;
    return Tooltip(
      message: tooltip,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(14),
        child: SizedBox(
          width: 28,
          height: 28,
          child: Icon(icon, size: 18, color: colors.textSecondary),
        ),
      ),
    );
  }
}
