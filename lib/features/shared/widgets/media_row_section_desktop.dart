import 'package:flutter/material.dart';
import 'package:flutter/gestures.dart';
import 'dart:math' as math;
import 'dart:async';

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../../playback/playback_coordinator.dart';
import 'media_table_section_desktop.dart';

class MediaRowSectionDesktop<T> extends StatefulWidget {
  static const double _defaultHeight = 226;
  static const double _reservedTextHeight = 80;

  final String title;
  final bool isLoading;
  final List<T> items;
  final String emptyText;
  final double minItemWith;
  final double itemSpacing;
  final int skeletonCount;
  final PlaybackCoordinator? playbackCoordinator;
  final DesktopRowAction<T>? onRowMoreTap;
  final DesktopRowAction<T>? onRowFavoriteTap;
  final DesktopRowAction<T>? onRowSelectTap;
  final DesktopRowFavoriteResolver<T>? isRowFavorite;
  final DesktopRowFavoriteResolver<T>? isRowSelected;

  const MediaRowSectionDesktop({
    super.key,
    required this.title,
    required this.isLoading,
    required this.items,
    required this.emptyText,
    this.minItemWith = 160,
    this.itemSpacing = 16,
    this.skeletonCount = 8,
    this.playbackCoordinator,
    this.onRowMoreTap,
    this.onRowFavoriteTap,
    this.onRowSelectTap,
    this.isRowFavorite,
    this.isRowSelected,
  });

  @override
  State<MediaRowSectionDesktop<T>> createState() => _MediaRowSectionDesktopState<T>();
}

class _MediaRowSectionDesktopState<T> extends State<MediaRowSectionDesktop<T>> {
  static const Duration _pageOutDuration = Duration(milliseconds: 220);
  static const Duration _pageInDuration = Duration(milliseconds: 220);
  static const Curve _pageTransitionCurve = Curves.easeInOutCubic;
  static const double _chunkTranslatePx = 50;

  int _pageStartIndex = 0;
  int _navigationDirection = 1;
  int? _outgoingStartIndex;
  int? _incomingStartIndex;
  bool _isAnimatingOut = false;
  bool _isAnimatingIn = false;
  int _transitionRunId = 0;

  bool _isItemFavorite(T item, int index) {
    return widget.isRowFavorite?.call(item, index) ?? false;
  }

  bool _isItemSelected(T item, int index) {
    return widget.isRowSelected?.call(item, index) ?? false;
  }

  _RowLayoutMetrics _computeLayoutMetrics(double maxWidth) {
    final int itemCount = widget.items.length;

    if (itemCount <= 0) {
      return const _RowLayoutMetrics(visibleCount: 1, computedItemWidth: 0);
    }

    final double minItemWidth = widget.minItemWith > 0 ? widget.minItemWith : 164;
    final double spacing = widget.itemSpacing;

    if (!maxWidth.isFinite || maxWidth <= 0) {
      final int fallbackVisible = math.max(1, itemCount);
      return _RowLayoutMetrics(visibleCount: fallbackVisible, computedItemWidth: minItemWidth);
    }

    final double denominator = minItemWidth + spacing;
    int fitByMinSize = denominator > 0 ? ((maxWidth + spacing) / denominator).floor() : 1;
    fitByMinSize = math.max(1, fitByMinSize);

    final int visibleCount = math.min(itemCount, fitByMinSize);
    final double computedWidth = (maxWidth - ((visibleCount - 1) * spacing)) / visibleCount;

    return _RowLayoutMetrics(
      visibleCount: visibleCount,
      computedItemWidth: math.max(minItemWidth, computedWidth),
    );
  }

  double _resolveCoverSize(double resolvedItemWidth) {
    return math.max(1, resolvedItemWidth);
  }

  @override
  void didUpdateWidget(covariant MediaRowSectionDesktop<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.items.isEmpty) {
      _pageStartIndex = 0;
      return;
    }
    if (_pageStartIndex >= widget.items.length) {
      _pageStartIndex = widget.items.length - 1;
    }
    if (_pageStartIndex < 0) {
      _pageStartIndex = 0;
    }
  }

  int _clampStartIndex({required int visibleCount}) {
    if (widget.items.isEmpty) {
      return 0;
    }
    final int pageCount = (widget.items.length / visibleCount).ceil();
    final int maxStart = math.max(0, (pageCount - 1) * visibleCount);
    return _pageStartIndex.clamp(0, maxStart);
  }

  bool get _isTransitionRunning => _isAnimatingOut || _isAnimatingIn;

  void _finishRunningTransitionImmediately() {
    if (!_isTransitionRunning) {
      return;
    }

    _transitionRunId++;
    if (_incomingStartIndex != null) {
      _pageStartIndex = _incomingStartIndex!;
    }

    _outgoingStartIndex = null;
    _incomingStartIndex = null;
    _isAnimatingOut = false;
    _isAnimatingIn = false;
  }

  Future<void> _startPageTransition({
    required int targetStartIndex,
    required int direction,
  }) async {
    if (_isTransitionRunning || targetStartIndex == _pageStartIndex) {
      return;
    }

    final int runId = ++_transitionRunId;

    setState(() {
      _navigationDirection = direction;
      _outgoingStartIndex = _pageStartIndex;
      _incomingStartIndex = targetStartIndex;
      _isAnimatingOut = true;
      _isAnimatingIn = false;
    });

    await Future<void>.delayed(_pageOutDuration);
    if (!mounted || runId != _transitionRunId) {
      return;
    }

    setState(() {
      _pageStartIndex = targetStartIndex;
      _outgoingStartIndex = null;
      _isAnimatingOut = false;
      _isAnimatingIn = true;
    });

    await Future<void>.delayed(_pageInDuration);
    if (!mounted || runId != _transitionRunId) {
      return;
    }

    setState(() {
      _incomingStartIndex = null;
      _isAnimatingIn = false;
    });
  }

  List<T> _sliceItems({required int startIndex, required int visibleCount}) {
    if (widget.items.isEmpty) {
      return <T>[];
    }
    final int safeStart = startIndex.clamp(0, widget.items.length - 1);
    final int end = math.min(widget.items.length, safeStart + visibleCount);
    return widget.items.sublist(safeStart, end);
  }

  MediaItem? _resolvePlayableMediaItem(Object? rawItem) {
    if (rawItem is QueueItem) {
      return rawItem.mediaItem;
    }
    if (rawItem is MediaItem) {
      return rawItem;
    }
    return null;
  }

  bool _isValidPlayable(MediaItem item) {
    return item.itemId.trim().isNotEmpty && item.provider.trim().isNotEmpty;
  }

  List<MediaItem> _playableItemsFromIndex(int startIndex) {
    if (widget.items.isEmpty || startIndex < 0 || startIndex >= widget.items.length) {
      return const <MediaItem>[];
    }

    final List<MediaItem> playItems = <MediaItem>[];
    for (int i = startIndex; i < widget.items.length; i++) {
      final MediaItem? resolved = _resolvePlayableMediaItem(widget.items[i]);
      if (resolved == null || !_isValidPlayable(resolved)) {
        continue;
      }
      playItems.add(resolved);
    }

    return playItems;
  }

  void _onPlayTap(int absoluteIndex) {
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
    if (coordinator == null) {
      return;
    }

    final List<MediaItem> playItems = _playableItemsFromIndex(absoluteIndex);
    if (playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await coordinator.playMedia(playItems);
      } catch (_) {
        // Ignore failures to keep row interactions responsive.
      }
    }());
  }

  Widget _buildSlideList({
    required int startIndex,
    required int visibleCount,
    required double resolvedItemWidth,
    required double resolvedCoverSize,
  }) {
    final List<T> visibleItems = _sliceItems(startIndex: startIndex, visibleCount: visibleCount);

    return ListView.separated(
      key: ValueKey<int>(startIndex),
      scrollDirection: Axis.horizontal,
      physics: const NeverScrollableScrollPhysics(),
      itemCount: visibleItems.length,
      separatorBuilder: (_, _) => SizedBox(width: widget.itemSpacing),
      itemBuilder: (BuildContext context, int index) {
        final int absoluteIndex = startIndex + index;
        final Object? item = visibleItems[index];
        if (item is Album) {
          return _buildDesktopMediaRowAlbum(
            item: item,
            index: absoluteIndex,
            itemWidth: resolvedItemWidth,
            coverSize: resolvedCoverSize,
            onPlayTap: () => _onPlayTap(absoluteIndex),
            isFavorite: _isItemFavorite(item as T, absoluteIndex),
            onFavoriteTap: widget.onRowFavoriteTap == null ? null : () => widget.onRowFavoriteTap!(item as T, absoluteIndex),
            onMoreTap: widget.onRowMoreTap == null ? null : () => widget.onRowMoreTap!(item as T, absoluteIndex),
            isSelected: _isItemSelected(item as T, absoluteIndex),
            onSelectTap: widget.onRowSelectTap == null ? null : () => widget.onRowSelectTap!(item as T, absoluteIndex),
          );
        }
        if (item is Playlist) {
          return _buildDesktopMediaRowPlaylist(
            item: item,
            index: absoluteIndex,
            itemWidth: resolvedItemWidth,
            coverSize: resolvedCoverSize,
            onPlayTap: () => _onPlayTap(absoluteIndex),
            isFavorite: _isItemFavorite(item as T, absoluteIndex),
            onFavoriteTap: widget.onRowFavoriteTap == null ? null : () => widget.onRowFavoriteTap!(item as T, absoluteIndex),
            onMoreTap: widget.onRowMoreTap == null ? null : () => widget.onRowMoreTap!(item as T, absoluteIndex),
            isSelected: _isItemSelected(item as T, absoluteIndex),
            onSelectTap: widget.onRowSelectTap == null ? null : () => widget.onRowSelectTap!(item as T, absoluteIndex),
          );
        }
        if (item is Artist) {
          return _buildDesktopMediaRowArtist(
            item: item,
            index: absoluteIndex,
            itemWidth: resolvedItemWidth,
            coverSize: resolvedCoverSize,
            onPlayTap: () => _onPlayTap(absoluteIndex),
            isFavorite: _isItemFavorite(item as T, absoluteIndex),
            onFavoriteTap: widget.onRowFavoriteTap == null ? null : () => widget.onRowFavoriteTap!(item as T, absoluteIndex),
            onMoreTap: widget.onRowMoreTap == null ? null : () => widget.onRowMoreTap!(item as T, absoluteIndex),
            isSelected: _isItemSelected(item as T, absoluteIndex),
            onSelectTap: widget.onRowSelectTap == null ? null : () => widget.onRowSelectTap!(item as T, absoluteIndex),
          );
        }
        if (item is Track) {
          return _buildDesktopMediaRowTrack(
            item: item,
            index: absoluteIndex,
            itemWidth: resolvedItemWidth,
            coverSize: resolvedCoverSize,
            onPlayTap: () => _onPlayTap(absoluteIndex),
            isFavorite: _isItemFavorite(item as T, absoluteIndex),
            onFavoriteTap: widget.onRowFavoriteTap == null ? null : () => widget.onRowFavoriteTap!(item as T, absoluteIndex),
            onMoreTap: widget.onRowMoreTap == null ? null : () => widget.onRowMoreTap!(item as T, absoluteIndex),
            isSelected: _isItemSelected(item as T, absoluteIndex),
            onSelectTap: widget.onRowSelectTap == null ? null : () => widget.onRowSelectTap!(item as T, absoluteIndex),
          );
        }

        return _buildDesktopMediaRowUnknown(
          index: absoluteIndex,
          itemWidth: resolvedItemWidth,
          coverSize: resolvedCoverSize,
          onPlayTap: () => _onPlayTap(absoluteIndex),
          isFavorite: _isItemFavorite(visibleItems[index], absoluteIndex),
          onFavoriteTap: widget.onRowFavoriteTap == null ? null : () => widget.onRowFavoriteTap!(visibleItems[index], absoluteIndex),
          onMoreTap: widget.onRowMoreTap == null ? null : () => widget.onRowMoreTap!(visibleItems[index], absoluteIndex),
          isSelected: _isItemSelected(visibleItems[index], absoluteIndex),
          onSelectTap: widget.onRowSelectTap == null ? null : () => widget.onRowSelectTap!(visibleItems[index], absoluteIndex),
        );
      },
    );
  }

  void _goToPreviousPage(int visibleCount) {
    if (_isTransitionRunning) {
      setState(_finishRunningTransitionImmediately);
    }

    if (_pageStartIndex <= 0) {
      return;
    }

    final int target = math.max(0, _pageStartIndex - visibleCount);
    _startPageTransition(targetStartIndex: target, direction: -1);
  }

  void _goToNextPage(int visibleCount, int pageCount) {
    if (_isTransitionRunning) {
      setState(_finishRunningTransitionImmediately);
    }

    final int maxStart = math.max(0, (pageCount - 1) * visibleCount);
    if (_pageStartIndex >= maxStart) {
      return;
    }
    final int target = math.min(maxStart, _pageStartIndex + visibleCount);
    _startPageTransition(targetStartIndex: target, direction: 1);
  }

  Widget _buildPaginationControls(BuildContext context, {required int visibleCount, required int pageCount}) {
    final int clampedStart = _clampStartIndex(visibleCount: visibleCount);
    final int maxStart = math.max(0, (pageCount - 1) * visibleCount);
    final bool canGoBack = clampedStart > 0;
    final bool canGoNext = clampedStart < maxStart;

    return Row(
      mainAxisSize: MainAxisSize.min,
      children: <Widget>[
        IconButton(
          tooltip: 'Vorherige Seite',
          onPressed: canGoBack ? () => _goToPreviousPage(visibleCount) : null,
          icon: const Icon(Icons.chevron_left_outlined),
          visualDensity: VisualDensity.compact,
        ),
        IconButton(
          tooltip: 'Nächste Seite',
          onPressed: canGoNext ? () => _goToNextPage(visibleCount, pageCount) : null,
          icon: const Icon(Icons.chevron_right_outlined),
          visualDensity: VisualDensity.compact,
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool hasTitle = widget.title.trim().isNotEmpty;

    return LayoutBuilder(
      builder: (BuildContext context, BoxConstraints constraints) {
        final _RowLayoutMetrics metrics = _computeLayoutMetrics(constraints.maxWidth);
        final int visibleCount = metrics.visibleCount;
        final double resolvedItemWidth = metrics.computedItemWidth;
        final double resolvedCoverSize = _resolveCoverSize(resolvedItemWidth);
        final double sectionHeight = math.max(
          MediaRowSectionDesktop._defaultHeight,
          resolvedCoverSize + MediaRowSectionDesktop._reservedTextHeight,
        );
        final int pageCount = widget.items.isEmpty ? 1 : (widget.items.length / visibleCount).ceil();
        final bool isPagingEnabled = pageCount > 1;
        _pageStartIndex = _clampStartIndex(visibleCount: visibleCount);

        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            if (hasTitle || (isPagingEnabled && !widget.isLoading && widget.items.isNotEmpty))
              Row(
                children: <Widget>[
                  if (hasTitle)
                    Expanded(
                      child: Text(
                        widget.title,
                        style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
                      ),
                    )
                  else
                    const Spacer(),
                  if (isPagingEnabled && !widget.isLoading && widget.items.isNotEmpty)
                    _buildPaginationControls(context, visibleCount: visibleCount, pageCount: pageCount),
                ],
              ),
            if (hasTitle || (isPagingEnabled && !widget.isLoading && widget.items.isNotEmpty))
              const SizedBox(height: 10),
            SizedBox(
              height: sectionHeight,
              child: widget.isLoading
                  ? ListView.separated(
                      scrollDirection: Axis.horizontal,
                      itemCount: widget.skeletonCount,
                      separatorBuilder: (_, _) => SizedBox(width: widget.itemSpacing),
                      itemBuilder: (_, _) {
                        if (T == Album) {
                          return _buildDesktopMediaRowAlbumSkeleton(
                            itemWidth: resolvedItemWidth,
                            coverSize: resolvedCoverSize,
                          );
                        }
                        if (T == Playlist) {
                          return _buildDesktopMediaRowPlaylistSkeleton(
                            itemWidth: resolvedItemWidth,
                            coverSize: resolvedCoverSize,
                          );
                        }
                        if (T == Artist) {
                          return _buildDesktopMediaRowArtistSkeleton(
                            itemWidth: resolvedItemWidth,
                            coverSize: resolvedCoverSize,
                          );
                        }
                        if (T == Track) {
                          return _buildDesktopMediaRowTrackSkeleton(
                            itemWidth: resolvedItemWidth,
                            coverSize: resolvedCoverSize,
                          );
                        }

                        return _buildDesktopMediaRowUnknownSkeleton(
                          itemWidth: resolvedItemWidth,
                          coverSize: resolvedCoverSize,
                        );
                      },
                    )
                  : widget.items.isEmpty
                      ? Container(
                          width: double.infinity,
                          padding: const EdgeInsets.all(16),
                          decoration: BoxDecoration(
                            borderRadius: BorderRadius.circular(12),
                            border: Border.all(color: colors.border),
                            color: colors.bgSecondary,
                          ),
                          child: Align(
                            alignment: Alignment.centerLeft,
                            child: Text(
                              widget.emptyText,
                              style: TextStyle(color: colors.textSecondary),
                            ),
                          ),
                        )
                      : ClipRect(
                          child: Stack(
                            fit: StackFit.expand,
                            children: <Widget>[
                              if (_isAnimatingOut && _outgoingStartIndex != null)
                                TweenAnimationBuilder<double>(
                                  key: ValueKey<int>(_outgoingStartIndex!),
                                  tween: Tween<double>(begin: 0, end: 1),
                                  duration: _pageOutDuration,
                                  curve: _pageTransitionCurve,
                                  builder: (BuildContext context, double value, Widget? child) {
                                    final double dx = (_navigationDirection >= 0 ? -1 : 1) * _chunkTranslatePx * value;
                                    return Opacity(
                                      opacity: 1 - value,
                                      child: Transform.translate(
                                        offset: Offset(dx, 0),
                                        child: child,
                                      ),
                                    );
                                  },
                                  child: _buildSlideList(
                                    startIndex: _outgoingStartIndex!,
                                    visibleCount: visibleCount,
                                    resolvedItemWidth: resolvedItemWidth,
                                    resolvedCoverSize: resolvedCoverSize,
                                  ),
                                )
                              else if (_isAnimatingIn && _incomingStartIndex != null)
                                TweenAnimationBuilder<double>(
                                  key: ValueKey<int>(_incomingStartIndex!),
                                  tween: Tween<double>(begin: 0, end: 1),
                                  duration: _pageInDuration,
                                  curve: _pageTransitionCurve,
                                  builder: (BuildContext context, double value, Widget? child) {
                                    final double dx = (_navigationDirection >= 0 ? 1 : -1) * _chunkTranslatePx * (1 - value);
                                    return Opacity(
                                      opacity: value,
                                      child: Transform.translate(
                                        offset: Offset(dx, 0),
                                        child: child,
                                      ),
                                    );
                                  },
                                  child: _buildSlideList(
                                    startIndex: _incomingStartIndex!,
                                    visibleCount: visibleCount,
                                    resolvedItemWidth: resolvedItemWidth,
                                    resolvedCoverSize: resolvedCoverSize,
                                  ),
                                )
                              else
                                _buildSlideList(
                                  startIndex: _pageStartIndex,
                                  visibleCount: visibleCount,
                                  resolvedItemWidth: resolvedItemWidth,
                                  resolvedCoverSize: resolvedCoverSize,
                                ),
                            ],
                          ),
                        ),
            ),
          ],
        );
      },
    );
  }
}

class _RowLayoutMetrics {
  final int visibleCount;
  final double computedItemWidth;

  const _RowLayoutMetrics({
    required this.visibleCount,
    required this.computedItemWidth,
  });
}

// Typed desktop row builders
Widget _buildDesktopMediaRowAlbum({
  required Album item,
  required int index,
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
  required bool isFavorite,
  required VoidCallback? onFavoriteTap,
  required VoidCallback? onMoreTap,
  required bool isSelected,
  required VoidCallback? onSelectTap,
}) {
  return _buildDesktopMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.album_outlined,
    title: _prefer(item.name, 'Unknown Album'),
    secondaryText: _albumSubtitle(item),
    tertiaryText: _albumTertiary(item),
    coverUrl: item.imageUri ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
    onPlayTap: onPlayTap,
    isFavorite: isFavorite,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    isSelected: isSelected,
    onSelectTap: onSelectTap,
  );
}

Widget _buildDesktopMediaRowPlaylist({
  required Playlist item,
  required int index,
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
  required bool isFavorite,
  required VoidCallback? onFavoriteTap,
  required VoidCallback? onMoreTap,
  required bool isSelected,
  required VoidCallback? onSelectTap,
}) {
  return _buildDesktopMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.queue_music_outlined,
    title: _prefer(item.name, 'Unknown Playlist'),
    secondaryText: _playlistSubtitle(item),
    tertiaryText: _playlistTertiary(item),
    coverUrl: item.imageUri ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
    onPlayTap: onPlayTap,
    isFavorite: isFavorite,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    isSelected: isSelected,
    onSelectTap: onSelectTap,
  );
}

Widget _buildDesktopMediaRowArtist({
  required Artist item,
  required int index,
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
  required bool isFavorite,
  required VoidCallback? onFavoriteTap,
  required VoidCallback? onMoreTap,
  required bool isSelected,
  required VoidCallback? onSelectTap,
}) {
  return _buildDesktopMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.person_outlined,
    title: _prefer(item.name, 'Unknown Artist'),
    secondaryText: 'Artist',
    tertiaryText: _artistTertiary(item),
    coverUrl: item.imageUri ?? '',
    isCircularCover: true,
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
    onPlayTap: onPlayTap,
    isFavorite: isFavorite,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    isSelected: isSelected,
    onSelectTap: onSelectTap,
  );
}

Widget _buildDesktopMediaRowTrack({
  required Track item,
  required int index,
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
  required bool isFavorite,
  required VoidCallback? onFavoriteTap,
  required VoidCallback? onMoreTap,
  required bool isSelected,
  required VoidCallback? onSelectTap,
}) {
  return _buildDesktopMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.music_note_outlined,
    title: _prefer(item.name, 'Unknown Track'),
    secondaryText: _trackSubtitle(item),
    tertiaryText: _trackTertiary(item),
    coverUrl: item.imageUri ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
    onPlayTap: onPlayTap,
    isFavorite: isFavorite,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    isSelected: isSelected,
    onSelectTap: onSelectTap,
  );
}

Widget _buildDesktopMediaRowUnknown({
  required int index,
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
  required bool isFavorite,
  required VoidCallback? onFavoriteTap,
  required VoidCallback? onMoreTap,
  required bool isSelected,
  required VoidCallback? onSelectTap,
}) {
  return _buildDesktopMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.music_note_outlined,
    title: 'Unknown Item',
    secondaryText: '-',
    tertiaryText: '-',
    coverUrl: '',
    providerLabel: '-',
    providerIconSvg: null,
    onPlayTap: onPlayTap,
    isFavorite: isFavorite,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    isSelected: isSelected,
    onSelectTap: onSelectTap,
  );
}

Widget _buildDesktopMediaRowItemBase({
  required double itemWidth,
  required double coverSize,
  required IconData icon,
  required String title,
  required String? secondaryText,
  required String? tertiaryText,
  required String? coverUrl,
  required String providerLabel,
  required String? providerIconSvg,
  required VoidCallback? onPlayTap,
  required bool isFavorite,
  required VoidCallback? onFavoriteTap,
  required VoidCallback? onMoreTap,
  required bool isSelected,
  required VoidCallback? onSelectTap,
  bool showPlayOverlay = true,
  bool isCircularCover = false,
}) {
  return Builder(
    builder: (BuildContext context) {
      final colors = context.mashinColors;
      final bool hasSecondary = secondaryText != null && secondaryText.trim().isNotEmpty;
      final bool hasTertiary = tertiaryText != null && tertiaryText.trim().isNotEmpty;

      return SizedBox(
        width: itemWidth,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            _DesktopMediaCover(
              coverSize: coverSize,
              icon: icon,
              coverUrl: coverUrl,
              providerLabel: providerLabel,
              providerIconSvg: providerIconSvg,
              onPlayTap: onPlayTap,
              isFavorite: isFavorite,
              onFavoriteTap: onFavoriteTap,
              onMoreTap: onMoreTap,
              isSelected: isSelected,
              onSelectTap: onSelectTap,
              showPlayOverlay: showPlayOverlay,
              isCircularCover: isCircularCover,
            ),
            const SizedBox(height: 12),
            Text(
              title,
              maxLines: 1,
              textAlign: TextAlign.left,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(context).textTheme.titleSmall?.copyWith(fontWeight: FontWeight.w400, fontSize: 15),
            ),
            if (hasSecondary) ...<Widget>[
              const SizedBox(height: 2),
              Text(
                secondaryText,
                maxLines: 1,
                textAlign: TextAlign.left,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: colors.textSecondary, fontSize: 14),
              ),
            ],
            if (hasTertiary) ...<Widget>[
              const SizedBox(height: 2),
              Text(
                tertiaryText,
                maxLines: 1,
                textAlign: TextAlign.left,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: colors.textTertiary, fontSize: 14),
              ),
            ],
          ],
        ),
      );
    },
  );
}

class _DesktopMediaCover extends StatefulWidget {
  final double coverSize;
  final IconData icon;
  final String? coverUrl;
  final String providerLabel;
  final String? providerIconSvg;
  final VoidCallback? onPlayTap;
  final bool isFavorite;
  final VoidCallback? onFavoriteTap;
  final VoidCallback? onMoreTap;
  final bool isSelected;
  final VoidCallback? onSelectTap;
  final bool showPlayOverlay;
  final bool isCircularCover;

  const _DesktopMediaCover({
    required this.coverSize,
    required this.icon,
    required this.coverUrl,
    required this.providerLabel,
    required this.providerIconSvg,
    required this.onPlayTap,
    required this.isFavorite,
    required this.onFavoriteTap,
    required this.onMoreTap,
    required this.isSelected,
    required this.onSelectTap,
    required this.showPlayOverlay,
    required this.isCircularCover,
  });

  @override
  State<_DesktopMediaCover> createState() => _DesktopMediaCoverState();
}

class _DesktopMediaCoverState extends State<_DesktopMediaCover> {
  static const Duration _hoverFadeDuration = Duration(milliseconds: 140);
  static const Duration _buttonScaleDuration = Duration(milliseconds: 140);
  bool _isHovered = false;
  bool _isPlayHovered = false;

  bool _isPrimaryActivationPointer(PointerDownEvent event) {
    if (event.kind == PointerDeviceKind.mouse) {
      return (event.buttons & kPrimaryMouseButton) != 0;
    }
    return true;
  }

  void _setPlayHovered(bool value) {
    if (_isPlayHovered == value || widget.onPlayTap == null) {
      return;
    }
    setState(() {
      _isPlayHovered = value;
    });
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool hasCover = widget.coverUrl != null && widget.coverUrl!.trim().isNotEmpty;
    final bool showHoverPlay = widget.showPlayOverlay && widget.onPlayTap != null;
    const bool showSelectControl = true;
    final bool showSelectControlVisible = _isHovered || widget.isSelected;
    const bool showOverlayActions = true;

    final Widget clippedContent = SizedBox(
      width: widget.coverSize,
      height: widget.coverSize,
      child: Stack(
        fit: StackFit.expand,
        children: <Widget>[
          Container(
            color: colors.bgTertiary,
            child: widget.isCircularCover
                ? ClipOval(
                    child: hasCover
                        ? Image.network(
                            widget.coverUrl!,
                            fit: BoxFit.cover,
                            errorBuilder: (_, _, _) => Icon(widget.icon, color: colors.accent, size: 34),
                          )
                        : Icon(widget.icon, color: colors.accent, size: 34),
                  )
                : hasCover
                    ? Image.network(
                        widget.coverUrl!,
                        fit: BoxFit.cover,
                        errorBuilder: (_, _, _) => Icon(widget.icon, color: colors.accent, size: 34),
                      )
                    : Icon(widget.icon, color: colors.accent, size: 34),
          ),
          if (showHoverPlay)
            IgnorePointer(
              child: AnimatedOpacity(
                duration: _hoverFadeDuration,
                opacity: _isHovered ? 1 : 0,
                child: Container(color: Colors.black.withValues(alpha: 0.34)),
              ),
            ),
          Positioned(
            left: 6,
            top: 6,
            child: MediaProviderIcon(
              providerLabel: widget.providerLabel,
              iconSvg: widget.providerIconSvg,
              size: 18,
            ),
          ),
          if (showHoverPlay)
            Positioned.fill(
              child: Center(
                child: AnimatedOpacity(
                  duration: _hoverFadeDuration,
                  opacity: _isHovered ? 1 : 0,
                  child: MouseRegion(
                    onEnter: (_) => _setPlayHovered(true),
                    onExit: (_) => _setPlayHovered(false),
                    child: AnimatedScale(
                      scale: _isPlayHovered ? 1.08 : 1,
                      duration: _buttonScaleDuration,
                      curve: Curves.easeOutCubic,
                      child: Listener(
                        onPointerDown: (PointerDownEvent event) {
                          if (_isPrimaryActivationPointer(event)) {
                            widget.onPlayTap?.call();
                          }
                        },
                        child: Container(
                          width: 56,
                          height: 56,
                          decoration: BoxDecoration(
                            color: colors.textPrimary,
                            borderRadius: BorderRadius.circular(28),
                          ),
                          child: Icon(
                            Icons.play_arrow_sharp,
                            size: 30,
                            color: colors.bgPrimary,
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          if (showOverlayActions)
            Positioned(
              left: 8,
              right: 8,
              bottom: 8,
              child: AnimatedOpacity(
                duration: _hoverFadeDuration,
                opacity: _isHovered ? 1 : 0,
                child: Row(
                  children: <Widget>[
                    const Spacer(),
                    _DesktopCoverOverlayIconButton(
                      icon: widget.isFavorite ? Icons.favorite_outlined : Icons.favorite_border_outlined,
                      onTap: widget.onFavoriteTap,
                      baseColor: colors.textSecondary,
                      hoverColor: colors.textPrimary,
                    ),
                    const SizedBox(width: 12),
                    _DesktopCoverOverlayIconButton(
                      icon: Icons.more_horiz_outlined,
                      onTap: widget.onMoreTap,
                      baseColor: colors.textSecondary,
                      hoverColor: colors.textPrimary,
                    ),
                  ],
                ),
              ),
            ),
          if (showSelectControl)
            Positioned(
              right: 6,
              top: 6,
              child: AnimatedOpacity(
                duration: _hoverFadeDuration,
                opacity: showSelectControlVisible ? 1 : 0,
                child: _DesktopCoverSelectionCheckbox(
                  isChecked: widget.isSelected,
                  onTap: widget.onSelectTap,
                ),
              ),
            ),
        ],
      ),
    );

    final Widget shapedContent = ClipRRect(
      borderRadius: BorderRadius.circular(8),
      child: clippedContent,
    );

    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: shapedContent,
    );
  }
}

class _DesktopCoverSelectionCheckbox extends StatelessWidget {
  final bool isChecked;
  final VoidCallback? onTap;

  const _DesktopCoverSelectionCheckbox({
    required this.isChecked,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return GestureDetector(
      onTap: onTap,
      behavior: HitTestBehavior.opaque,
      child: SizedBox(
        width: 20,
        height: 20,
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: isChecked ? colors.accent : Colors.transparent,
            borderRadius: BorderRadius.circular(4),
            border: Border.all(
              color: isChecked ? colors.accent : colors.textSecondary,
              width: 0.75,
            ),
          ),
          child: isChecked
              ? Icon(
                  Icons.check,
                  size: 14,
                  color: colors.bgPrimary,
                )
              : null,
        ),
      ),
    );
  }
}

class _DesktopCoverOverlayIconButton extends StatefulWidget {
  final IconData icon;
  final VoidCallback? onTap;
  final Color baseColor;
  final Color hoverColor;

  const _DesktopCoverOverlayIconButton({
    required this.icon,
    required this.onTap,
    required this.baseColor,
    required this.hoverColor,
  });

  @override
  State<_DesktopCoverOverlayIconButton> createState() => _DesktopCoverOverlayIconButtonState();
}

class _DesktopCoverOverlayIconButtonState extends State<_DesktopCoverOverlayIconButton> {
  bool _isHovered = false;

  bool _isPrimaryActivationPointer(PointerDownEvent event) {
    if (event.kind == PointerDeviceKind.mouse) {
      return (event.buttons & kPrimaryMouseButton) != 0;
    }
    return true;
  }

  @override
  Widget build(BuildContext context) {
    final Color iconColor = _isHovered ? widget.hoverColor : widget.baseColor;

    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: AnimatedScale(
        scale: _isHovered && widget.onTap != null ? 1.08 : 1,
        duration: const Duration(milliseconds: 140),
        curve: Curves.easeOutCubic,
        child: Listener(
          onPointerDown: widget.onTap == null
              ? null
              : (PointerDownEvent event) {
                  if (_isPrimaryActivationPointer(event)) {
                    widget.onTap!.call();
                  }
                },
          child: SizedBox(
            width: 30,
            height: 30,
            child: Center(
              child: Icon(
                widget.icon,
                size: 18,
                color: iconColor,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

// Typed desktop row skeleton builders
Widget _buildDesktopMediaRowAlbumSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildDesktopMediaRowPlaylistSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildDesktopMediaRowArtistSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    isCircularCover: true,
  );
}

Widget _buildDesktopMediaRowTrackSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildDesktopMediaRowUnknownSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildDesktopMediaRowSkeletonBase({
  required double itemWidth,
  required double coverSize,
  bool isCircularCover = false,
}) {
  return Builder(
    builder: (BuildContext context) {
      final colors = context.mashinColors;
      final double safeItemWidth = itemWidth > 0 ? itemWidth : 1;

      double safeClamp({required double preferred, required double minWidth}) {
        final double lower = math.min(minWidth, safeItemWidth);
        final double upper = math.max(lower, safeItemWidth);
        return preferred.clamp(lower, upper).toDouble();
      }

      final double titleWidth = safeClamp(preferred: math.min(safeItemWidth - 8, 118), minWidth: 52);
      final double secondaryWidth = safeClamp(preferred: math.min(safeItemWidth - 16, 92), minWidth: 44);
      final double tertiaryWidth = safeClamp(preferred: math.min(safeItemWidth - 24, 74), minWidth: 40);

      Widget line(double width) {
        return Container(
          width: width,
          height: 12,
          decoration: BoxDecoration(
            color: colors.bgTertiary,
            borderRadius: BorderRadius.circular(6),
          ),
        );
      }

      final Widget cover = Container(
        width: coverSize,
        height: coverSize,
        color: colors.bgTertiary,
      );

      return SizedBox(
        width: itemWidth,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Stack(
              children: <Widget>[
                if (isCircularCover)
                  ClipOval(child: cover)
                else
                  ClipRRect(
                    borderRadius: BorderRadius.circular(8),
                    child: cover,
                  ),
                Positioned(
                  left: 6,
                  top: 6,
                  child: Container(
                    width: 18,
                    height: 18,
                    decoration: BoxDecoration(
                      color: colors.bgSecondary,
                      borderRadius: BorderRadius.circular(9),
                    ),
                  ),
                ),
                Positioned(
                  left: 6,
                  bottom: 6,
                  child: Container(
                    width: 40,
                    height: 40,
                    decoration: BoxDecoration(
                      color: colors.bgSecondary,
                      borderRadius: BorderRadius.circular(20),
                      border: Border.all(color: colors.border),
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 12),
            line(titleWidth),
            const SizedBox(height: 6),
            line(secondaryWidth),
            const SizedBox(height: 6),
            line(tertiaryWidth),
          ],
        ),
      );
    },
  );
}

// Helper
String _stripOrEmpty(String? value) {
  return value?.trim() ?? '';
}

String _prefer(String primary, String fallback) {
  final a = primary.trim();
  if (a.isNotEmpty) {
    return a;
  }
  return fallback.trim();
}

String _albumSubtitle(Album album) {
  final artist = firstArtistName(album.artists);
  if (artist != '-') {
    return artist;
  }
  return _mediaTypeLabel(album.albumType);
}

String _trackSubtitle(Track track) {
  final artist = firstArtistName(track.artists);
  if (artist != '-') {
    return artist;
  }
  return _prefer(track.album?.name ?? '', 'Track');
}

String _trackTertiary(Track track) {
  final albumName = _stripOrEmpty(track.album?.name);
  if (albumName.isNotEmpty) {
    return albumName;
  }
  return formatDuration(track.duration);
}

String _albumTertiary(Album album) {
  if (album.year != null && album.year! > 0) {
    return '${album.year}';
  }
  return 'Album';
}

String _playlistSubtitle(Playlist playlist) {
  final owner = _stripOrEmpty(playlist.owner);
  if (owner.isNotEmpty) {
    return owner;
  }
  return 'Playlist';
}

String _playlistTertiary(Playlist playlist) {
  return playlist.isEditable ? 'Editierbar' : 'Readonly';
}

String _artistTertiary(Artist artist) {
  final provider = _stripOrEmpty(artist.provider);
  if (provider.isEmpty) {
    return '-';
  }

  final cut = provider.split('--').first.trim();
  return cut.isEmpty ? provider : cut;
}

String _mediaTypeLabel(String? rawType) {
  final value = rawType?.trim().toLowerCase() ?? '';
  if (value.isEmpty) {
    return '-';
  }

  switch (value) {
    case 'album':
      return 'Album';
    case 'single':
      return 'Single';
    case 'compilation':
      return 'Compilation';
    case 'ep':
      return 'EP';
    default:
      return rawType ?? '-';
  }
}

String firstArtistName(List<Artist> artists) {
  if (artists.isEmpty) {
    return '-';
  }

  return artists.first.name;
}

String formatDuration(int seconds) {
  if (seconds <= 0) {
    return '0:00';
  }

  final duration = Duration(seconds: seconds);
  final minutes = duration.inMinutes;
  final remainingSeconds = duration.inSeconds % 60;

  return '$minutes:${remainingSeconds.toString().padLeft(2, '0')}';
}

String _providerLabelFromId(String providerOrDomain) {
  final String trimmed = providerOrDomain.trim();
  if (trimmed.isEmpty) {
    return '-';
  }

  final String domain = trimmed.split('--').first.trim();
  if (domain.isEmpty) {
    return trimmed;
  }

  return domain[0].toUpperCase() + domain.substring(1);
}
