import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../shared/userdata_controller.dart';
import '../shared/widgets/media_table_section_mobile.dart';
import '../shared/widgets/media_image.dart';
import 'playlist_detail_controller.dart';

/// Mobile playlist detail page: pinned banner header with centered actions and
/// an internally scrolling track table.
class PlaylistDetailPageMobile extends StatefulWidget {
  const PlaylistDetailPageMobile({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
  });

  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserDataController? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

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
      client: widget.client,
      playbackCoordinator: widget.playbackCoordinator,
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

  List<ContextMenuAction> _buildItemMenuActions(MediaItem item) {
    final bool isFavorite = _controller.isItemFavorite(item);
    final List<ContextMenuAction> actions = <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        icon: Icons.play_arrow,
        onSelected: () =>
            widget.playbackCoordinator.playMedia(<MediaItem>[item]),
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
        onSelected: () =>
            widget.playbackCoordinator.playMediaNext(<MediaItem>[item]),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () =>
            widget.playbackCoordinator.playMediaLast(<MediaItem>[item]),
      ),
    ];

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
        onSelected: () => _controller.toggleItemFavorite(item),
      ),
    );
    return actions;
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
      ContextMenuAction(
        label: _controller.isPlaylistFavorite
            ? 'Aus Favoriten entfernen'
            : 'Zu Favoriten hinzufügen',
        icon: _controller.isPlaylistFavorite
            ? Icons.favorite_outlined
            : Icons.favorite_border_outlined,
        onSelected: _controller.togglePlaylistFavorite,
      ),
    ];
  }

  @override
  Widget build(BuildContext context) {
    final MashinColors colors = context.mashinColors;
    final Playlist? playlist = _controller.playlist;
    final String? imageUri = playlist?.imageUri;

    return Container(
      color: colors.bgPrimary,
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
                color: colors.bgTertiary,
                child: imageUri == null || imageUri.trim().isEmpty
                    ? Icon(
                        Icons.queue_music_outlined,
                        color: colors.accent,
                        size: 72,
                      )
                    : MediaImage(
                        source: imageUri,
                        fit: BoxFit.cover,
                        fallback: Icon(
                          Icons.queue_music_outlined,
                          color: colors.accent,
                          size: 72,
                        ),
                      ),
              ),
            ),
          ),
          const SizedBox(height: 8),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            child: Text(
              _controller.playlistName,
              textAlign: TextAlign.center,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(
                context,
              ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.bold),
            ),
          ),
          const SizedBox(height: 4),
          Text(
            '${_controller.tracksCountText} • ${_controller.totalDurationText}',
            style: TextStyle(color: colors.textSecondary),
          ),
          const SizedBox(height: 8),
          Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: <Widget>[
              IconButton(
                onPressed: _controller.playPlaylist,
                tooltip: 'Abspielen',
                icon: const Icon(Icons.play_arrow),
              ),
              IconButton(
                onPressed: _controller.shufflePlaylist,
                tooltip: 'Zufällig',
                icon: const Icon(Icons.shuffle),
              ),
              IconButton(
                onPressed: _controller.togglePlaylistFavorite,
                tooltip: 'Favorit',
                icon: Icon(
                  _controller.isPlaylistFavorite
                      ? Icons.favorite
                      : Icons.favorite_border_outlined,
                ),
              ),
              IconButton(
                onPressed: playlist == null
                    ? null
                    : () => unawaited(
                        const AppContextMenuController().show(
                          context: context,
                          actions: _buildHeaderMenuActions(playlist),
                        ),
                      ),
                tooltip: 'Weitere Optionen',
                icon: const Icon(Icons.more_vert_outlined),
              ),
            ],
          ),
          const SizedBox(height: 8),
          // Header stays pinned, only the table scrolls.
          Expanded(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: MediaTableSectionMobile<Track>(
                title: ' ',
                isLoading: _controller.isLoadingTracks,
                items: _controller.tracks,
                emptyText: 'Keine Titel in dieser Wiedergabeliste.',
                maxItemsPerPage: 0,
                playbackCoordinator: widget.playbackCoordinator,
                onRowTap: (Track item, int _) => unawaited(
                  const AppContextMenuController().show(
                    context: context,
                    actions: _buildItemMenuActions(item),
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
