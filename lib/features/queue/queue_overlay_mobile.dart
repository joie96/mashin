import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../shared/userdata_controller.dart';
import '../shared/widgets/media_table_section_mobile.dart';

class QueueOverlayMobile extends StatefulWidget {
  final String queueTrackCountText;
  final String queueTotalDurationText;
  final List<QueueItem> queueItems;
  final int? currentQueueIndex;
  final PlaybackCoordinator playbackCoordinator;
  final UserDataController? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<double> onDismissDragUpdate;
  final ValueChanged<double> onDismissDragEnd;

  const QueueOverlayMobile({
    required this.queueTrackCountText,
    required this.queueTotalDurationText,
    required this.queueItems,
    required this.currentQueueIndex,
    required this.playbackCoordinator,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
    required this.onDismissDragUpdate,
    required this.onDismissDragEnd,
    super.key,
  });

  @override
  State<QueueOverlayMobile> createState() => _QueueOverlayMobileState();
}

class _QueueOverlayMobileState extends State<QueueOverlayMobile> {
  static const double _queueRowExtent = 64;
  final ScrollController _scrollController = ScrollController();
  int? _lastCurrentQueueIndex;
  bool _isDragging = false;

  void _setDragging(bool isDragging) {
    if (_isDragging == isDragging) {
      return;
    }
    setState(() => _isDragging = isDragging);
  }

  bool _handleTableScrollNotification(ScrollNotification notification) {
    if (notification.metrics.pixels > notification.metrics.minScrollExtent) {
      return false;
    }
    if (notification is OverscrollNotification && notification.overscroll < 0) {
      _setDragging(true);
      widget.onDismissDragUpdate(-notification.overscroll);
    } else if (notification is ScrollEndNotification) {
      _setDragging(false);
      widget.onDismissDragEnd(notification.dragDetails?.primaryVelocity ?? 0);
    }
    return false;
  }

  @override
  void initState() {
    super.initState();
    _scrollToCurrentItem();
  }

  @override
  void didUpdateWidget(covariant QueueOverlayMobile oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.currentQueueIndex != widget.currentQueueIndex ||
        oldWidget.queueItems.length != widget.queueItems.length) {
      _scrollToCurrentItem();
    }
  }

  @override
  void dispose() {
    _scrollController.dispose();
    super.dispose();
  }

  void _scrollToCurrentItem() {
    final int? index = widget.currentQueueIndex;
    if (index == null ||
        index < 0 ||
        index >= widget.queueItems.length ||
        index == _lastCurrentQueueIndex) {
      return;
    }
    _lastCurrentQueueIndex = index;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || !_scrollController.hasClients) {
        return;
      }
      final double target = (index * _queueRowExtent - _queueRowExtent)
          .clamp(0, _scrollController.position.maxScrollExtent)
          .toDouble();
      _scrollController.animateTo(
        target,
        duration: const Duration(milliseconds: 220),
        curve: Curves.easeOutCubic,
      );
    });
  }

  Future<void> _moveTargetItemsNext(List<QueueItem> targets) async {
    final int? playingIndex = widget.currentQueueIndex;
    if (playingIndex == null) {
      return;
    }
    final List<String> itemOrder = widget.queueItems
        .map((QueueItem item) => item.queueItemId)
        .toList(growable: true);
    final Set<String> targetIds = targets
        .map((QueueItem item) => item.queueItemId)
        .where((String id) => id.isNotEmpty)
        .toSet();
    final List<String> orderedTargetIds = itemOrder
        .where(targetIds.contains)
        .toList(growable: false);
    int insertPosition = (playingIndex + 1).clamp(0, itemOrder.length);
    for (final String itemId in orderedTargetIds) {
      final int currentPosition = itemOrder.indexOf(itemId);
      if (currentPosition < 0) {
        continue;
      }
      if (currentPosition < insertPosition) {
        insertPosition--;
      }
      final int positionShift = insertPosition - currentPosition;
      if (positionShift != 0) {
        await widget.playbackCoordinator.moveQueueItem(
          itemId,
          posShift: positionShift,
        );
      }
      itemOrder
        ..removeAt(currentPosition)
        ..insert(insertPosition, itemId);
      insertPosition++;
    }
  }

  Future<void> _moveTargetItemsLast(List<QueueItem> targets) async {
    final List<String> itemOrder = widget.queueItems
        .map((QueueItem item) => item.queueItemId)
        .toList(growable: true);
    final Set<String> targetIds = targets
        .map((QueueItem item) => item.queueItemId)
        .where((String id) => id.isNotEmpty)
        .toSet();
    for (final String itemId in itemOrder.where(targetIds.contains).toList()) {
      final int currentPosition = itemOrder.indexOf(itemId);
      if (currentPosition < 0) {
        continue;
      }
      final int positionShift = itemOrder.length - 1 - currentPosition;
      if (positionShift != 0) {
        await widget.playbackCoordinator.moveQueueItem(
          itemId,
          posShift: positionShift,
        );
      }
      itemOrder
        ..removeAt(currentPosition)
        ..add(itemId);
    }
  }

  List<ContextMenuAction> _buildMenuActions(List<QueueItem> targets) {
    final List<Track> tracks = targets
        .map((QueueItem item) => item.mediaItem)
        .whereType<Track>()
        .toList(growable: false);
    final Track? track = tracks.length == 1 ? tracks.single : null;
    final UserDataController? controller = widget.userDataController;
    final bool areAllFavorites =
        tracks.isNotEmpty &&
        tracks.every(
          (Track item) => controller?.isFavorite(item) ?? item.favorite,
        );
    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        icon: Icons.play_arrow,
        onSelected: () {
          final int index = widget.queueItems.indexWhere(
            (QueueItem item) => item.queueItemId == targets.first.queueItemId,
          );
          return index < 0
              ? Future<void>.value()
              : widget.playbackCoordinator.playQueueIndex(index);
        },
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => _moveTargetItemsNext(targets),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => _moveTargetItemsLast(targets),
      ),
      ContextMenuAction(
        label: 'Aus Queue entfernen',
        icon: Icons.remove_circle_outline,
        onSelected: () async {
          for (final QueueItem target in targets) {
            if (target.queueItemId.isNotEmpty) {
              await widget.playbackCoordinator.deleteQueueItem(
                target.queueItemId,
              );
            }
          }
        },
      ),
      if (track != null &&
          (widget.onOpenArtist != null ||
              widget.onOpenAlbum != null)) ...<ContextMenuAction>[
        const ContextMenuAction.separator(),
        if (track.artists.isNotEmpty && widget.onOpenArtist != null)
          ContextMenuAction(
            label: 'Künstler:in öffnen',
            icon: Icons.person_outline,
            onSelected: () => widget.onOpenArtist!(track.artists.first),
          ),
        if (track.album != null && widget.onOpenAlbum != null)
          ContextMenuAction(
            label: 'Album öffnen',
            icon: Icons.album_outlined,
            onSelected: () => widget.onOpenAlbum!(track.album!),
          ),
      ],
      if (tracks.isNotEmpty &&
          controller != null &&
          controller.playlists.isNotEmpty) ...<ContextMenuAction>[
        const ContextMenuAction.separator(),
        ContextMenuAction.submenu(
          label: 'Zu Wiedergabeliste hinzufügen',
          icon: Icons.playlist_add_outlined,
          children: controller.playlists
              .map(
                (Playlist playlist) => ContextMenuAction(
                  label: playlist.name,
                  icon: Icons.playlist_play_outlined,
                  onSelected: () =>
                      controller.addPlaylistTracks(playlist.itemId, tracks),
                ),
              )
              .toList(growable: false),
        ),
      ],
      if (tracks.isNotEmpty && controller != null) ...<ContextMenuAction>[
        const ContextMenuAction.separator(),
        ContextMenuAction(
          label: areAllFavorites
              ? 'Aus Favoriten entfernen'
              : 'Zu Favoriten hinzufügen',
          icon: areAllFavorites
              ? Icons.favorite_outlined
              : Icons.favorite_border_outlined,
          onSelected: () => controller.setFavorite(tracks, !areAllFavorites),
        ),
      ],
    ];
  }

  Future<void> _showSelectionMenu(List<QueueItem> items) {
    return const AppContextMenuController().show(
      context: context,
      actions: _buildMenuActions(items),
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 16, 16, 12),
      child: Column(
        children: <Widget>[
          GestureDetector(
            onVerticalDragStart: (_) => _setDragging(true),
            onVerticalDragUpdate: (DragUpdateDetails details) {
              if (details.delta.dy > 0) {
                widget.onDismissDragUpdate(details.delta.dy);
              }
            },
            onVerticalDragEnd: (DragEndDetails details) {
              _setDragging(false);
              widget.onDismissDragEnd(details.primaryVelocity ?? 0);
            },
            onVerticalDragCancel: () => _setDragging(false),
            behavior: HitTestBehavior.opaque,
            child: Column(
              children: <Widget>[
                SizedBox(
                  height: 28,
                  child: Center(
                    child: MobileOverlayDragHandle(isDragging: _isDragging),
                  ),
                ),
                const SizedBox(height: 10),
                Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: <Widget>[
                    Text(
                      'Warteschlange',
                      style: TextStyle(
                        color: colors.textSecondary,
                        fontSize: 14,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Text(
                      '•',
                      style: TextStyle(
                        color: colors.textTertiary,
                        fontSize: 14,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Text(
                      widget.queueTrackCountText,
                      style: TextStyle(
                        color: colors.textTertiary,
                        fontSize: 14,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Text(
                      '•',
                      style: TextStyle(
                        color: colors.textTertiary,
                        fontSize: 14,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Text(
                      widget.queueTotalDurationText,
                      style: TextStyle(
                        color: colors.textTertiary,
                        fontSize: 14,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 10),
              ],
            ),
          ),
          Expanded(
            child: Listener(
              behavior: HitTestBehavior.translucent,
              onPointerUp: (_) {
                _setDragging(false);
                widget.onDismissDragEnd(0);
              },
              onPointerCancel: (_) {
                _setDragging(false);
                widget.onDismissDragEnd(0);
              },
              child: NotificationListener<ScrollNotification>(
                onNotification: _handleTableScrollNotification,
                child: CustomScrollView(
                  primary: false,
                  controller: _scrollController,
                  slivers: <Widget>[
                    MediaTableSectionMobile<QueueItem>(
                      title: ' ',
                      items: widget.queueItems,
                      isLoading: false,
                      emptyText: 'Queue ist leer',
                      maxItemsPerPage: 0,
                      enableSelection: true,
                      currentItemIndex: widget.currentQueueIndex,
                      playbackCoordinator: widget.playbackCoordinator,
                      onSelectionMenuTap: (List<QueueItem> items) =>
                          _showSelectionMenu(items),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
