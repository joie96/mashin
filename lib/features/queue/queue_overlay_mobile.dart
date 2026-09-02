import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/widgets/media_table_section_mobile.dart';

class QueueOverlayMobile extends StatefulWidget {
  final String queueTrackCountText;
  final String queueTotalDurationText;
  final List<QueueItem> queueItems;
  final int? currentQueueIndex;
  final PlaybackCoordinator playbackCoordinator;
  final VoidCallback onClose;
  final ValueChanged<double> onDismissDragUpdate;
  final ValueChanged<double> onDismissDragEnd;

  const QueueOverlayMobile({
    required this.queueTrackCountText,
    required this.queueTotalDurationText,
    required this.queueItems,
    required this.currentQueueIndex,
    required this.playbackCoordinator,
    required this.onClose,
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

  bool _handleTableScrollNotification(ScrollNotification notification) {
    if (notification.metrics.pixels > notification.metrics.minScrollExtent) {
      return false;
    }
    if (notification is OverscrollNotification && notification.overscroll < 0) {
      widget.onDismissDragUpdate(-notification.overscroll);
    } else if (notification is ScrollEndNotification) {
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

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 10, 16, 10),
      child: Column(
        children: <Widget>[
          GestureDetector(
            onVerticalDragUpdate: (DragUpdateDetails details) {
              if (details.delta.dy > 0) {
                widget.onDismissDragUpdate(details.delta.dy);
              }
            },
            onVerticalDragEnd: (DragEndDetails details) {
              widget.onDismissDragEnd(details.primaryVelocity ?? 0);
            },
            behavior: HitTestBehavior.opaque,
            child: Column(
              children: <Widget>[
                Container(
                  width: 52,
                  height: 4,
                  decoration: BoxDecoration(
                    color: colors.bgTertiary,
                    borderRadius: BorderRadius.circular(2),
                  ),
                ),
                const SizedBox(height: 14),
                Row(
                  children: <Widget>[
                    Text(
                      'Warteschlange',
                      style: TextStyle(
                        color: colors.textSecondary,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Text('•', style: TextStyle(color: colors.textTertiary)),
                    const SizedBox(width: 8),
                    Text(
                      widget.queueTrackCountText,
                      style: TextStyle(color: colors.textTertiary),
                    ),
                    const SizedBox(width: 8),
                    Text('•', style: TextStyle(color: colors.textTertiary)),
                    const SizedBox(width: 8),
                    Text(
                      widget.queueTotalDurationText,
                      style: TextStyle(color: colors.textTertiary),
                    ),
                    const Spacer(),
                    IconButton(
                      onPressed: widget.onClose,
                      icon: const Icon(Icons.close_outlined),
                    ),
                  ],
                ),
              ],
            ),
          ),
          Expanded(
            child: Listener(
              behavior: HitTestBehavior.translucent,
              onPointerUp: (_) => widget.onDismissDragEnd(0),
              onPointerCancel: (_) => widget.onDismissDragEnd(0),
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
