import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../../playback/playback_coordinator.dart';
import 'media_link_text.dart';
import 'media_list_section_desktop.dart';

class MediaListSectionMobile<T extends MediaItem> extends StatefulWidget {
  static const int _pageSize = 9;

  final String title;
  final List<T> items;
  final bool isLoading;
  final String emptyText;
  final double itemWidth;
  final PlaybackCoordinator? playbackCoordinator;
  final ValueChanged<T>? onItemTap;

  const MediaListSectionMobile({
    super.key,
    required this.title,
    required this.items,
    required this.isLoading,
    required this.emptyText,
    this.itemWidth = 320,
    this.playbackCoordinator,
    this.onItemTap,
  });

  @override
  State<MediaListSectionMobile<T>> createState() =>
      _MediaListSectionMobileState<T>();
}

class _MediaListSectionMobileState<T extends MediaItem>
    extends State<MediaListSectionMobile<T>> {
  int _loadedItemCount = MediaListSectionMobile._pageSize;

  @override
  void didUpdateWidget(covariant MediaListSectionMobile<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.items, widget.items)) {
      _loadedItemCount = MediaListSectionMobile._pageSize;
    }
    _loadedItemCount = _loadedItemCount.clamp(
      0,
      math.max(widget.items.length, 0),
    );
    if (widget.items.isNotEmpty && _loadedItemCount == 0) {
      _loadedItemCount = math.min(
        MediaListSectionMobile._pageSize,
        widget.items.length,
      );
    }
  }

  bool get _hasMoreItems => _loadedItemCount < widget.items.length;

  void _loadMore() {
    setState(() {
      _loadedItemCount = math.min(
        _loadedItemCount + MediaListSectionMobile._pageSize,
        widget.items.length,
      );
    });
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool hasTitle = widget.title.trim().isNotEmpty;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        if (hasTitle)
          Text(
            widget.title,
            style: Theme.of(
              context,
            ).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
          ),
        if (hasTitle) const SizedBox(height: 10),
        if (widget.isLoading)
          _MobileListSkeletonGrid(itemWidth: widget.itemWidth)
        else if (widget.items.isEmpty)
          Text(widget.emptyText, style: TextStyle(color: colors.textSecondary))
        else
          SizedBox(
            height: 166,
            child: ListView.separated(
              scrollDirection: Axis.horizontal,
              itemCount: _visibleColumnCount + (_hasMoreItems ? 1 : 0),
              separatorBuilder: (_, _) => const SizedBox(width: 8),
              itemBuilder: (BuildContext context, int columnIndex) {
                if (columnIndex >= _visibleColumnCount) {
                  return _MobileLoadMoreTile(onTap: _loadMore);
                }

                return SizedBox(
                  width: widget.itemWidth,
                  child: Column(
                    children: <Widget>[
                      for (int row = 0; row < 3; row++)
                        if ((columnIndex * 3) + row <
                            _visibleItemCount) ...<Widget>[
                          if (row > 0) const SizedBox(height: 8),
                          _MobileListTile<T>(
                            item: widget.items[(columnIndex * 3) + row],
                            playbackCoordinator: widget.playbackCoordinator,
                            onItemTap: widget.onItemTap,
                          ),
                        ],
                    ],
                  ),
                );
              },
            ),
          ),
      ],
    );
  }

  int get _visibleItemCount => math.min(_loadedItemCount, widget.items.length);
  int get _visibleColumnCount => (_visibleItemCount / 3).ceil();
}

class _MobileListTile<T extends MediaItem> extends StatelessWidget {
  final T item;
  final PlaybackCoordinator? playbackCoordinator;
  final ValueChanged<T>? onItemTap;

  const _MobileListTile({
    required this.item,
    required this.playbackCoordinator,
    required this.onItemTap,
  });

  void _play() {
    final PlaybackCoordinator? coordinator = playbackCoordinator;
    if (coordinator == null ||
        item.itemId.trim().isEmpty ||
        item.provider.trim().isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await coordinator.playMedia(<MediaItem>[item]);
      } catch (_) {
        // Playback failures should not block list taps.
      }
    }());
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final String title = item.name.trim().isEmpty
        ? 'Unbekannt'
        : item.name.trim();

    return Container(
      height: 50,
      decoration: BoxDecoration(
        color: colors.bgSecondary,
        borderRadius: BorderRadius.circular(10),
      ),
      clipBehavior: Clip.antiAlias,
      child: Row(
        children: <Widget>[
          Container(width: 10, color: accentColorFromText(title)),
          Expanded(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12),
              child: MediaLinkText(
                items: <MediaItem>[item],
                fallbackText: title,
                onItemTap: onItemTap == null
                    ? null
                    : (MediaItem mediaItem) => onItemTap!(mediaItem as T),
                style: TextStyle(
                  color: colors.textSecondary,
                  fontWeight: FontWeight.w700,
                  fontSize: 14,
                ),
              ),
            ),
          ),
          GestureDetector(
            onTap: _play,
            child: Container(
              width: 28,
              height: 28,
              margin: const EdgeInsets.only(right: 4),
              decoration: BoxDecoration(
                color: colors.overlayPlayButtonBackground,
                borderRadius: BorderRadius.circular(14),
              ),
              child: Icon(
                Icons.play_arrow_sharp,
                size: 18,
                color: colors.overlayPlayButtonForeground,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _MobileListSkeletonGrid extends StatelessWidget {
  final double itemWidth;

  const _MobileListSkeletonGrid({required this.itemWidth});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return SizedBox(
      height: 166,
      child: ListView.separated(
        scrollDirection: Axis.horizontal,
        itemCount: 3,
        separatorBuilder: (_, _) => const SizedBox(width: 8),
        itemBuilder: (BuildContext context, int columnIndex) {
          return SizedBox(
            width: itemWidth,
            child: Column(
              children: <Widget>[
                for (int row = 0; row < 3; row++) ...<Widget>[
                  if (row > 0) const SizedBox(height: 8),
                  Container(
                    height: 50,
                    decoration: BoxDecoration(
                      color: colors.bgSecondary,
                      borderRadius: BorderRadius.circular(10),
                    ),
                    child: Row(
                      children: <Widget>[
                        Container(width: 10, color: colors.bgTertiary),
                        const SizedBox(width: 12),
                        Container(
                          width: 180,
                          height: 20,
                          decoration: BoxDecoration(
                            color: colors.bgTertiary,
                            borderRadius: BorderRadius.circular(8),
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ],
            ),
          );
        },
      ),
    );
  }
}

class _MobileLoadMoreTile extends StatelessWidget {
  final VoidCallback onTap;

  const _MobileLoadMoreTile({required this.onTap});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return GestureDetector(
      onTap: onTap,
      child: Container(
        width: 166,
        height: 166,
        decoration: BoxDecoration(
          color: colors.hover,
          borderRadius: BorderRadius.circular(10),
        ),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: <Widget>[
            Icon(Icons.chevron_right, size: 30, color: colors.textPrimary),
            const SizedBox(height: 8),
            Text(
              'Mehr',
              style: TextStyle(
                color: colors.textSecondary,
                fontSize: 13,
                fontFamily: 'PoppinsRegular',
              ),
            ),
          ],
        ),
      ),
    );
  }
}
