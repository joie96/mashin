import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../../playback/playback_coordinator.dart';
import 'media_list_section_desktop.dart';
import 'media_table_section_desktop.dart';

class MediaListSectionMobile<T extends MediaItem> extends StatefulWidget {
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
  static const double _mobileChunkViewportFraction = 0.92;
  static const double _mobileChunkGap = 8;

  final PageController _mobileChunkPageController = PageController(
    viewportFraction: _mobileChunkViewportFraction,
  );

  @override
  void dispose() {
    _mobileChunkPageController.dispose();
    super.dispose();
  }

  List<MediaItem> _playableItems() {
    return widget.items.where(_isValidPlayable).toList(growable: false);
  }

  bool _isValidPlayable(MediaItem item) {
    return item.itemId.trim().isNotEmpty && item.provider.trim().isNotEmpty;
  }

  void _onPlayAllTap() {
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
    final List<MediaItem> playItems = _playableItems();
    if (coordinator == null || playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await coordinator.playMedia(<MediaItem>[playItems.first]);
      } catch (_) {
        // Playback failures should not block header actions.
      }
    }());
  }

  void _onShuffleAllTap() {
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
    final List<MediaItem> playItems = _playableItems();
    if (coordinator == null || playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        final MediaItem item =
            playItems[math.Random().nextInt(playItems.length)];
        await coordinator.playMedia(<MediaItem>[item]);
      } catch (_) {
        // Playback failures should not block header actions.
      }
    }());
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool hasTitle = widget.title.trim().isNotEmpty;
    final bool hasPlayableContent =
        !widget.isLoading && _playableItems().isNotEmpty;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        if (hasTitle)
          Row(
            children: <Widget>[
              Text(
                widget.title,
                style: Theme.of(
                  context,
                ).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
              ),
              if (!widget.isLoading && widget.items.isNotEmpty) ...<Widget>[
                const SizedBox(width: 10),
                SectionCountBadge(count: widget.items.length),
              ],
              const SizedBox(width: 16),
              const Expanded(child: SectionHeaderDivider()),
              const SizedBox(width: 16),
              if (widget.playbackCoordinator != null) ...<Widget>[
                SectionHeaderIconButton(
                  icon: Icons.play_arrow,
                  tooltip: 'Abspielen',
                  onTap: hasPlayableContent ? _onPlayAllTap : null,
                ),
                const SizedBox(width: 8),
                SectionHeaderIconButton(
                  icon: Icons.shuffle,
                  tooltip: 'Zufällig abspielen',
                  onTap: hasPlayableContent ? _onShuffleAllTap : null,
                ),
              ],
            ],
          ),
        if (hasTitle) const SizedBox(height: 10),
        if (widget.isLoading)
          _MobileListSkeletonGrid(itemWidth: widget.itemWidth)
        else if (widget.items.isEmpty)
          Text(widget.emptyText, style: TextStyle(color: colors.textSecondary))
        else
          SizedBox(
            height: 166,
            child: PageView.builder(
              controller: _mobileChunkPageController,
              padEnds: false,
              physics: const PageScrollPhysics(parent: BouncingScrollPhysics()),
              itemCount: _visibleColumnCount,
              itemBuilder: (BuildContext context, int columnIndex) {
                final bool isLastPage = columnIndex == _visibleColumnCount - 1;

                return Padding(
                  padding: EdgeInsets.only(
                    right: isLastPage ? 0 : _mobileChunkGap,
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: <Widget>[
                      for (int row = 0; row < 3; row++)
                        if ((columnIndex * 3) + row <
                            widget.items.length) ...<Widget>[
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

  int get _visibleColumnCount => (widget.items.length / 3).ceil();
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

    return GestureDetector(
      onTap: onItemTap == null ? null : () => onItemTap!(item),
      behavior: HitTestBehavior.opaque,
      child: Container(
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
                child: Text(
                  title,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
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
      child: PageView.builder(
        controller: PageController(viewportFraction: 0.92),
        padEnds: false,
        physics: const PageScrollPhysics(parent: BouncingScrollPhysics()),
        itemCount: 3,
        itemBuilder: (BuildContext context, int columnIndex) {
          return Padding(
            padding: EdgeInsets.only(right: columnIndex == 2 ? 0 : 8),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
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
