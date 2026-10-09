import 'dart:async';

import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';
import '../../core/core_bridge.dart';
import '../../core/core_models.dart';
import '../selection/mobile_section_selection.dart';
import 'media_list_section_desktop.dart';
import 'mobile_section_header.dart';

class MediaListSectionMobile<T extends MediaItem> extends StatefulWidget {
  final String title;
  final List<T> items;
  final bool isLoading;
  final String emptyText;
  final double itemWidth;
  final MashinCoreBridgeApi? coreBridge;
  final ValueChanged<T>? onItemTap;
  final ValueChanged<List<T>>? onSelectionMenuTap;

  const MediaListSectionMobile({
    super.key,
    required this.title,
    required this.items,
    required this.isLoading,
    required this.emptyText,
    this.itemWidth = 320,
    this.coreBridge,
    this.onItemTap,
    this.onSelectionMenuTap,
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
  late final MobileSectionSelectionState _selectionController;

  @override
  void initState() {
    super.initState();
    _selectionController = MobileSectionSelectionState(
      itemType: T,
      onMenuTap: _showSelectionMenu,
    );
    _selectionController.addListener(_onSelectionChanged);
  }

  @override
  void didUpdateWidget(covariant MediaListSectionMobile<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    _selectionController.updateItemCount(widget.items.length);
  }

  @override
  void dispose() {
    _mobileChunkPageController.dispose();
    _selectionController
      ..removeListener(_onSelectionChanged)
      ..disposeSelection()
      ..dispose();
    super.dispose();
  }

  void _onSelectionChanged() {
    if (mounted) {
      setState(() {});
    }
  }

  void _showSelectionMenu() {
    final ValueChanged<List<T>>? onSelectionMenuTap = widget.onSelectionMenuTap;
    if (onSelectionMenuTap == null) {
      return;
    }
    final List<T> selectedItems = _selectionController.selectedIndexes
        .where((int index) => index >= 0 && index < widget.items.length)
        .map((int index) => widget.items[index])
        .toList(growable: false);
    if (selectedItems.isNotEmpty) {
      onSelectionMenuTap(selectedItems);
    }
  }

  List<MediaItem> _playableItems() {
    return widget.items.where(_isValidPlayable).toList(growable: false);
  }

  bool _isValidPlayable(MediaItem item) {
    return item.canPlay &&
        item.itemId.trim().isNotEmpty &&
        item.provider.trim().isNotEmpty;
  }

  void _onPlayAllTap() {
    final MashinCoreBridgeApi? bridge = widget.coreBridge;
    final List<MediaItem> playItems = _playableItems();
    if (bridge == null || playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await bridge.queuePlayMedia(<MediaItem>[playItems.first]);
      } catch (_) {
        // Playback failures should not block header actions.
      }
    }());
  }

  void _onShuffleAllTap() {
    final MashinCoreBridgeApi? bridge = widget.coreBridge;
    final List<MediaItem> playItems = _playableItems();
    if (bridge == null || playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await bridge.queuePlayMedia(playItems, shuffle: true);
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
          MobileSectionHeader(
            title: widget.title,
            itemCount: !widget.isLoading && widget.items.isNotEmpty
                ? widget.items.length
                : null,
            showPlaybackActions: widget.coreBridge != null,
            onPlayTap: hasPlayableContent ? _onPlayAllTap : null,
            onShuffleTap: hasPlayableContent ? _onShuffleAllTap : null,
            shuffleTooltip: 'Zufällig abspielen',
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
                          MobileSelectableSectionItem(
                            selectionModeActive:
                                _selectionController.isSelectionModeActive,
                            isSelected: _selectionController.isSelected(
                              (columnIndex * 3) + row,
                            ),
                            onLongPress: () => _selectionController.select(
                              (columnIndex * 3) + row,
                              widget.items.length,
                            ),
                            onToggleSelection: () =>
                                _selectionController.toggle(
                                  (columnIndex * 3) + row,
                                  widget.items.length,
                                ),
                            onTap: widget.onItemTap == null
                                ? null
                                : () => widget.onItemTap!(
                                    widget.items[(columnIndex * 3) + row],
                                  ),
                            child: _MobileListTile<T>(
                              item: widget.items[(columnIndex * 3) + row],
                              coreBridge: widget.coreBridge,
                              onItemTap: null,
                            ),
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
  final MashinCoreBridgeApi? coreBridge;
  final ValueChanged<T>? onItemTap;

  const _MobileListTile({
    required this.item,
    required this.coreBridge,
    required this.onItemTap,
  });

  void _play() {
    final MashinCoreBridgeApi? bridge = coreBridge;
    if (bridge == null ||
        !item.canPlay ||
        item.itemId.trim().isEmpty ||
        item.provider.trim().isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await bridge.queuePlayMedia(<MediaItem>[item]);
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

    return Opacity(
      opacity: item.canPlay ? 1 : 0.35,
      child: GestureDetector(
        onTap: onItemTap == null || (item is Track && !item.canPlay)
            ? null
            : () => onItemTap!(item),
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
                      fontWeight: FontWeight.w500,
                      fontSize: 14,
                    ),
                  ),
                ),
              ),
              GestureDetector(
                onTap: item.canPlay ? _play : null,
                child: Container(
                  width: 28,
                  height: 28,
                  margin: const EdgeInsets.only(right: 4),
                  decoration: BoxDecoration(
                    color: colors.overlayPlayButtonBackground.withValues(
                      alpha: 0.88,
                    ),
                    borderRadius: BorderRadius.circular(24),
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
