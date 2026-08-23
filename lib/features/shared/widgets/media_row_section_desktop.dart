import 'package:flutter/material.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'dart:math' as math;
import 'dart:async';

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../../playback/playback_coordinator.dart';
import '../selection/selection_controller.dart';
import 'media_link_text.dart';
import 'media_table_section_desktop.dart';

class MediaRowSectionDesktop<T> extends StatefulWidget {
  static const double _defaultHeight = 226;
  static const double _reservedTextHeight = 80;
  static const double _minItemWidth = 150;

  final String title;
  final bool isLoading;
  final List<T> items;
  final String emptyText;
  final double minItemWith;
  final double itemSpacing;
  final int skeletonCount;
  final PlaybackCoordinator? playbackCoordinator;
  final bool enableSelection;
  final DesktopRowAction<T>? onRowMoreTap;
  final DesktopRowMoreAnchorAction<T>? onRowMoreAnchorTap;
  final DesktopRowContextMenuAction<T>? onRowContextMenu;
  final DesktopRowAction<T>? onRowFavoriteTap;
  final DesktopRowFavoriteResolver<T>? isRowFavorite;
  final ValueChanged<List<T>>? onSelectionChanged;
  final VoidCallback? onSelectionMenuTap;
  final ValueChanged<Artist>? onArtistTap;
  final ValueChanged<Album>? onAlbumTap;

  const MediaRowSectionDesktop({
    super.key,
    required this.title,
    required this.isLoading,
    required this.items,
    required this.emptyText,
    this.minItemWith = _minItemWidth,
    this.itemSpacing = 8,
    this.skeletonCount = 8,
    this.playbackCoordinator,
    this.enableSelection = true,
    this.onRowMoreTap,
    this.onRowMoreAnchorTap,
    this.onRowContextMenu,
    this.onRowFavoriteTap,
    this.isRowFavorite,
    this.onSelectionChanged,
    this.onSelectionMenuTap,
    this.onArtistTap,
    this.onAlbumTap,
  });

  @override
  State<MediaRowSectionDesktop<T>> createState() =>
      _MediaRowSectionDesktopState<T>();
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
  bool _isExpanded = false;
  int _expandedItemCount = 0;
  int _expandLoadRunId = 0;
  int _expandedBaseCount = 0;

  final FocusNode _sectionFocusNode = FocusNode(
    debugLabel: 'MediaRowDesktopSectionFocus',
  );
  final Set<int> _selectedIndexes = <int>{};
  final Set<int> _suppressedSelectionPointers = <int>{};
  final Object _selectionSessionOwner = Object();
  final SelectionController _selectionSessionController =
      SelectionController.instance;
  int? _selectionAnchorIndex;

  @override
  void didUpdateWidget(covariant MediaRowSectionDesktop<T> oldWidget) {
    super.didUpdateWidget(oldWidget);

    _selectedIndexes.removeWhere(
      (int index) => index < 0 || index >= widget.items.length,
    );
    if (_selectionAnchorIndex != null &&
        (_selectionAnchorIndex! < 0 ||
            _selectionAnchorIndex! >= widget.items.length)) {
      _selectionAnchorIndex = null;
    }

    if (_selectedIndexes.isEmpty) {
      _deactivateSelectionSession();
    } else {
      _syncSelectionSession();
    }

    if (_isExpanded) {
      _expandedItemCount = _expandedItemCount.clamp(0, widget.items.length);
    }

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

  @override
  void dispose() {
    _sectionFocusNode.dispose();
    _deactivateSelectionSession();
    super.dispose();
  }

  // Selection and Paging
  bool _isItemFavorite(T item, int index) {
    return widget.isRowFavorite?.call(item, index) ?? false;
  }

  ValueChanged<BuildContext>? _moreAnchorHandler(T item, int index) {
    final DesktopRowMoreAnchorAction<T>? handler = widget.onRowMoreAnchorTap;
    return handler == null
        ? null
        : (BuildContext anchorContext) {
            _ensureItemSelectedForMenu(index);
            handler(item, index, anchorContext);
          };
  }

  /// Selects only the item whose context menu opens so the user can see which
  /// entry the menu belongs to. Any existing selection is replaced.
  void _ensureItemSelectedForMenu(int index) {
    if (!widget.enableSelection ||
        index < 0 ||
        index >= widget.items.length ||
        (_selectedIndexes.length == 1 && _selectedIndexes.contains(index))) {
      return;
    }

    _requestSectionFocus();
    _selectSingleIndex(index);
  }

  /// Right-click variant: keeps the current selection untouched when the
  /// clicked item is already part of it; otherwise selects only that item.
  void _selectItemForRightClickMenu(int index) {
    if (!widget.enableSelection ||
        index < 0 ||
        index >= widget.items.length ||
        _selectedIndexes.contains(index)) {
      return;
    }

    _requestSectionFocus();
    _selectSingleIndex(index);
  }

  bool _isItemSelected(int index) {
    return widget.enableSelection && _selectedIndexes.contains(index);
  }

  bool get _isAllSelected =>
      widget.items.isNotEmpty && _selectedIndexes.length == widget.items.length;

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

  bool get _isTrackSection => T == Track;

  List<MediaItem> _playableItemsFromIndex(int startIndex) {
    if (widget.items.isEmpty ||
        startIndex < 0 ||
        startIndex >= widget.items.length) {
      return const <MediaItem>[];
    }

    final List<MediaItem> playItems = <MediaItem>[];
    for (int index = startIndex; index < widget.items.length; index++) {
      final MediaItem? resolved = _resolvePlayableMediaItem(
        widget.items[index],
      );
      if (resolved == null || !_isValidPlayable(resolved)) {
        continue;
      }
      playItems.add(resolved);
    }

    return playItems;
  }

  void _emitSelectionChanged() {
    if (widget.onSelectionChanged == null) {
      return;
    }
    final List<T> selected = _selectedIndexes
        .where((int index) => index >= 0 && index < widget.items.length)
        .map((int index) => widget.items[index])
        .toList(growable: false);
    widget.onSelectionChanged!(selected);
  }

  void _syncSelectionSession() {
    if (_selectedIndexes.isEmpty) {
      _deactivateSelectionSession();
      return;
    }

    _selectionSessionController.activate(
      owner: _selectionSessionOwner,
      registration: SelectionSessionRegistration(
        allSelected: _isAllSelected,
        onToggleAll: _toggleAllSelectedIndexes,
        onMenuTap: widget.onSelectionMenuTap,
        onClose: _clearSelectionFromController,
      ),
    );
  }

  void _deactivateSelectionSession() {
    _selectionSessionController.deactivate(_selectionSessionOwner);
  }

  void _clearSelectionFromController() {
    if (!mounted) {
      return;
    }
    setState(() {
      _selectedIndexes.clear();
      _selectionAnchorIndex = null;
    });
    _deactivateSelectionSession();
    _emitSelectionChanged();
  }

  void _requestSectionFocus() {
    if (!_sectionFocusNode.hasFocus) {
      _sectionFocusNode.requestFocus();
    }
  }

  void _toggleSelectedIndex(int index) {
    if (!widget.enableSelection) {
      return;
    }
    setState(() {
      if (_selectedIndexes.contains(index)) {
        _selectedIndexes.remove(index);
      } else {
        _selectedIndexes.add(index);
      }
      _selectionAnchorIndex = index;
    });
    _syncSelectionSession();
    _emitSelectionChanged();
  }

  void _selectSingleIndex(int index) {
    if (!widget.enableSelection || index < 0 || index >= widget.items.length) {
      return;
    }

    setState(() {
      _selectedIndexes
        ..clear()
        ..add(index);
      _selectionAnchorIndex = index;
    });
    _syncSelectionSession();
    _emitSelectionChanged();
  }

  void _selectRangeTo(int index) {
    if (!widget.enableSelection || index < 0 || index >= widget.items.length) {
      return;
    }

    final int anchor = _selectionAnchorIndex ?? index;
    final int start = anchor < index ? anchor : index;
    final int end = anchor > index ? anchor : index;

    setState(() {
      _selectedIndexes
        ..clear()
        ..addAll(
          List<int>.generate(end - start + 1, (int offset) => start + offset),
        );
    });
    _syncSelectionSession();
    _emitSelectionChanged();
  }

  void _toggleAllSelectedIndexes() {
    if (!widget.enableSelection || widget.items.isEmpty) {
      return;
    }
    setState(() {
      if (_isAllSelected) {
        _selectedIndexes.clear();
      } else {
        _selectedIndexes
          ..clear()
          ..addAll(
            List<int>.generate(widget.items.length, (int index) => index),
          );
      }
    });
    _syncSelectionSession();
    _emitSelectionChanged();
  }

  void _handleItemPointerDown(int index, PointerDownEvent event) {
    if (event.kind == PointerDeviceKind.mouse &&
        (event.buttons & kSecondaryMouseButton) != 0) {
      _suppressedSelectionPointers.remove(event.pointer);
      final DesktopRowContextMenuAction<T>? handler = widget.onRowContextMenu;
      if (handler != null && index >= 0 && index < widget.items.length) {
        _selectItemForRightClickMenu(index);
        handler(widget.items[index], index, event.position);
      }
      return;
    }

    if (_suppressedSelectionPointers.remove(event.pointer)) {
      return;
    }

    if (!widget.enableSelection ||
        event.kind != PointerDeviceKind.mouse ||
        (event.buttons & kPrimaryMouseButton) == 0) {
      return;
    }

    final Set<LogicalKeyboardKey> keys =
        HardwareKeyboard.instance.logicalKeysPressed;
    final bool isCtrlPressed =
        keys.contains(LogicalKeyboardKey.controlLeft) ||
        keys.contains(LogicalKeyboardKey.controlRight);
    final bool isShiftPressed =
        keys.contains(LogicalKeyboardKey.shiftLeft) ||
        keys.contains(LogicalKeyboardKey.shiftRight);

    _requestSectionFocus();

    if (isShiftPressed) {
      _selectRangeTo(index);
      return;
    }

    if (isCtrlPressed) {
      _toggleSelectedIndex(index);
      return;
    }

    _selectSingleIndex(index);
  }

  void _suppressSelectionForPointer(int pointer) {
    _suppressedSelectionPointers.add(pointer);
  }

  _RowLayoutMetrics _computeLayoutMetrics(double maxWidth) {
    // While loading, items is still empty, so fall back to skeletonCount to keep skeleton items visibly sized.
    final int itemCount = widget.isLoading
        ? widget.skeletonCount
        : widget.items.length;

    if (itemCount <= 0) {
      return const _RowLayoutMetrics(visibleCount: 1, computedItemWidth: 0);
    }

    final double minItemWidth = widget.minItemWith > 0
        ? widget.minItemWith
        : MediaRowSectionDesktop._minItemWidth;
    final double spacing = widget.itemSpacing;

    if (!maxWidth.isFinite || maxWidth <= 0) {
      final int fallbackVisible = math.max(1, itemCount);
      return _RowLayoutMetrics(
        visibleCount: fallbackVisible,
        computedItemWidth: minItemWidth,
      );
    }

    final double denominator = minItemWidth + spacing;
    int fitByMinSize = denominator > 0
        ? ((maxWidth + spacing) / denominator).floor()
        : 1;
    fitByMinSize = math.max(1, fitByMinSize);

    final int visibleCount = math.min(itemCount, fitByMinSize);
    final double computedWidth =
        (maxWidth - ((visibleCount - 1) * spacing)) / visibleCount;

    return _RowLayoutMetrics(
      visibleCount: visibleCount,
      computedItemWidth: math.max(minItemWidth, computedWidth),
    );
  }

  double _resolveCoverSize(double resolvedItemWidth) {
    return math.max(1, resolvedItemWidth);
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

  // Playback actions
  void _onPlayTap(int absoluteIndex) {
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
    if (coordinator == null ||
        absoluteIndex < 0 ||
        absoluteIndex >= widget.items.length) {
      return;
    }

    final MediaItem? item = _resolvePlayableMediaItem(
      widget.items[absoluteIndex],
    );
    if (item == null || !_isValidPlayable(item)) {
      return;
    }

    unawaited(() async {
      try {
        await coordinator.playMedia(<MediaItem>[item]);
      } catch (_) {
        // Ignore failures to keep row interactions responsive.
      }
    }());
  }

  void _onPlayAllTap() {
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
    if (coordinator == null) {
      return;
    }

    final List<MediaItem> playItems = _playableItemsFromIndex(0);
    if (playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await coordinator.playMedia(
          _isTrackSection ? playItems : <MediaItem>[playItems.first],
        );
      } catch (_) {
        // Ignore failures to keep header interactions responsive.
      }
    }());
  }

  void _onShuffleAllTap() {
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
    if (coordinator == null) {
      return;
    }

    final List<MediaItem> playItems = _playableItemsFromIndex(0);
    if (playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        if (_isTrackSection) {
          await coordinator.shufflePlayMedia(playItems);
          return;
        }

        final MediaItem item =
            playItems[math.Random().nextInt(playItems.length)];
        await coordinator.playMedia(<MediaItem>[item]);
      } catch (_) {
        // Ignore failures to keep header interactions responsive.
      }
    }());
  }

  // Build
  Widget _buildSlideList({
    required int startIndex,
    required int visibleCount,
    required double resolvedItemWidth,
    required double resolvedCoverSize,
  }) {
    final List<T> visibleItems = _sliceItems(
      startIndex: startIndex,
      visibleCount: visibleCount,
    );

    return _buildItemListView(
      items: visibleItems,
      startIndex: startIndex,
      resolvedItemWidth: resolvedItemWidth,
      resolvedCoverSize: resolvedCoverSize,
      key: ValueKey<int>(startIndex),
    );
  }

  Widget _buildItemListView({
    required List<T> items,
    required int startIndex,
    required double resolvedItemWidth,
    required double resolvedCoverSize,
    Key? key,
  }) {
    return ListView.separated(
      key: key,
      scrollDirection: Axis.horizontal,
      physics: const NeverScrollableScrollPhysics(),
      itemCount: items.length,
      separatorBuilder: (_, _) => SizedBox(width: widget.itemSpacing),
      itemBuilder: (BuildContext context, int index) {
        final int absoluteIndex = startIndex + index;
        return _buildRowItem(
          item: items[index],
          absoluteIndex: absoluteIndex,
          itemWidth: resolvedItemWidth,
          coverSize: resolvedCoverSize,
        );
      },
    );
  }

  Widget _buildRowItem({
    required T item,
    required int absoluteIndex,
    required double itemWidth,
    required double coverSize,
  }) {
    final bool isSelected = _isItemSelected(absoluteIndex);
    void onSelectTap() => _toggleSelectedIndex(absoluteIndex);
    void onSelectPointerDown(PointerDownEvent event) =>
        _handleItemPointerDown(absoluteIndex, event);

    if (item is Album) {
      return _buildDesktopMediaRowAlbum(
        item: item,
        index: absoluteIndex,
        itemWidth: itemWidth,
        coverSize: coverSize,
        onPlayTap: () => _onPlayTap(absoluteIndex),
        isFavorite: _isItemFavorite(item as T, absoluteIndex),
        onFavoriteTap: widget.onRowFavoriteTap == null
            ? null
            : () => widget.onRowFavoriteTap!(item as T, absoluteIndex),
        onMoreTap: widget.onRowMoreTap == null
            ? null
            : () => widget.onRowMoreTap!(item as T, absoluteIndex),
        onMoreAnchorTap: _moreAnchorHandler(item as T, absoluteIndex),
        isSelected: isSelected,
        onSelectTap: widget.enableSelection ? onSelectTap : null,
        onSelectPointerDown: widget.enableSelection
            ? onSelectPointerDown
            : null,
        onArtistTap: widget.onArtistTap,
        onArtistPointerDown: _suppressSelectionForPointer,
        onAlbumTap: widget.onAlbumTap,
      );
    }
    if (item is Playlist) {
      return _buildDesktopMediaRowPlaylist(
        item: item,
        index: absoluteIndex,
        itemWidth: itemWidth,
        coverSize: coverSize,
        onPlayTap: () => _onPlayTap(absoluteIndex),
        isFavorite: _isItemFavorite(item as T, absoluteIndex),
        onFavoriteTap: widget.onRowFavoriteTap == null
            ? null
            : () => widget.onRowFavoriteTap!(item as T, absoluteIndex),
        onMoreTap: widget.onRowMoreTap == null
            ? null
            : () => widget.onRowMoreTap!(item as T, absoluteIndex),
        onMoreAnchorTap: _moreAnchorHandler(item as T, absoluteIndex),
        isSelected: isSelected,
        onSelectTap: widget.enableSelection ? onSelectTap : null,
        onSelectPointerDown: widget.enableSelection
            ? onSelectPointerDown
            : null,
      );
    }
    if (item is Artist) {
      return _buildDesktopMediaRowArtist(
        item: item,
        index: absoluteIndex,
        itemWidth: itemWidth,
        coverSize: coverSize,
        onPlayTap: () => _onPlayTap(absoluteIndex),
        isFavorite: _isItemFavorite(item as T, absoluteIndex),
        onFavoriteTap: widget.onRowFavoriteTap == null
            ? null
            : () => widget.onRowFavoriteTap!(item as T, absoluteIndex),
        onMoreTap: widget.onRowMoreTap == null
            ? null
            : () => widget.onRowMoreTap!(item as T, absoluteIndex),
        onMoreAnchorTap: _moreAnchorHandler(item as T, absoluteIndex),
        isSelected: isSelected,
        onSelectTap: widget.enableSelection ? onSelectTap : null,
        onSelectPointerDown: widget.enableSelection
            ? onSelectPointerDown
            : null,
        onArtistTap: widget.onArtistTap,
      );
    }
    if (item is Track) {
      return _buildDesktopMediaRowTrack(
        item: item,
        index: absoluteIndex,
        itemWidth: itemWidth,
        coverSize: coverSize,
        onPlayTap: () => _onPlayTap(absoluteIndex),
        isFavorite: _isItemFavorite(item as T, absoluteIndex),
        onFavoriteTap: widget.onRowFavoriteTap == null
            ? null
            : () => widget.onRowFavoriteTap!(item as T, absoluteIndex),
        onMoreTap: widget.onRowMoreTap == null
            ? null
            : () => widget.onRowMoreTap!(item as T, absoluteIndex),
        onMoreAnchorTap: _moreAnchorHandler(item as T, absoluteIndex),
        isSelected: isSelected,
        onSelectTap: widget.enableSelection ? onSelectTap : null,
        onSelectPointerDown: widget.enableSelection
            ? onSelectPointerDown
            : null,
        onArtistTap: widget.onArtistTap,
        onArtistPointerDown: _suppressSelectionForPointer,
        onAlbumTap: widget.onAlbumTap,
      );
    }

    return _buildDesktopMediaRowUnknown(
      index: absoluteIndex,
      itemWidth: itemWidth,
      coverSize: coverSize,
      onPlayTap: () => _onPlayTap(absoluteIndex),
      isFavorite: _isItemFavorite(item, absoluteIndex),
      onFavoriteTap: widget.onRowFavoriteTap == null
          ? null
          : () => widget.onRowFavoriteTap!(item, absoluteIndex),
      onMoreTap: widget.onRowMoreTap == null
          ? null
          : () => widget.onRowMoreTap!(item, absoluteIndex),
      onMoreAnchorTap: _moreAnchorHandler(item, absoluteIndex),
      isSelected: isSelected,
      onSelectTap: widget.enableSelection ? onSelectTap : null,
      onSelectPointerDown: widget.enableSelection ? onSelectPointerDown : null,
    );
  }

  Widget _buildExpandedGrid({
    required double resolvedItemWidth,
    required double resolvedCoverSize,
    required double sectionHeight,
    required int itemsPerRow,
  }) {
    final int itemCount = math.min(_expandedItemCount, widget.items.length);
    final List<T> items = widget.items.take(itemCount).toList(growable: false);

    return SizedBox(
      width: double.infinity,
      child: Wrap(
        spacing: widget.itemSpacing,
        runSpacing: widget.itemSpacing,
        children: <Widget>[
          for (int index = 0; index < items.length; index++)
            _buildExpandedRowItem(
              item: items[index],
              absoluteIndex: index,
              itemWidth: resolvedItemWidth,
              coverSize: resolvedCoverSize,
              itemsPerRow: itemsPerRow,
            ),
        ],
      ),
    );
  }

  /// Wraps an item in the expanded view. Items that were already visible on
  /// the current page appear instantly; newly appended rows slide in from
  /// above with a per-row stagger so the grid grows downwards row by row.
  Widget _buildExpandedRowItem({
    required T item,
    required int absoluteIndex,
    required double itemWidth,
    required double coverSize,
    required int itemsPerRow,
  }) {
    final Widget child = _buildRowItem(
      item: item,
      absoluteIndex: absoluteIndex,
      itemWidth: itemWidth,
      coverSize: coverSize,
    );

    if (absoluteIndex < _expandedBaseCount) {
      return child;
    }

    final int rowIndex =
        (absoluteIndex - _expandedBaseCount) ~/ math.max(1, itemsPerRow);

    return _ExpandRevealItem(
      key: ValueKey<String>('expand-reveal-$absoluteIndex'),
      delay: Duration(milliseconds: 70 * rowIndex),
      child: child,
    );
  }

  bool get _canToggleExpand =>
      !widget.isLoading &&
      widget.items.length >
          math.max(1, _lastVisibleCount ?? _pageStartIndex + 1);

  /// Last computed number of items fitting on one page; updated during build.
  int? _lastVisibleCount;

  void _toggleExpanded() {
    if (!_canToggleExpand) {
      return;
    }

    if (_isExpanded) {
      setState(() {
        _isExpanded = false;
        _expandLoadRunId++;
      });
      return;
    }

    setState(() {
      _isExpanded = true;
      _finishRunningTransitionImmediately();
      // Keep the currently visible page content static; only the appended
      // items animate in from above, row by row.
      _expandedItemCount = math.min(
        math.max(_pageStartIndex + (_lastVisibleCount ?? 1), 1),
        widget.items.length,
      );
      _expandedBaseCount = _expandedItemCount;
    });

    _loadExpandedItemsIncrementally();
  }

  Future<void> _loadExpandedItemsIncrementally() async {
    final int runId = ++_expandLoadRunId;
    final int pageSize = math.max(1, _lastVisibleCount ?? 1);

    while (mounted && _isExpanded && runId == _expandLoadRunId) {
      if (_expandedItemCount >= widget.items.length) {
        return;
      }

      await Future<void>.delayed(const Duration(milliseconds: 220));
      if (!mounted || !_isExpanded || runId != _expandLoadRunId) {
        return;
      }

      setState(() {
        _expandedItemCount = math.min(
          _expandedItemCount + pageSize,
          widget.items.length,
        );
      });
    }
  }

  // Typed desktop row skeleton dispatch
  Widget _buildDesktopTypedRowSkeleton({
    required double itemWidth,
    required double coverSize,
  }) {
    if (T == Album) {
      return _buildDesktopMediaRowAlbumSkeleton(
        itemWidth: itemWidth,
        coverSize: coverSize,
      );
    }
    if (T == Playlist) {
      return _buildDesktopMediaRowPlaylistSkeleton(
        itemWidth: itemWidth,
        coverSize: coverSize,
      );
    }
    if (T == Artist) {
      return _buildDesktopMediaRowArtistSkeleton(
        itemWidth: itemWidth,
        coverSize: coverSize,
      );
    }
    if (T == Track) {
      return _buildDesktopMediaRowTrackSkeleton(
        itemWidth: itemWidth,
        coverSize: coverSize,
      );
    }

    return _buildDesktopMediaRowUnknownSkeleton(
      itemWidth: itemWidth,
      coverSize: coverSize,
    );
  }

  Widget _buildPaginationControls(
    BuildContext context, {
    required int visibleCount,
    required int pageCount,
  }) {
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
          onPressed: canGoNext
              ? () => _goToNextPage(visibleCount, pageCount)
              : null,
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

    final Widget content = LayoutBuilder(
      builder: (BuildContext context, BoxConstraints constraints) {
        final _RowLayoutMetrics metrics = _computeLayoutMetrics(
          constraints.maxWidth,
        );
        final int visibleCount = metrics.visibleCount;
        _lastVisibleCount = visibleCount;
        final double resolvedItemWidth = metrics.computedItemWidth;
        final double resolvedCoverSize = _resolveCoverSize(resolvedItemWidth);
        final double sectionHeight = math.max(
          MediaRowSectionDesktop._defaultHeight,
          resolvedCoverSize + MediaRowSectionDesktop._reservedTextHeight,
        );
        final int pageCount = widget.items.isEmpty
            ? 1
            : (widget.items.length / visibleCount).ceil();
        final bool isPagingEnabled = pageCount > 1;
        _pageStartIndex = _clampStartIndex(visibleCount: visibleCount);
        final bool hasPlayableContent =
            !widget.isLoading && widget.items.isNotEmpty;
        final bool showHeaderRow =
            hasTitle ||
            (hasPlayableContent &&
                (isPagingEnabled || widget.playbackCoordinator != null));
        final bool canToggleExpand = _canToggleExpand;
        final Widget collapsedContent = SizedBox(
          height: sectionHeight,
          child: widget.isLoading
              ? ListView.separated(
                  scrollDirection: Axis.horizontal,
                  itemCount: widget.skeletonCount,
                  separatorBuilder: (_, _) =>
                      SizedBox(width: widget.itemSpacing),
                  itemBuilder: (_, _) => _buildDesktopTypedRowSkeleton(
                    itemWidth: resolvedItemWidth,
                    coverSize: resolvedCoverSize,
                  ),
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
                          builder:
                              (
                                BuildContext context,
                                double value,
                                Widget? child,
                              ) {
                                final double dx =
                                    (_navigationDirection >= 0 ? -1 : 1) *
                                    _chunkTranslatePx *
                                    value;
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
                          builder:
                              (
                                BuildContext context,
                                double value,
                                Widget? child,
                              ) {
                                final double dx =
                                    (_navigationDirection >= 0 ? 1 : -1) *
                                    _chunkTranslatePx *
                                    (1 - value);
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
        );
        final Widget bodyContent = AnimatedSize(
          duration: const Duration(milliseconds: 380),
          curve: Curves.easeInOutCubic,
          alignment: Alignment.topCenter,
          clipBehavior: Clip.hardEdge,
          child: _isExpanded
              ? SizedBox(
                  key: const ValueKey<String>('row-expanded'),
                  width: double.infinity,
                  child: _buildExpandedGrid(
                    resolvedItemWidth: resolvedItemWidth,
                    resolvedCoverSize: resolvedCoverSize,
                    sectionHeight: sectionHeight,
                    itemsPerRow: visibleCount,
                  ),
                )
              : SizedBox(
                  key: const ValueKey<String>('row-collapsed'),
                  width: double.infinity,
                  child: collapsedContent,
                ),
        );

        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            if (showHeaderRow)
              Row(
                children: <Widget>[
                  if (hasTitle)
                    Text(
                      widget.title,
                      style: Theme.of(context).textTheme.titleMedium?.copyWith(
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                  if (hasTitle && hasPlayableContent) ...<Widget>[
                    const SizedBox(width: 10),
                    SectionCountBadge(count: widget.items.length),
                  ],
                  const SizedBox(width: 16),
                  const Expanded(child: SectionHeaderDivider()),
                  const SizedBox(width: 16),
                  if (hasPlayableContent &&
                      widget.playbackCoordinator != null) ...<Widget>[
                    SectionHeaderIconButton(
                      icon: Icons.play_arrow,
                      tooltip: 'Abspielen',
                      onTap: _onPlayAllTap,
                    ),
                    const SizedBox(width: 8),
                    SectionHeaderIconButton(
                      icon: Icons.shuffle,
                      tooltip: 'Zufällig',
                      onTap: _onShuffleAllTap,
                    ),
                  ],
                  if (isPagingEnabled &&
                      hasPlayableContent &&
                      !_isExpanded) ...<Widget>[
                    const SizedBox(width: 4),
                    const SectionHeaderVerticalDivider(),
                    _buildPaginationControls(
                      context,
                      visibleCount: visibleCount,
                      pageCount: pageCount,
                    ),
                  ],
                ],
              ),
            if (showHeaderRow) const SizedBox(height: 10),
            bodyContent,
            if (canToggleExpand)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Center(
                  child: _ExpandToggleButton(
                    isExpanded: _isExpanded,
                    onTap: _toggleExpanded,
                  ),
                ),
              ),
          ],
        );
      },
    );

    final Widget focusableContent = Focus(
      focusNode: _sectionFocusNode,
      child: content,
    );

    if (!widget.enableSelection) {
      return focusableContent;
    }

    return Shortcuts(
      shortcuts: const <ShortcutActivator, Intent>{
        SingleActivator(LogicalKeyboardKey.keyA, control: true):
            _SelectAllIntent(),
        SingleActivator(LogicalKeyboardKey.escape): _ClearSelectionIntent(),
      },
      child: Actions(
        actions: <Type, Action<Intent>>{
          _SelectAllIntent: CallbackAction<_SelectAllIntent>(
            onInvoke: (_SelectAllIntent intent) {
              _toggleAllSelectedIndexes();
              return null;
            },
          ),
          _ClearSelectionIntent: CallbackAction<_ClearSelectionIntent>(
            onInvoke: (_ClearSelectionIntent intent) {
              _clearSelectionFromController();
              return null;
            },
          ),
        },
        child: focusableContent,
      ),
    );
  }
}

class _SelectAllIntent extends Intent {
  const _SelectAllIntent();
}

class _ClearSelectionIntent extends Intent {
  const _ClearSelectionIntent();
}

// Small circular toggle shown below a section to expand/collapse all items,
// matching the MAUI TableView/RowView expand toggle styling.
class _ExpandToggleButton extends StatefulWidget {
  final bool isExpanded;
  final VoidCallback onTap;

  const _ExpandToggleButton({required this.isExpanded, required this.onTap});

  @override
  State<_ExpandToggleButton> createState() => _ExpandToggleButtonState();
}

class _ExpandToggleButtonState extends State<_ExpandToggleButton> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Tooltip(
      message: widget.isExpanded ? 'Weniger anzeigen' : 'Alle anzeigen',
      child: MouseRegion(
        onEnter: (_) => setState(() => _isHovered = true),
        onExit: (_) => setState(() => _isHovered = false),
        child: Material(
          color: _isHovered ? colors.hover : Colors.transparent,
          borderRadius: BorderRadius.circular(12),
          child: InkWell(
            onTap: widget.onTap,
            borderRadius: BorderRadius.circular(12),
            hoverColor: colors.hover,
            child: SizedBox(
              width: 24,
              height: 24,
              child: AnimatedScale(
                scale: _isHovered ? 1.08 : 1,
                duration: const Duration(milliseconds: 120),
                curve: Curves.easeOutCubic,
                child: AnimatedRotation(
                  turns: widget.isExpanded ? 0.5 : 0,
                  duration: const Duration(milliseconds: 220),
                  curve: Curves.easeOutCubic,
                  child: Icon(
                    Icons.expand_more_outlined,
                    size: 14,
                    color: colors.textSecondary,
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Reveals a newly appended expanded-grid item by fading and sliding it in
/// from above, matching the MAUI RowView page-extension animation
/// (TranslateY -50 + FadeIn). Runs once on insert; the optional [delay]
/// staggers items row by row.
class _ExpandRevealItem extends StatefulWidget {
  final Widget child;
  final Duration delay;

  const _ExpandRevealItem({
    super.key,
    required this.child,
    this.delay = Duration.zero,
  });

  @override
  State<_ExpandRevealItem> createState() => _ExpandRevealItemState();
}

class _ExpandRevealItemState extends State<_ExpandRevealItem> {
  bool _revealed = false;
  Timer? _delayTimer;

  @override
  void initState() {
    super.initState();
    if (widget.delay == Duration.zero) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _startReveal());
    } else {
      _delayTimer = Timer(widget.delay, _startReveal);
    }
  }

  void _startReveal() {
    if (!mounted || _revealed) {
      return;
    }
    setState(() {
      _revealed = true;
    });
  }

  @override
  void dispose() {
    _delayTimer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return TweenAnimationBuilder<double>(
      tween: Tween<double>(begin: 0, end: _revealed ? 1 : 0),
      duration: const Duration(milliseconds: 400),
      curve: Curves.easeInOutCubic,
      builder: (BuildContext context, double value, Widget? child) {
        return Opacity(
          opacity: value,
          child: Transform.translate(
            offset: Offset(0, -50 * (1 - value)),
            child: child,
          ),
        );
      },
      child: widget.child,
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

// Inset between the item's cover/text content and the frame edge, revealing the hover/selection background.
const double _kRowItemInset = 6;

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
  ValueChanged<BuildContext>? onMoreAnchorTap,
  required bool isSelected,
  required VoidCallback? onSelectTap,
  required ValueChanged<PointerDownEvent>? onSelectPointerDown,
  ValueChanged<Artist>? onArtistTap,
  ValueChanged<int>? onArtistPointerDown,
  ValueChanged<Album>? onAlbumTap,
}) {
  return _buildDesktopMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.album_outlined,
    title: _prefer(item.name, 'Unknown Album'),
    secondaryText: _albumSubtitle(item),
    secondaryArtists: item.artists,
    onArtistTap: onArtistTap,
    onArtistPointerDown: onArtistPointerDown,
    titleLinkItem: item,
    onTitleTap: onAlbumTap == null
        ? null
        : (MediaItem media) => onAlbumTap(media as Album),
    tertiaryText: _albumTertiary(item),
    coverUrl: item.imageUri ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
    onPlayTap: onPlayTap,
    isFavorite: isFavorite,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    onMoreAnchorTap: onMoreAnchorTap,
    isSelected: isSelected,
    onSelectTap: onSelectTap,
    onSelectPointerDown: onSelectPointerDown,
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
  ValueChanged<BuildContext>? onMoreAnchorTap,
  required bool isSelected,
  required VoidCallback? onSelectTap,
  required ValueChanged<PointerDownEvent>? onSelectPointerDown,
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
    onMoreAnchorTap: onMoreAnchorTap,
    isSelected: isSelected,
    onSelectTap: onSelectTap,
    onSelectPointerDown: onSelectPointerDown,
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
  ValueChanged<BuildContext>? onMoreAnchorTap,
  required bool isSelected,
  required VoidCallback? onSelectTap,
  required ValueChanged<PointerDownEvent>? onSelectPointerDown,
  ValueChanged<Artist>? onArtistTap,
}) {
  return _buildDesktopMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.person_outlined,
    title: _prefer(item.name, 'Unknown Artist'),
    titleLinkItem: item,
    onTitleTap: onArtistTap == null
        ? null
        : (MediaItem media) => onArtistTap(media as Artist),
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
    onMoreAnchorTap: onMoreAnchorTap,
    isSelected: isSelected,
    onSelectTap: onSelectTap,
    onSelectPointerDown: onSelectPointerDown,
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
  ValueChanged<BuildContext>? onMoreAnchorTap,
  required bool isSelected,
  required VoidCallback? onSelectTap,
  required ValueChanged<PointerDownEvent>? onSelectPointerDown,
  ValueChanged<Artist>? onArtistTap,
  ValueChanged<int>? onArtistPointerDown,
  ValueChanged<Album>? onAlbumTap,
}) {
  return _buildDesktopMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.music_note_outlined,
    title: _prefer(item.name, 'Unknown Track'),
    secondaryText: _trackSubtitle(item),
    secondaryArtists: item.artists,
    onArtistTap: onArtistTap,
    onArtistPointerDown: onArtistPointerDown,
    tertiaryText: _trackTertiary(item),
    tertiaryAlbum: item.album,
    onAlbumTap: onAlbumTap,
    coverUrl: item.imageUri ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
    onPlayTap: onPlayTap,
    isFavorite: isFavorite,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    onMoreAnchorTap: onMoreAnchorTap,
    isSelected: isSelected,
    onSelectTap: onSelectTap,
    onSelectPointerDown: onSelectPointerDown,
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
  ValueChanged<BuildContext>? onMoreAnchorTap,
  required bool isSelected,
  required VoidCallback? onSelectTap,
  required ValueChanged<PointerDownEvent>? onSelectPointerDown,
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
    onMoreAnchorTap: onMoreAnchorTap,
    isSelected: isSelected,
    onSelectTap: onSelectTap,
    onSelectPointerDown: onSelectPointerDown,
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
  ValueChanged<BuildContext>? onMoreAnchorTap,
  required bool isSelected,
  required VoidCallback? onSelectTap,
  required ValueChanged<PointerDownEvent>? onSelectPointerDown,
  List<Artist>? secondaryArtists,
  ValueChanged<Artist>? onArtistTap,
  ValueChanged<int>? onArtistPointerDown,
  Album? tertiaryAlbum,
  ValueChanged<Album>? onAlbumTap,
  MediaItem? titleLinkItem,
  ValueChanged<MediaItem>? onTitleTap,
  bool showPlayOverlay = true,
  bool isCircularCover = false,
}) {
  return Builder(
    builder: (BuildContext context) {
      final colors = context.mashinColors;
      final bool hasArtistLinks =
          secondaryArtists != null && secondaryArtists.isNotEmpty;
      final bool hasSecondary =
          hasArtistLinks ||
          (secondaryText != null && secondaryText.trim().isNotEmpty);
      final bool hasTertiary =
          tertiaryText != null && tertiaryText.trim().isNotEmpty;
      final double insetCoverSize = math.max(
        1,
        coverSize - (_kRowItemInset * 2),
      );

      return SizedBox(
        width: itemWidth,
        child: _DesktopRowItemFrame(
          isSelected: isSelected,
          onSelectPointerDown: onSelectPointerDown,
          coverBuilder: (bool isHovered) => _DesktopMediaCover(
            coverSize: insetCoverSize,
            icon: icon,
            coverUrl: coverUrl,
            providerLabel: providerLabel,
            providerIconSvg: providerIconSvg,
            onPlayTap: onPlayTap,
            isFavorite: isFavorite,
            onFavoriteTap: onFavoriteTap,
            onMoreTap: onMoreTap,
            onMoreAnchorTap: onMoreAnchorTap,
            isSelected: isSelected,
            onSelectTap: onSelectTap,
            onBackgroundPointerDown: onSelectPointerDown,
            isHovered: isHovered,
            showPlayOverlay: showPlayOverlay,
            isCircularCover: isCircularCover,
          ),
          textArea: SizedBox(
            width: double.infinity,
            child: Padding(
              padding: const EdgeInsets.only(top: 12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  titleLinkItem == null
                      ? Text(
                          title,
                          maxLines: 1,
                          textAlign: TextAlign.left,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.titleSmall
                              ?.copyWith(
                                fontWeight: FontWeight.w400,
                                fontSize: 15,
                              ),
                        )
                      : MediaLinkText(
                          items: <MediaItem>[titleLinkItem],
                          fallbackText: title,
                          onItemTap: onTitleTap,
                          style: Theme.of(context).textTheme.titleSmall
                              ?.copyWith(
                                fontWeight: FontWeight.w400,
                                fontSize: 15,
                              ),
                        ),
                  if (hasSecondary) ...<Widget>[
                    const SizedBox(height: 2),
                    hasArtistLinks
                        ? MediaLinkText(
                            items: secondaryArtists,
                            fallbackText: secondaryText ?? '',
                            onItemTap: onArtistTap == null
                                ? null
                                : (MediaItem media) =>
                                      onArtistTap(media as Artist),
                            onItemPointerDown: onArtistPointerDown,
                            style: TextStyle(
                              color: colors.textSecondary,
                              fontSize: 14,
                            ),
                          )
                        : Text(
                            secondaryText ?? '',
                            maxLines: 1,
                            textAlign: TextAlign.left,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                              color: colors.textSecondary,
                              fontSize: 14,
                            ),
                          ),
                  ],
                  if (hasTertiary) ...<Widget>[
                    const SizedBox(height: 2),
                    tertiaryAlbum == null
                        ? Text(
                            tertiaryText,
                            maxLines: 1,
                            textAlign: TextAlign.left,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                              color: colors.textTertiary,
                              fontSize: 14,
                            ),
                          )
                        : MediaLinkText(
                            items: <MediaItem>[tertiaryAlbum],
                            fallbackText: tertiaryText,
                            onItemTap: onAlbumTap == null
                                ? null
                                : (MediaItem media) =>
                                      onAlbumTap(media as Album),
                            style: TextStyle(
                              color: colors.textTertiary,
                              fontSize: 14,
                            ),
                          ),
                  ],
                ],
              ),
            ),
          ),
        ),
      );
    },
  );
}

class _DesktopRowItemFrame extends StatefulWidget {
  final Widget Function(bool isHovered) coverBuilder;
  final Widget textArea;
  final bool isSelected;
  final ValueChanged<PointerDownEvent>? onSelectPointerDown;

  const _DesktopRowItemFrame({
    required this.coverBuilder,
    required this.textArea,
    required this.isSelected,
    required this.onSelectPointerDown,
  });

  @override
  State<_DesktopRowItemFrame> createState() => _DesktopRowItemFrameState();
}

class _DesktopRowItemFrameState extends State<_DesktopRowItemFrame> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    // Matches the hover/selection background of MediaTableSectionDesktop rows.
    Color background = Colors.transparent;
    if (widget.isSelected) {
      background = _isHovered ? colors.hover : colors.selection;
    } else if (_isHovered) {
      background = colors.hover;
    }

    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: Container(
        decoration: BoxDecoration(
          color: background,
          borderRadius: BorderRadius.circular(8),
        ),
        padding: const EdgeInsets.all(_kRowItemInset),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            widget.coverBuilder(_isHovered),
            Listener(
              onPointerDown: widget.onSelectPointerDown,
              behavior: HitTestBehavior.opaque,
              child: widget.textArea,
            ),
          ],
        ),
      ),
    );
  }
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
  final ValueChanged<BuildContext>? onMoreAnchorTap;
  final bool isSelected;
  final VoidCallback? onSelectTap;
  final ValueChanged<PointerDownEvent>? onBackgroundPointerDown;
  final bool isHovered;
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
    required this.onMoreAnchorTap,
    required this.isSelected,
    required this.onSelectTap,
    required this.onBackgroundPointerDown,
    required this.isHovered,
    required this.showPlayOverlay,
    required this.isCircularCover,
  });

  @override
  State<_DesktopMediaCover> createState() => _DesktopMediaCoverState();
}

class _DesktopMediaCoverState extends State<_DesktopMediaCover> {
  static const Duration _hoverFadeDuration = Duration(milliseconds: 140);
  static const Duration _buttonScaleDuration = Duration(milliseconds: 140);
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
    final bool hasCover =
        widget.coverUrl != null && widget.coverUrl!.trim().isNotEmpty;
    final bool showHoverPlay =
        widget.showPlayOverlay && widget.onPlayTap != null;
    const bool showSelectControl = true;
    final bool showSelectControlVisible = widget.isHovered || widget.isSelected;
    const bool showOverlayActions = true;

    final Widget clippedContent = SizedBox(
      width: widget.coverSize,
      height: widget.coverSize,
      child: Stack(
        fit: StackFit.expand,
        children: <Widget>[
          Listener(
            onPointerDown: widget.onBackgroundPointerDown,
            behavior: HitTestBehavior.opaque,
            child: widget.isCircularCover
                ? ClipOval(
                    child: Container(
                      color: colors.bgTertiary,
                      child: hasCover
                          ? Image.network(
                              widget.coverUrl!,
                              fit: BoxFit.cover,
                              errorBuilder: (_, _, _) => Icon(
                                widget.icon,
                                color: colors.accent,
                                size: 34,
                              ),
                            )
                          : Icon(widget.icon, color: colors.accent, size: 34),
                    ),
                  )
                : Container(
                    color: colors.bgTertiary,
                    child: hasCover
                        ? Image.network(
                            widget.coverUrl!,
                            fit: BoxFit.cover,
                            errorBuilder: (_, _, _) => Icon(
                              widget.icon,
                              color: colors.accent,
                              size: 34,
                            ),
                          )
                        : Icon(widget.icon, color: colors.accent, size: 34),
                  ),
          ),
          if (showHoverPlay)
            IgnorePointer(
              child: AnimatedOpacity(
                duration: _hoverFadeDuration,
                opacity: widget.isHovered ? 1 : 0,
                child: widget.isCircularCover
                    ? ClipOval(
                        child: Container(
                          color: Colors.black.withValues(alpha: 0.34),
                        ),
                      )
                    : Container(color: Colors.black.withValues(alpha: 0.34)),
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
                  opacity: widget.isHovered ? 1 : 0,
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
              left: 2,
              right: 2,
              bottom: 2,
              child: AnimatedOpacity(
                duration: _hoverFadeDuration,
                opacity: widget.isHovered ? 1 : 0,
                child: Row(
                  children: <Widget>[
                    const Spacer(),
                    _DesktopCoverOverlayIconButton(
                      icon: widget.isFavorite
                          ? Icons.favorite_outlined
                          : Icons.favorite_border_outlined,
                      onTap: widget.onFavoriteTap,
                      baseColor: colors.textPrimary,
                      hoverColor: colors.textSecondary,
                    ),
                    const SizedBox(width: 4),
                    _DesktopCoverOverlayIconButton(
                      icon: Icons.more_horiz_outlined,
                      onTap: widget.onMoreTap,
                      onAnchorTap: widget.onMoreAnchorTap,
                      baseColor: colors.textPrimary,
                      hoverColor: colors.textSecondary,
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

    return shapedContent;
  }
}

class _DesktopCoverSelectionCheckbox extends StatelessWidget {
  final bool isChecked;
  final VoidCallback? onTap;

  const _DesktopCoverSelectionCheckbox({
    required this.isChecked,
    required this.onTap,
  });

  bool _isPrimaryActivationPointer(PointerDownEvent event) {
    if (event.kind == PointerDeviceKind.mouse) {
      return (event.buttons & kPrimaryMouseButton) != 0;
    }
    return true;
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Listener(
      onPointerDown: onTap == null
          ? null
          : (PointerDownEvent event) {
              if (_isPrimaryActivationPointer(event)) {
                onTap!.call();
              }
            },
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
              ? Icon(Icons.check, size: 14, color: colors.bgPrimary)
              : null,
        ),
      ),
    );
  }
}

class _DesktopCoverOverlayIconButton extends StatefulWidget {
  final IconData icon;
  final VoidCallback? onTap;
  final ValueChanged<BuildContext>? onAnchorTap;
  final Color baseColor;
  final Color hoverColor;

  const _DesktopCoverOverlayIconButton({
    required this.icon,
    required this.onTap,
    this.onAnchorTap,
    required this.baseColor,
    required this.hoverColor,
  });

  @override
  State<_DesktopCoverOverlayIconButton> createState() =>
      _DesktopCoverOverlayIconButtonState();
}

class _DesktopCoverOverlayIconButtonState
    extends State<_DesktopCoverOverlayIconButton> {
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
      child: Listener(
        onPointerDown: widget.onTap == null && widget.onAnchorTap == null
            ? null
            : (PointerDownEvent event) {
                if (_isPrimaryActivationPointer(event)) {
                  final ValueChanged<BuildContext>? onAnchorTap =
                      widget.onAnchorTap;
                  if (onAnchorTap != null) {
                    onAnchorTap(context);
                  } else {
                    widget.onTap?.call();
                  }
                }
              },
        child: SizedBox(
          width: 32,
          height: 32,
          child: Center(
            child: AnimatedScale(
              scale: _isHovered ? 1.08 : 1,
              duration: const Duration(milliseconds: 120),
              curve: Curves.easeOutCubic,
              child: Icon(widget.icon, size: 18, color: iconColor),
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
  return _buildDesktopMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
  );
}

Widget _buildDesktopMediaRowPlaylistSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
  );
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
  return _buildDesktopMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
  );
}

Widget _buildDesktopMediaRowUnknownSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
  );
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
      final double insetCoverSize = math.max(
        1,
        coverSize - (_kRowItemInset * 2),
      );

      double safeClamp({required double preferred, required double minWidth}) {
        final double lower = math.min(minWidth, safeItemWidth);
        final double upper = math.max(lower, safeItemWidth);
        return preferred.clamp(lower, upper).toDouble();
      }

      final double titleWidth = safeClamp(
        preferred: math.min(safeItemWidth - 8, 118),
        minWidth: 52,
      );
      final double secondaryWidth = safeClamp(
        preferred: math.min(safeItemWidth - 16, 92),
        minWidth: 44,
      );
      final double tertiaryWidth = safeClamp(
        preferred: math.min(safeItemWidth - 24, 74),
        minWidth: 40,
      );

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
        width: insetCoverSize,
        height: insetCoverSize,
        color: colors.bgTertiary,
      );

      return SizedBox(
        width: itemWidth,
        child: Padding(
          padding: const EdgeInsets.all(_kRowItemInset),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              isCircularCover
                  ? ClipOval(child: cover)
                  : ClipRRect(
                      borderRadius: BorderRadius.circular(8),
                      child: cover,
                    ),
              const SizedBox(height: 12),
              line(titleWidth),
              const SizedBox(height: 6),
              line(secondaryWidth),
              const SizedBox(height: 6),
              line(tertiaryWidth),
            ],
          ),
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
