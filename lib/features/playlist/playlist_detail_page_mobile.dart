import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_table_section_mobile.dart';
import '../shared/widgets/media_image.dart';
import 'playlist_detail_controller.dart';
import 'playlist_name_dialog.dart';

/// Mobile playlist detail page: pinned banner header with centered actions and
/// an internally scrolling track table.
class PlaylistDetailPageMobile extends StatefulWidget {
  const PlaylistDetailPageMobile({
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
  State<PlaylistDetailPageMobile> createState() =>
      _PlaylistDetailPageMobileState();
}

class _PlaylistDetailPageMobileState extends State<PlaylistDetailPageMobile> {
  static const double _bannerHeight = 220;

  late final PlaylistDetailController _controller;

  @override
  void initState() {
    super.initState();
    _controller = PlaylistDetailController(
      coreBridge: widget.coreBridge,
      itemId: widget.itemId,
      providerInstanceIdOrDomain: widget.providerInstanceIdOrDomain,
      userDataController: widget.userDataController,
    )..addListener(_onChanged);
    unawaited(_controller.loadPlaylist());
  }

  @override
  void dispose() {
    _controller
      ..removeListener(_onChanged)
      ..dispose();
    super.dispose();
  }

  void _onChanged() {
    if (mounted) setState(() {});
  }

  List<ContextMenuAction> _buildItemMenuActions(
    MediaItem item, [
    List<MediaItem> selectedItems = const <MediaItem>[],
  ]) {
    final List<MediaItem> targets = selectedItems.isEmpty
        ? <MediaItem>[item]
        : selectedItems;
    final List<Track> tracks = targets.whereType<Track>().toList(
      growable: false,
    );
    final bool isSingleTarget = targets.length == 1;
    final bool allFavorite = targets.every(_controller.isItemFavorite);
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
          onSelected: () => widget.coreBridge.queuePlayMedia(targets, shuffle: true),
        ),
      ContextMenuAction(
        label: 'Radio starten',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.radio_outlined,
        onSelected: () => widget.coreBridge.startSendspinRadio(targets),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
          onSelected: () => widget.coreBridge.queuePlayMedia(targets, option: QueueOption.next),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.last_page,
          onSelected: () => widget.coreBridge.queuePlayMedia(targets, option: QueueOption.add),
      ),
    ];

    if (isSingleTarget) {
      final Artist? primaryArtist = _primaryArtistOf(item);
      final Album? primaryAlbum = item is Track ? item.album : null;
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

    if (tracks.isNotEmpty) {
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

    if (tracks.isNotEmpty && _controller.isEditable) {
      if (widget.userDataController?.playlists.isEmpty ?? true) {
        actions.add(const ContextMenuAction.separator());
      }
      actions.add(
        ContextMenuAction(
          label: 'Aus Wiedergabeliste entfernen',
          icon: Icons.playlist_remove_outlined,
          onSelected: () async {
            for (final Track track in tracks) {
              await _controller.removeTrack(track);
            }
          },
        ),
      );
    }

    actions.add(const ContextMenuAction.separator());
    actions.add(
      ContextMenuAction(
        label: allFavorite
            ? 'Aus Favoriten entfernen'
            : 'Zu Favoriten hinzufügen',
        icon: allFavorite
            ? Icons.favorite_outlined
            : Icons.favorite_border_outlined,
        onSelected: () async {
          for (final MediaItem target in targets) {
            if (_controller.isItemFavorite(target) == allFavorite) {
              await _controller.toggleItemFavorite(target);
            }
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

  Future<void> _showSelectionMenu(List<Track> items) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      actions: _buildItemMenuActions(items.first, items),
    );
  }

  List<ContextMenuAction> _buildHeaderMenuActions(Playlist playlist) {
    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () =>
            widget.coreBridge.queuePlayMedia(<MediaItem>[playlist], option: QueueOption.next),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () =>
            widget.coreBridge.queuePlayMedia(<MediaItem>[playlist], option: QueueOption.add),
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

  @override
  Widget build(BuildContext context) {
    final MashinColors colors = context.mashinColors;
    final Playlist? playlist = _controller.playlist;
    final String? imageUri = playlist?.imageUri;
    final bool isLoadingMetadata = _controller.isLoadingMetadata;
    final bool hasImage = imageUri != null && imageUri.trim().isNotEmpty;

    return Container(
      color: colors.bgPrimary,
      child: CustomScrollView(
        slivers: <Widget>[
          SliverPadding(
            padding: const EdgeInsets.only(bottom: 12),
            sliver: SliverToBoxAdapter(
              child: Column(
                children: <Widget>[
                  SizedBox(
                    height: _bannerHeight,
                    width: double.infinity,
                    child: ClipRRect(
                      borderRadius: const BorderRadius.vertical(
                        bottom: Radius.circular(16),
                      ),
                      child: Container(
                        color: colors.bgPrimary,
                        child: Stack(
                          fit: StackFit.expand,
                          children: <Widget>[
                            isLoadingMetadata
                                ? Icon(
                                    Icons.queue_music_outlined,
                                    color: colors.textSecondary,
                                    size: 72,
                                  )
                                : !hasImage
                                ? Icon(
                                    Icons.queue_music_outlined,
                                    color: colors.textSecondary,
                                    size: 72,
                                  )
                                : MediaImage(
                                    source: imageUri,
                                    fit: BoxFit.cover,
                                    fallback: Icon(
                                      Icons.queue_music_outlined,
                                      color: colors.textSecondary,
                                      size: 72,
                                    ),
                                  ),
                            if (hasImage)
                              Positioned.fill(
                                child: IgnorePointer(
                                  child: DecoratedBox(
                                    decoration: BoxDecoration(
                                      gradient: LinearGradient(
                                        begin: Alignment.topCenter,
                                        end: Alignment.bottomCenter,
                                        colors: <Color>[
                                          Colors.transparent,
                                          colors.bgPrimary,
                                        ],
                                        stops: const <double>[0.44, 1],
                                      ),
                                    ),
                                  ),
                                ),
                              ),
                          ],
                        ),
                      ),
                    ),
                  ),
                  const SizedBox(height: 8),
                  if (isLoadingMetadata)
                    _buildSkeletonLine(colors, width: 200, height: 24)
                  else
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 16),
                      child: Text(
                        _controller.playlistName,
                        textAlign: TextAlign.center,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context).textTheme.headlineSmall
                            ?.copyWith(fontWeight: FontWeight.bold),
                      ),
                    ),
                  const SizedBox(height: 4),
                  if (isLoadingMetadata)
                    _buildSkeletonLine(colors, width: 140, height: 14)
                  else
                    Text(
                      '${_controller.tracksCountText} • ${_controller.totalDurationText}',
                      style: TextStyle(color: colors.textSecondary),
                    ),
                  const SizedBox(height: 8),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: <Widget>[
                      _MobileActionButton(
                        icon: Icons.play_arrow,
                        onTap: isLoadingMetadata
                            ? null
                            : _controller.playPlaylist,
                      ),
                      const SizedBox(width: 14),
                      _MobileActionButton(
                        icon: Icons.shuffle,
                        onTap: isLoadingMetadata
                            ? null
                            : _controller.shufflePlaylist,
                      ),
                      if (!_controller.isLocalPlaylist) ...<Widget>[
                        const SizedBox(width: 14),
                        _MobileActionButton(
                          icon: _controller.isPlaylistFavorite
                              ? Icons.favorite
                              : Icons.favorite_border_outlined,
                          onTap: isLoadingMetadata
                              ? null
                              : _controller.togglePlaylistFavorite,
                        ),
                      ],
                      const SizedBox(width: 14),
                      _MobileActionButton(
                        icon: Icons.more_vert_outlined,
                        onTap: playlist == null
                            ? null
                            : () => unawaited(
                                const AppContextMenuController()
                                    .showContextMenu(
                                      context: context,
                                      actions: _buildHeaderMenuActions(
                                        playlist,
                                      ),
                                    ),
                              ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 8),
                ],
              ),
            ),
          ),
          SliverPadding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            sliver: MediaTableSectionMobile<Track>(
              title: ' ',
              isLoading: _controller.isLoadingTracks,
              items: _controller.tracks,
              emptyText: 'Keine Titel in dieser Wiedergabeliste.',
              maxItemsPerPage: 0,
              coreBridge: widget.coreBridge,
              coreEvents: widget.coreEvents,
              onRowTap: (Track item, int _) => unawaited(
                const AppContextMenuController().showContextMenu(
                  context: context,
                  actions: _buildItemMenuActions(item),
                ),
              ),
              onSelectionMenuTap: (List<Track> items) =>
                  unawaited(_showSelectionMenu(items)),
            ),
          ),
        ],
      ),
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

class _MobileActionButton extends StatelessWidget {
  final IconData icon;
  final VoidCallback? onTap;

  const _MobileActionButton({required this.icon, required this.onTap});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Material(
      color: Colors.transparent,
      shape: const CircleBorder(),
      child: InkWell(
        customBorder: const CircleBorder(),
        onTap: onTap,
        highlightColor: Colors.transparent,
        splashColor: colors.selection,
        child: SizedBox(
          width: 44,
          height: 44,
          child: Icon(icon, color: colors.textPrimary, size: 20),
        ),
      ),
    );
  }
}
