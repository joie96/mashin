import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import '../shared/widgets/media_image.dart';
import 'playlist_detail_controller.dart';
import 'playlist_name_dialog.dart';

/// Desktop playlist detail page with a shared scroll area for header and tracks.
class PlaylistDetailPageDesktop extends StatefulWidget {
  const PlaylistDetailPageDesktop({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onPlaylistDeleted,
  });

  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final VoidCallback? onPlaylistDeleted;

  @override
  State<PlaylistDetailPageDesktop> createState() =>
      _PlaylistDetailPageDesktopState();
}

class _PlaylistDetailPageDesktopState extends State<PlaylistDetailPageDesktop> {
  static const double _coverSize = 200;

  late final PlaylistDetailController _controller;
  final GlobalKey _headerMenuAnchorKey = GlobalKey();

  @override
  void initState() {
    super.initState();
    _controller = PlaylistDetailController(
      coreBridge: widget.coreBridge,
      itemId: widget.itemId,
      providerInstanceIdOrDomain: widget.providerInstanceIdOrDomain,
      userDataController: widget.userDataController,
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
  List<ContextMenuAction> _buildItemMenuActions(
    MediaItem item, [
    List<Track> selectedTracks = const <Track>[],
  ]) {
    final List<Track> tracks = selectedTracks.isEmpty && item is Track
        ? <Track>[item]
        : selectedTracks;
    final bool isFavorite = _controller.isItemFavorite(item);
    final Artist? primaryArtist = _primaryArtistOf(item);
    final Album? primaryAlbum = item is Track ? item.album : null;

    final List<ContextMenuAction> actions = <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        isEnabled: tracks.isEmpty
            ? item.canPlay
            : tracks.any((track) => track.canPlay),
        icon: Icons.play_arrow,
        onSelected: () => widget.coreBridge.queuePlayMedia(
          tracks.isEmpty ? <MediaItem>[item] : tracks,
        ),
      ),
      if (item is Album || item is Playlist)
        ContextMenuAction(
          label: 'Zufällig abspielen',
          isEnabled: item.canPlay,
          icon: Icons.shuffle_outlined,
          onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[
            item,
          ], shuffle: true),
        ),
      ContextMenuAction(
        label: 'Radio starten',
        isEnabled: tracks.isEmpty
            ? item.canPlay
            : tracks.any((track) => track.canPlay),
        icon: Icons.radio_outlined,
        onSelected: () => item is Track
            ? widget.coreBridge.startRadio(
                tracks.isEmpty ? <MediaItem>[item] : tracks,
              )
            : _controller.startItemRadio(item),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        isEnabled: tracks.isEmpty
            ? item.canPlay
            : tracks.any((track) => track.canPlay),
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => widget.coreBridge.queuePlayMedia(
          tracks.isEmpty ? <MediaItem>[item] : tracks,
          option: QueueOption.next,
        ),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        isEnabled: tracks.isEmpty
            ? item.canPlay
            : tracks.any((track) => track.canPlay),
        icon: Icons.last_page,
        onSelected: () => widget.coreBridge.queuePlayMedia(
          tracks.isEmpty ? <MediaItem>[item] : tracks,
          option: QueueOption.add,
        ),
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

    if (item is Track) {
      final List<Playlist> playlists =
          widget.userDataController?.playlists ?? const <Playlist>[];
      if (playlists.isNotEmpty) {
        actions.add(const ContextMenuAction.separator());
        actions.add(
          ContextMenuAction.submenu(
            label: 'Zu Wiedergabeliste hinzufügen',
            icon: Icons.playlist_add_outlined,
            children: playlists
                .map(
                  (Playlist playlist) => ContextMenuAction(
                    label: playlist.name,
                    icon: Icons.playlist_play_outlined,
                    onSelected: () => _controller.addTracksToPlaylist(
                      playlist.itemId,
                      tracks,
                    ),
                  ),
                )
                .toList(growable: false),
          ),
        );
      }
    }

    if (item is Track && _controller.isEditable) {
      if (widget.userDataController?.playlists.isEmpty ?? true) {
        actions.add(const ContextMenuAction.separator());
      }
      actions.add(
        ContextMenuAction(
          label: 'Aus Wiedergabeliste entfernen',
          icon: Icons.playlist_remove_outlined,
          onSelected: () async {
            for (final Track selectedTrack in tracks) {
              await _controller.removeTrack(selectedTrack);
            }
          },
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
        onSelected: () async {
          if (tracks.isEmpty) {
            await _controller.toggleItemFavorite(item);
            return;
          }
          for (final Track selectedTrack in tracks) {
            await _controller.toggleItemFavorite(selectedTrack);
          }
        },
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
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[
          playlist,
        ], option: QueueOption.next),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[
          playlist,
        ], option: QueueOption.add),
      ),
      if (!_controller.isLocalPlaylist) ...<ContextMenuAction>[
        const ContextMenuAction.separator(),
        ContextMenuAction(
          label: _controller.isPlaylistFavorite
              ? 'Aus Favoriten entfernen'
              : 'Zu Favoriten hinzufügen',
          icon: _controller.isPlaylistFavorite
              ? Icons.favorite_outlined
              : Icons.favorite_border_outlined,
          onSelected: _controller.togglePlaylistFavorite,
        ),
      ],
      if (_controller.isLocalPlaylist) ...<ContextMenuAction>[
        const ContextMenuAction.separator(),
        ContextMenuAction(
          label: 'Umbenennen',
          icon: Icons.edit_outlined,
          onSelected: _renamePlaylist,
        ),
        ContextMenuAction(
          label: 'Löschen',
          icon: Icons.delete_outline,
          isDestructive: true,
          onSelected: _deletePlaylist,
        ),
      ],
    ];
  }

  Future<void> _renamePlaylist() async {
    final Playlist? playlist = _controller.playlist;
    final UserData? userData = widget.userDataController;
    if (playlist == null || userData == null || !_controller.isLocalPlaylist) {
      return;
    }

    final String? name = await showPlaylistNameDialog(
      context: context,
      playlist: playlist,
    );
    if (name != null) {
      await userData.renamePlaylist(playlist, name);
      await _controller.loadPlaylist();
    }
  }

  Future<void> _deletePlaylist() async {
    final Playlist? playlist = _controller.playlist;
    final UserData? userData = widget.userDataController;
    if (playlist == null || userData == null || !_controller.isLocalPlaylist) {
      return;
    }

    final bool confirmed = await showDeletePlaylistDialog(
      context: context,
      playlistName: _controller.playlistName,
    );
    if (!confirmed || !mounted) {
      return;
    }

    if (await userData.removePlaylist(playlist)) {
      widget.onPlaylistDeleted?.call();
    }
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

  Future<void> _showHeaderMenu(GlobalKey anchorKey) async {
    final Playlist? playlist = _controller.playlist;
    if (playlist == null) {
      return;
    }
    await const AppContextMenuController().showContextMenu(
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
      child: CustomScrollView(
        slivers: <Widget>[
          SliverToBoxAdapter(child: _buildHeader(context, colors)),
          SliverPadding(
            padding: const EdgeInsets.fromLTRB(24, 12, 24, 24),
            sliver: MediaTableSectionDesktop<Track>(
              title: ' ',
              isLoading: _controller.isLoadingTracks,
              items: _controller.tracks,
              emptyText: 'Keine Titel in dieser Wiedergabeliste.',
              maxItemsPerPage: 0,
              coreBridge: widget.coreBridge,
              coreEvents: widget.coreEvents,
              isRowFavorite: (Track item, int _) =>
                  _controller.isItemFavorite(item),
              onRowFavoriteTap: (Track item, int _) =>
                  _controller.toggleItemFavorite(item),
              onRowMoreAnchorTap: (Track item, int _, BuildContext anchor) =>
                  _showItemMenu(item, anchorContext: anchor),
              onRowContextMenu: (Track item, int _, Offset position) =>
                  _showItemMenuAtPosition(item, position),
              onRowSelectionContextMenu: (List<Track> items, Offset position) =>
                  _showItemMenuAtPosition(items.first, position, items),
              onArtistTap: widget.onOpenArtist,
              onAlbumTap: widget.onOpenAlbum,
            ),
          ),
        ],
      ),
    );
  }

  // Header
  Widget _buildHeader(BuildContext context, MashinColors colors) {
    final Playlist? playlist = _controller.playlist;
    final bool isLoading = _controller.isLoadingMetadata;
    final String? imageUri = playlist?.imageUri;
    final bool showBlurredBackground =
        !isLoading && imageUri != null && imageUri.trim().isNotEmpty;

    return SizedBox(
      height: _coverSize + 32,
      child: Stack(
        fit: StackFit.expand,
        children: <Widget>[
          if (showBlurredBackground) _buildBlurBackdrop(colors, imageUri),
          Padding(
            padding: const EdgeInsets.fromLTRB(24, 16, 24, 16),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                _buildCover(
                  colors,
                  imageUri,
                  isLoading,
                  showShadow: showBlurredBackground,
                ),
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
                        _buildActions(anchorKey: _headerMenuAnchorKey),
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
        if (!_controller.isLocalPlaylist) ...<Widget>[
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
        ],
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
                  child: MediaImage(
                    source: imageUri,
                    fit: BoxFit.cover,
                    fallback: const SizedBox.expand(),
                  ),
                ),
              ),
            ),
            DecoratedBox(
              decoration: BoxDecoration(
                color: colors.bgPrimary.withValues(
                  alpha: colors.blurredImageOverlayOpacity,
                ),
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
    bool isLoading, {
    bool showShadow = false,
  }) {
    return Container(
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(8),
        boxShadow: showShadow
            ? <BoxShadow>[
                BoxShadow(
                  color: Colors.black.withValues(alpha: 0.28),
                  blurRadius: 14,
                  offset: const Offset(0, 4),
                ),
              ]
            : null,
      ),
      child: ClipRRect(
        borderRadius: BorderRadius.circular(8),
        child: Container(
          width: _coverSize,
          height: _coverSize,
          color: colors.bgTertiary,
          child: isLoading
              ? null
              : imageUri == null || imageUri.trim().isEmpty
              ? Icon(
                  Icons.queue_music_outlined,
                  color: colors.textSecondary,
                  size: _coverSize * 0.36,
                )
              : MediaImage(
                  source: imageUri,
                  fit: BoxFit.cover,
                  fallback: Icon(
                    Icons.queue_music_outlined,
                    color: colors.textSecondary,
                    size: _coverSize * 0.36,
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
    final MashinColors colors = context.mashinColors;
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
