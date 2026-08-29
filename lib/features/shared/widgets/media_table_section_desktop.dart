import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'dart:async';
import 'dart:math' as math;

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../../playback/playback_coordinator.dart';
import '../../playback/playback_models.dart';
import '../selection/selection_controller.dart';
import 'media_link_text.dart';
import 'media_image.dart';

typedef DesktopRowAction<T> = void Function(T item, int index);
typedef DesktopRowMoreAnchorAction<T> =
    void Function(T item, int index, BuildContext anchorContext);
typedef DesktopRowContextMenuAction<T> =
    void Function(T item, int index, Offset globalPosition);
typedef DesktopRowSelectionContextMenuAction<T> =
    void Function(List<T> items, Offset globalPosition);
typedef DesktopRowFavoriteResolver<T> = bool Function(T item, int index);

class MediaTableSectionDesktop<T> extends StatefulWidget {
  final String title;
  final List<T> items;
  final bool isLoading;
  final String emptyText;
  final int skeletonRowCount;
  final int maxItemsPerPage;
  final bool enableSelection;
  final ValueChanged<List<T>>? onSelectionChanged;
  final VoidCallback? onSelectionMenuTap;
  final VoidCallback? onHeaderMoreTap;
  final DesktopRowAction<T>? onRowMoreTap;
  final DesktopRowMoreAnchorAction<T>? onRowMoreAnchorTap;
  final DesktopRowContextMenuAction<T>? onRowContextMenu;
  final DesktopRowSelectionContextMenuAction<T>? onRowSelectionContextMenu;
  final DesktopRowAction<T>? onRowFavoriteTap;
  final DesktopRowAction<T>? onRowDoubleTap;
  final DesktopRowFavoriteResolver<T>? isRowFavorite;
  final PlaybackCoordinator? playbackCoordinator;
  final ValueChanged<Artist>? onArtistTap;
  final ValueChanged<Album>? onAlbumTap;
  final ValueChanged<Playlist>? onPlaylistTap;

  const MediaTableSectionDesktop({
    super.key,
    required this.title,
    required this.items,
    required this.isLoading,
    required this.emptyText,
    this.skeletonRowCount = 10,
    this.maxItemsPerPage = 10,
    this.enableSelection = true,
    this.onSelectionChanged,
    this.onSelectionMenuTap,
    this.onHeaderMoreTap,
    this.onRowMoreTap,
    this.onRowMoreAnchorTap,
    this.onRowContextMenu,
    this.onRowSelectionContextMenu,
    this.onRowFavoriteTap,
    this.onRowDoubleTap,
    this.isRowFavorite,
    this.playbackCoordinator,
    this.onArtistTap,
    this.onAlbumTap,
    this.onPlaylistTap,
  });

  @override
  State<MediaTableSectionDesktop<T>> createState() =>
      _MediaTableSectionDesktopState<T>();
}

class _MediaTableSectionDesktopState<T>
    extends State<MediaTableSectionDesktop<T>> {
  static const double _desktopRowExtent = 64;
  static const Duration _desktopPageFadeDuration = Duration(milliseconds: 140);

  final FocusNode _sectionFocusNode = FocusNode(
    debugLabel: 'MediaTableDesktopSectionFocus',
  );
  final Set<int> _selectedIndexes = <int>{};
  final Object _selectionSessionOwner = Object();
  final SelectionController _selectionSessionController =
      SelectionController.instance;
  final Set<int> _suppressedSelectionPointers = <int>{};
  int? _selectionAnchorIndex;
  int _currentPage = 0;
  bool _isPageFading = false;
  bool _isExpanded = false;
  int _expandedBaseCount = 0;
  // Height changes only animate while an explicit expand/collapse is in
  // flight; it stays zero otherwise so unrelated rebuilds (data refresh,
  // scrolling, ...) resize the section instantly without a shrink animation.
  Duration _pageHeightAnimationDuration = Duration.zero;

  @override
  void didUpdateWidget(covariant MediaTableSectionDesktop<T> oldWidget) {
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

    if (!_isPagingEnabled) {
      _currentPage = 0;
      return;
    }

    if (_isExpanded && widget.items.length <= _pageSize) {
      _isExpanded = false;
    }

    final int lastPage = _pageCount > 0 ? _pageCount - 1 : 0;
    if (_currentPage > lastPage) {
      _currentPage = lastPage;
    }
  }

  @override
  void dispose() {
    _sectionFocusNode.dispose();
    _deactivateSelectionSession();
    super.dispose();
  }

  // Selection and Paging
  bool get _isPagingEnabled => widget.maxItemsPerPage > 0;
  bool get _isAllSelected =>
      widget.items.isNotEmpty && _selectedIndexes.length == widget.items.length;
  int get _chunkSize => _isPagingEnabled
      ? math.min(widget.maxItemsPerPage, widget.items.length)
      : 0;

  int get _pageSize {
    if (!_isPagingEnabled) {
      return widget.items.length;
    }
    return widget.maxItemsPerPage;
  }

  int get _pageCount {
    if (!_isPagingEnabled || widget.items.isEmpty) {
      return 1;
    }
    return (widget.items.length / _pageSize).ceil();
  }

  bool _isItemFavorite(T item, int index) {
    final DesktopRowFavoriteResolver<T>? resolver = widget.isRowFavorite;
    if (resolver != null) {
      return resolver(item, index);
    }
    if (item is QueueItem) {
      return item.mediaItem?.favorite ?? false;
    }
    return (item as MediaItem?)?.favorite ?? false;
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
    for (int i = startIndex; i < widget.items.length; i++) {
      final MediaItem? resolved = _resolvePlayableMediaItem(widget.items[i]);
      if (resolved == null || !_isValidPlayable(resolved)) {
        continue;
      }
      playItems.add(resolved);
    }

    return playItems;
  }

  Future<bool> _handleRowActivate({required T item, required int index}) async {
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
    if (coordinator == null || index < 0 || index >= widget.items.length) {
      return false;
    }

    final Object? rawItem = item;
    if (rawItem is QueueItem) {
      try {
        await coordinator.playQueueIndex(index);
        return true;
      } catch (_) {
        return false;
      }
    }

    final List<MediaItem> playItems = _playableItemsFromIndex(index);
    if (playItems.isEmpty) {
      return false;
    }

    try {
      await coordinator.playMedia(playItems);
      return true;
    } catch (_) {
      return false;
    }
  }

  void _onRowActivated({required T item, required int index}) {
    unawaited(() async {
      final bool handled = await _handleRowActivate(item: item, index: index);
      if (!handled) {
        widget.onRowDoubleTap?.call(item, index);
      }
    }());
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

  void _handleSectionPointerDown(PointerDownEvent event) {
    _requestSectionFocus();
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

  void _handleRowPointerDown(int index, PointerDownEvent event) {
    if (event.kind == PointerDeviceKind.mouse &&
        (event.buttons & kSecondaryMouseButton) != 0) {
      _suppressedSelectionPointers.remove(event.pointer);
      final DesktopRowContextMenuAction<T>? handler = widget.onRowContextMenu;
      final DesktopRowSelectionContextMenuAction<T>? selectionHandler =
          widget.onRowSelectionContextMenu;
      if (index >= 0 && index < widget.items.length) {
        _selectItemForRightClickMenu(index);
        final List<T> selectedItems = _selectedIndexes
            .where(
              (int selectedIndex) =>
                  selectedIndex >= 0 && selectedIndex < widget.items.length,
            )
            .map((int selectedIndex) => widget.items[selectedIndex])
            .toList(growable: false);
        if (selectionHandler != null && selectedItems.isNotEmpty) {
          selectionHandler(selectedItems, event.position);
        } else {
          handler?.call(widget.items[index], index, event.position);
        }
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

    // Perform single selection on pointer down to avoid tap-delay caused by
    // double-tap recognizers and to snapshot modifier state immediately.
    _selectSingleIndex(index);
  }

  void _suppressSelectionForPointer(int pointer) {
    _suppressedSelectionPointers.add(pointer);
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

  void _goToPreviousPage() {
    if (_isExpanded || !_isPagingEnabled || _currentPage <= 0) {
      return;
    }
    _fadeToPage(_currentPage - 1);
  }

  void _goToNextPage() {
    if (_isExpanded || !_isPagingEnabled || _currentPage >= _pageCount - 1) {
      return;
    }
    _fadeToPage(_currentPage + 1);
  }

  bool get _canToggleExpand =>
      _isPagingEnabled && widget.items.length > _pageSize;

  void _toggleExpanded() {
    if (!_canToggleExpand) {
      return;
    }

    setState(() {
      _isExpanded = !_isExpanded;
      // Rows up to and including the current page keep their identity; only
      // rows beyond that point animate in from above.
      _expandedBaseCount = _isExpanded
          ? math.min((_currentPage + 1) * _pageSize, widget.items.length)
          : 0;
      _currentPage = 0;
      _pageHeightAnimationDuration = const Duration(milliseconds: 380);
    });
  }

  Future<void> _fadeToPage(int targetPage) async {
    if (_isPageFading ||
        targetPage < 0 ||
        targetPage >= _pageCount ||
        targetPage == _currentPage) {
      return;
    }

    setState(() {
      _isPageFading = true;
    });

    await Future<void>.delayed(_desktopPageFadeDuration);
    if (!mounted) {
      return;
    }

    setState(() {
      _currentPage = targetPage;
      _isPageFading = false;
    });
  }

  // Playback actions
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
  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool hasTitle = widget.title.trim().isNotEmpty;

    // Ohne Titel entfällt die komplette Kopfzeile (Trenner, Play/Shuffle, Paging).
    final bool showHeaderRow = hasTitle;

    final Widget content = LayoutBuilder(
      builder: (BuildContext context, BoxConstraints constraints) {
        final bool hasBoundedHeight = constraints.maxHeight.isFinite;
        final bool fillScrollableViewport =
            hasBoundedHeight && !_isPagingEnabled;

        final Widget tableContent = _buildDesktopContent(
          context,
          colors,
          constrainToHeight: hasBoundedHeight,
          fillScrollableViewport: fillScrollableViewport,
        );

        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: hasBoundedHeight ? MainAxisSize.max : MainAxisSize.min,
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
                  if (hasTitle && widget.items.isNotEmpty) ...<Widget>[
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
                      onTap: widget.items.isEmpty ? null : _onPlayAllTap,
                    ),
                    const SizedBox(width: 8),
                    SectionHeaderIconButton(
                      icon: Icons.shuffle,
                      tooltip: 'Zufällig',
                      onTap: widget.items.isEmpty ? null : _onShuffleAllTap,
                    ),
                  ],
                  if (_isPagingEnabled && !_isExpanded) ...<Widget>[
                    const SizedBox(width: 4),
                    const SectionHeaderVerticalDivider(),
                    _buildPaginationControls(context),
                  ],
                ],
              ),
            if (showHeaderRow) const SizedBox(height: 10),
            if (hasBoundedHeight)
              Expanded(child: ClipRect(child: tableContent))
            else
              tableContent,
            // Always reserve the toggle's space so sections without one
            // line up visually with sections that have it.
            if (!hasBoundedHeight)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: SizedBox(
                  height: 24,
                  child: _canToggleExpand
                      ? Center(
                          child: _ExpandToggleButton(
                            isExpanded: _isExpanded,
                            onTap: _toggleExpanded,
                          ),
                        )
                      : const SizedBox.shrink(),
                ),
              ),
          ],
        );
      },
    );

    final Widget focusableContent = Focus(
      focusNode: _sectionFocusNode,
      child: Listener(onPointerDown: _handleSectionPointerDown, child: content),
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

  Widget _buildDesktopContent(
    BuildContext context,
    MashinColors colors, {
    required bool constrainToHeight,
    required bool fillScrollableViewport,
  }) {
    final bool useInternalScroll = fillScrollableViewport || constrainToHeight;
    final int visibleRowCount = _isExpanded ? widget.items.length : _chunkSize;
    final double pagedContentHeight = visibleRowCount * _desktopRowExtent;

    if (widget.isLoading) {
      final Widget body = ListView.builder(
        primary: false,
        shrinkWrap: !useInternalScroll,
        physics: useInternalScroll
            ? const ClampingScrollPhysics()
            : const NeverScrollableScrollPhysics(),
        addAutomaticKeepAlives: false,
        addRepaintBoundaries: true,
        addSemanticIndexes: false,
        itemCount: widget.skeletonRowCount,
        itemExtent: _desktopRowExtent,
        itemBuilder: (BuildContext context, int index) {
          return _buildDesktopTypedSkeletonRow(isFirst: index == 0);
        },
      );

      return Column(
        mainAxisSize: useInternalScroll ? MainAxisSize.max : MainAxisSize.min,
        children: <Widget>[
          _buildDesktopTypedHeader(),
          if (useInternalScroll) Expanded(child: body) else body,
        ],
      );
    }

    if (widget.items.isEmpty) {
      // No column header for empty state; the section header (title,
      // divider, disabled buttons) is rendered by build() above this.
      return Text(
        widget.emptyText,
        style: context.mashinTableTextStyles.emptyState,
      );
    }

    final Widget expandedList = ListView.builder(
      // Keyed by expansion state so collapsing back to paged mode rebuilds
      // the list structure, while expanding keeps row identities intact.
      key: ValueKey<String>('table-rows-$T-paged-$_isExpanded'),
      primary: false,
      shrinkWrap: !useInternalScroll,
      physics: useInternalScroll
          ? const ClampingScrollPhysics()
          : const NeverScrollableScrollPhysics(),
      addAutomaticKeepAlives: false,
      addRepaintBoundaries: true,
      addSemanticIndexes: false,
      itemCount: _isExpanded
          ? widget.items.length
          : math.min(widget.items.length, _pageSize),
      itemExtent: _desktopRowExtent,
      itemBuilder: (BuildContext context, int index) {
        final int itemIndex = index;
        final Widget row = _buildDesktopTypedRow(
          item: widget.items[index],
          index: itemIndex,
          isFirst: index == 0,
          isSelected: _selectedIndexes.contains(itemIndex),
          onToggleSelected: () => _toggleSelectedIndex(itemIndex),
          onPointerDown: (PointerDownEvent event) =>
              _handleRowPointerDown(itemIndex, event),
        );

        // Rows of the page that was visible before expanding stay static;
        // only newly appended rows slide in from above, staggered per page.
        if (_isExpanded && index >= _expandedBaseCount) {
          final int pageIndex =
              (index - _expandedBaseCount) ~/ math.max(1, _pageSize);
          return _ExpandRevealRow(
            key: ValueKey<String>('table-expand-reveal-$index'),
            delay: Duration(milliseconds: 70 * pageIndex),
            child: row,
          );
        }

        return row;
      },
    );

    final Widget list = _isPagingEnabled && !_isExpanded
        ? AnimatedOpacity(
            duration: _desktopPageFadeDuration,
            curve: Curves.easeOut,
            opacity: _isPageFading ? 0 : 1,
            child: KeyedSubtree(
              key: ValueKey<int>(_currentPage),
              child: _buildDesktopPageList(
                pageIndex: _currentPage,
                useInternalScroll: useInternalScroll,
              ),
            ),
          )
        : expandedList;

    return Column(
      mainAxisSize: useInternalScroll ? MainAxisSize.max : MainAxisSize.min,
      children: <Widget>[
        _buildDesktopTypedHeader(),
        if (useInternalScroll)
          Expanded(child: list)
        else if (_isPagingEnabled)
          // Only animates while _pageHeightAnimationDuration is non-zero, i.e.
          // during an explicit expand/collapse; otherwise resizes instantly.
          AnimatedContainer(
            duration: _pageHeightAnimationDuration,
            curve: Curves.easeInOutCubic,
            height: pagedContentHeight,
            onEnd: () {
              if (_pageHeightAnimationDuration != Duration.zero) {
                setState(() => _pageHeightAnimationDuration = Duration.zero);
              }
            },
            child: ClipRect(
              child: OverflowBox(
                alignment: Alignment.topCenter,
                minHeight: 0,
                maxHeight: double.infinity,
                child: SizedBox(
                  height: widget.items.length * _desktopRowExtent,
                  child: _isExpanded ? expandedList : list,
                ),
              ),
            ),
          )
        else
          list,
      ],
    );
  }

  Widget _buildDesktopPageList({
    required int pageIndex,
    required bool useInternalScroll,
  }) {
    final int start = pageIndex * _chunkSize;
    final int end = math.min(start + _chunkSize, widget.items.length);
    final List<T> chunkItems = widget.items.sublist(start, end);

    return ListView.builder(
      primary: false,
      shrinkWrap: !useInternalScroll,
      physics: useInternalScroll
          ? const ClampingScrollPhysics()
          : const NeverScrollableScrollPhysics(),
      addAutomaticKeepAlives: false,
      addRepaintBoundaries: true,
      addSemanticIndexes: false,
      itemCount: chunkItems.length,
      itemExtent: _desktopRowExtent,
      itemBuilder: (BuildContext context, int index) {
        final int itemIndex = start + index;
        return _buildDesktopTypedRow(
          item: chunkItems[index],
          index: itemIndex,
          isFirst: index == 0,
          isSelected: _selectedIndexes.contains(itemIndex),
          onToggleSelected: () => _toggleSelectedIndex(itemIndex),
          onPointerDown: (PointerDownEvent event) =>
              _handleRowPointerDown(itemIndex, event),
        );
      },
    );
  }

  // Typed desktop table builders
  Widget _buildDesktopTypedHeader() {
    if (T == Album) {
      return _buildDesktopTableHeaderAlbum(
        showSelection: widget.enableSelection,
        isAllSelected: _isAllSelected,
        onToggleSelectAll: _toggleAllSelectedIndexes,
        onMoreTap: widget.onHeaderMoreTap,
      );
    }
    if (T == Playlist) {
      return _buildDesktopTableHeaderPlaylist(
        showSelection: widget.enableSelection,
        isAllSelected: _isAllSelected,
        onToggleSelectAll: _toggleAllSelectedIndexes,
        onMoreTap: widget.onHeaderMoreTap,
      );
    }
    if (T == Artist) {
      return _buildDesktopTableHeaderArtist(
        showSelection: widget.enableSelection,
        isAllSelected: _isAllSelected,
        onToggleSelectAll: _toggleAllSelectedIndexes,
        onMoreTap: widget.onHeaderMoreTap,
      );
    }
    if (T == Track) {
      return _buildDesktopTableHeaderTrack(
        showSelection: widget.enableSelection,
        isAllSelected: _isAllSelected,
        onToggleSelectAll: _toggleAllSelectedIndexes,
        onMoreTap: widget.onHeaderMoreTap,
      );
    }
    if (T == QueueItem) {
      return _buildDesktopTableHeaderQueue(
        showSelection: widget.enableSelection,
        isAllSelected: _isAllSelected,
        onToggleSelectAll: _toggleAllSelectedIndexes,
        onMoreTap: widget.onHeaderMoreTap,
      );
    }

    return _buildDesktopTableHeaderTrack(
      showSelection: widget.enableSelection,
      isAllSelected: _isAllSelected,
      onToggleSelectAll: _toggleAllSelectedIndexes,
      onMoreTap: widget.onHeaderMoreTap,
    );
  }

  Widget _buildDesktopTypedRow({
    required T item,
    required int index,
    required bool isFirst,
    required bool isSelected,
    required VoidCallback onToggleSelected,
    required ValueChanged<PointerDownEvent> onPointerDown,
  }) {
    final Object? typedItem = item;

    if (typedItem is Album) {
      return _buildDesktopTableRowAlbum(
        item: typedItem,
        index: index,
        isFirst: isFirst,
        isSelected: isSelected,
        isFavorite: _isItemFavorite(item, index),
        selectionEnabled: widget.enableSelection,
        onToggleSelected: onToggleSelected,
        onFavoriteTap: widget.onRowFavoriteTap == null
            ? null
            : () => widget.onRowFavoriteTap!(item, index),
        onMoreTap: widget.onRowMoreTap == null
            ? null
            : () => widget.onRowMoreTap!(item, index),
        onMoreAnchorTap: _moreAnchorHandler(item, index),
        onDoubleTap: () => _onRowActivated(item: item, index: index),
        onPointerDown: onPointerDown,
      );
    }
    if (typedItem is Playlist) {
      return _buildDesktopTableRowPlaylist(
        item: typedItem,
        index: index,
        isFirst: isFirst,
        isSelected: isSelected,
        isFavorite: _isItemFavorite(item, index),
        selectionEnabled: widget.enableSelection,
        onToggleSelected: onToggleSelected,
        onFavoriteTap: widget.onRowFavoriteTap == null
            ? null
            : () => widget.onRowFavoriteTap!(item, index),
        onMoreTap: widget.onRowMoreTap == null
            ? null
            : () => widget.onRowMoreTap!(item, index),
        onMoreAnchorTap: _moreAnchorHandler(item, index),
        onDoubleTap: () => _onRowActivated(item: item, index: index),
        onPointerDown: onPointerDown,
        onPlaylistTap: widget.onPlaylistTap,
      );
    }
    if (typedItem is Artist) {
      return _buildDesktopTableRowArtist(
        item: typedItem,
        index: index,
        isFirst: isFirst,
        isSelected: isSelected,
        isFavorite: _isItemFavorite(item, index),
        selectionEnabled: widget.enableSelection,
        onToggleSelected: onToggleSelected,
        onFavoriteTap: widget.onRowFavoriteTap == null
            ? null
            : () => widget.onRowFavoriteTap!(item, index),
        onMoreTap: widget.onRowMoreTap == null
            ? null
            : () => widget.onRowMoreTap!(item, index),
        onMoreAnchorTap: _moreAnchorHandler(item, index),
        onDoubleTap: () => _onRowActivated(item: item, index: index),
        onPointerDown: onPointerDown,
      );
    }
    if (typedItem is Track) {
      return _buildDesktopTableRowTrack(
        item: typedItem,
        playbackCoordinator: widget.playbackCoordinator,
        index: index,
        isFirst: isFirst,
        isSelected: isSelected,
        isFavorite: _isItemFavorite(item, index),
        selectionEnabled: widget.enableSelection,
        onToggleSelected: onToggleSelected,
        onFavoriteTap: widget.onRowFavoriteTap == null
            ? null
            : () => widget.onRowFavoriteTap!(item, index),
        onMoreTap: widget.onRowMoreTap == null
            ? null
            : () => widget.onRowMoreTap!(item, index),
        onMoreAnchorTap: _moreAnchorHandler(item, index),
        onDoubleTap: () => _onRowActivated(item: item, index: index),
        onPointerDown: onPointerDown,
        onArtistTap: widget.onArtistTap,
        onArtistPointerDown: _suppressSelectionForPointer,
        onAlbumTap: widget.onAlbumTap,
      );
    }
    if (typedItem is QueueItem) {
      return _buildDesktopTableRowQueue(
        item: typedItem,
        playbackCoordinator: widget.playbackCoordinator,
        index: index,
        isFirst: isFirst,
        isSelected: isSelected,
        isFavorite: _isItemFavorite(item, index),
        selectionEnabled: widget.enableSelection,
        onToggleSelected: onToggleSelected,
        onFavoriteTap: widget.onRowFavoriteTap == null
            ? null
            : () => widget.onRowFavoriteTap!(item, index),
        onMoreTap: widget.onRowMoreTap == null
            ? null
            : () => widget.onRowMoreTap!(item, index),
        onMoreAnchorTap: _moreAnchorHandler(item, index),
        onDoubleTap: () => _onRowActivated(item: item, index: index),
        onPointerDown: onPointerDown,
        onArtistTap: widget.onArtistTap,
        onArtistPointerDown: _suppressSelectionForPointer,
        onAlbumTap: widget.onAlbumTap,
      );
    }

    return _buildDesktopTableRowUnknown(
      itemLabel: typedItem?.toString() ?? '-',
      isFirst: isFirst,
      isSelected: isSelected,
      isFavorite: _isItemFavorite(item, index),
      selectionEnabled: widget.enableSelection,
      onToggleSelected: onToggleSelected,
      onFavoriteTap: widget.onRowFavoriteTap == null
          ? null
          : () => widget.onRowFavoriteTap!(item, index),
      onMoreTap: widget.onRowMoreTap == null
          ? null
          : () => widget.onRowMoreTap!(item, index),
      onMoreAnchorTap: _moreAnchorHandler(item, index),
      onDoubleTap: () => _onRowActivated(item: item, index: index),
      onPointerDown: onPointerDown,
    );
  }

  // Typed desktop table skeleton builders
  Widget _buildDesktopTypedSkeletonRow({required bool isFirst}) {
    if (T == Album) {
      return _buildDesktopTableRowAlbumSkeleton(
        isFirst: isFirst,
        showSelection: widget.enableSelection,
      );
    }
    if (T == Playlist) {
      return _buildDesktopTableRowPlaylistSkeleton(
        isFirst: isFirst,
        showSelection: widget.enableSelection,
      );
    }
    if (T == Artist) {
      return _buildDesktopTableRowArtistSkeleton(
        isFirst: isFirst,
        showSelection: widget.enableSelection,
      );
    }
    if (T == Track) {
      return _buildDesktopTableRowTrackSkeleton(
        isFirst: isFirst,
        showSelection: widget.enableSelection,
      );
    }
    if (T == QueueItem) {
      return _buildDesktopTableRowQueueSkeleton(
        isFirst: isFirst,
        showSelection: widget.enableSelection,
      );
    }

    return _buildDesktopTableRowUnknownSkeleton(
      isFirst: isFirst,
      showSelection: widget.enableSelection,
    );
  }

  Widget _buildPaginationControls(BuildContext context) {
    final bool canGoBack = _currentPage > 0;
    final bool canGoNext = _currentPage < _pageCount - 1;

    return Row(
      mainAxisSize: MainAxisSize.min,
      children: <Widget>[
        IconButton(
          tooltip: 'Vorherige Seite',
          onPressed: canGoBack ? _goToPreviousPage : null,
          icon: const Icon(Icons.chevron_left_outlined),
          visualDensity: VisualDensity.compact,
        ),
        IconButton(
          tooltip: 'Nächste Seite',
          onPressed: canGoNext ? _goToNextPage : null,
          icon: const Icon(Icons.chevron_right_outlined),
          visualDensity: VisualDensity.compact,
        ),
      ],
    );
  }
}

// Typed desktop table widgets
Widget _buildDesktopTableHeaderTrack({
  required bool showSelection,
  required bool isAllSelected,
  required VoidCallback onToggleSelectAll,
  required VoidCallback? onMoreTap,
}) {
  return _buildDesktopTableHeaderBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 40,
        child: _buildDesktopHeaderText('#', TextAlign.center),
      ),
      _buildDesktopTableSizedCell(
        width: 48,
        child: _buildDesktopHeaderText('', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        flex: 5,
        child: _buildDesktopHeaderText('Titel', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        width: 80,
        child: _buildDesktopHeaderText('Dauer', TextAlign.right),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopHeaderText('Album', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopHeaderText('Artist', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        flex: 1,
        child: _buildDesktopHeaderText('Jahr', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        width: 80,
        child: _buildDesktopHeaderText('Quelle', TextAlign.center),
      ),
    ],
    showSelection: showSelection,
    isAllSelected: isAllSelected,
    onToggleSelectAll: onToggleSelectAll,
    onMoreTap: onMoreTap,
  );
}

Widget _buildDesktopTableHeaderQueue({
  required bool showSelection,
  required bool isAllSelected,
  required VoidCallback onToggleSelectAll,
  required VoidCallback? onMoreTap,
}) {
  return _buildDesktopTableHeaderBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 40,
        child: _buildDesktopHeaderText('#', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        width: 48,
        child: _buildDesktopHeaderText('', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        flex: 5,
        child: _buildDesktopHeaderText('Titel', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        width: 80,
        child: _buildDesktopHeaderText('Dauer', TextAlign.right),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopHeaderText('Album', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopHeaderText('Artist', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        width: 80,
        child: _buildDesktopHeaderText('Quelle', TextAlign.center),
      ),
    ],
    showSelection: showSelection,
    isAllSelected: isAllSelected,
    onToggleSelectAll: onToggleSelectAll,
    onMoreTap: onMoreTap,
  );
}

Widget _buildDesktopTableHeaderAlbum({
  required bool showSelection,
  required bool isAllSelected,
  required VoidCallback onToggleSelectAll,
  required VoidCallback? onMoreTap,
}) {
  return _buildDesktopTableHeaderBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 44,
        child: _buildDesktopHeaderText('#', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        width: 62,
        child: _buildDesktopHeaderText('', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        flex: 5,
        child: _buildDesktopHeaderText('Titel', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        flex: 4,
        child: _buildDesktopHeaderText('Artist', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        flex: 2,
        child: _buildDesktopHeaderText('Typ', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        width: 64,
        child: _buildDesktopHeaderText('Jahr', TextAlign.right),
      ),
      _buildDesktopTableSizedCell(
        width: 72,
        child: _buildDesktopHeaderText('Quelle', TextAlign.center),
      ),
    ],
    showSelection: showSelection,
    isAllSelected: isAllSelected,
    onToggleSelectAll: onToggleSelectAll,
    onMoreTap: onMoreTap,
  );
}

Widget _buildDesktopTableHeaderPlaylist({
  required bool showSelection,
  required bool isAllSelected,
  required VoidCallback onToggleSelectAll,
  required VoidCallback? onMoreTap,
}) {
  return _buildDesktopTableHeaderBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 44,
        child: _buildDesktopHeaderText('#', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        width: 62,
        child: _buildDesktopHeaderText('', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        flex: 5,
        child: _buildDesktopHeaderText('Titel', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        flex: 4,
        child: _buildDesktopHeaderText('Owner', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        flex: 2,
        child: _buildDesktopHeaderText('Status', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        width: 72,
        child: _buildDesktopHeaderText('Quelle', TextAlign.center),
      ),
    ],
    showSelection: showSelection,
    isAllSelected: isAllSelected,
    onToggleSelectAll: onToggleSelectAll,
    onMoreTap: onMoreTap,
  );
}

Widget _buildDesktopTableHeaderArtist({
  required bool showSelection,
  required bool isAllSelected,
  required VoidCallback onToggleSelectAll,
  required VoidCallback? onMoreTap,
}) {
  return _buildDesktopTableHeaderBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 44,
        child: _buildDesktopHeaderText('#', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        width: 62,
        child: _buildDesktopHeaderText('Bild', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        flex: 6,
        child: _buildDesktopHeaderText('Artist', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopHeaderText('Provider', TextAlign.left),
      ),
      _buildDesktopTableSizedCell(
        width: 72,
        child: _buildDesktopHeaderText('Quelle', TextAlign.center),
      ),
    ],
    showSelection: showSelection,
    isAllSelected: isAllSelected,
    onToggleSelectAll: onToggleSelectAll,
    onMoreTap: onMoreTap,
  );
}

Widget _buildDesktopTableRowTrack({
  required Track item,
  required PlaybackCoordinator? playbackCoordinator,
  required int index,
  required bool isFirst,
  required bool isSelected,
  required bool isFavorite,
  required bool selectionEnabled,
  required VoidCallback onToggleSelected,
  required VoidCallback? onFavoriteTap,
  required VoidCallback? onMoreTap,
  ValueChanged<BuildContext>? onMoreAnchorTap,
  required VoidCallback? onDoubleTap,
  required ValueChanged<PointerDownEvent>? onPointerDown,
  ValueChanged<Artist>? onArtistTap,
  ValueChanged<int>? onArtistPointerDown,
  ValueChanged<Album>? onAlbumTap,
}) {
  return _buildDesktopTableRowBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 40,
        child: Center(
          child: DesktopPlaybackIndicator(
            coordinator: playbackCoordinator,
            item: item,
            fallback: '${index + 1}',
          ),
        ),
      ),
      _buildDesktopTableSizedCell(
        width: 48,
        child: MediaTableCover(
          coverUrl: item.imageUriForSize(80),
          fallbackIcon: Icons.music_note_outlined,
          size: 48,
          radius: 8,
          onPlayTap: onDoubleTap,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 5,
        child: _buildDesktopTrackTitleCell(
          playbackCoordinator: playbackCoordinator,
          playbackItem: item,
          title: _prefer(item.name, 'Unknown Track'),
        ),
      ),
      _buildDesktopTableSizedCell(
        width: 80,
        child: _buildDesktopBodyText(
          formatDuration(item.duration),
          TextAlign.right,
          DesktopTextTone.tertiary,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopAlbumLinkCell(
          album: item.album,
          onAlbumTap: onAlbumTap,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopArtistLinkCell(
          artists: item.artists,
          fallbackText: _trackSubtitle(item),
          onArtistTap: onArtistTap,
          onArtistPointerDown: onArtistPointerDown,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 1,
        child: _buildDesktopBodyText(
          item.album?.year?.toString() ?? '-',
          TextAlign.left,
          DesktopTextTone.tertiary,
        ),
      ),
      _buildDesktopTableSizedCell(
        width: 80,
        child: Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: <Widget>[
            MediaProviderIcon(
              providerLabel: _providerLabelFromId(item.provider),
              iconSvg: ProviderManifestRegistry.iconSvgForProvider(
                item.provider,
              ),
              size: 16,
            ),
          ],
        ),
      ),
    ],
    isFirst: isFirst,
    isSelected: isSelected,
    isFavorite: isFavorite,
    selectionEnabled: selectionEnabled,
    onToggleSelected: onToggleSelected,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    onMoreAnchorTap: onMoreAnchorTap,
    onDoubleTap: onDoubleTap,
    onPointerDown: onPointerDown,
  );
}

Widget _buildDesktopTableRowAlbum({
  required Album item,
  required int index,
  required bool isFirst,
  required bool isSelected,
  required bool isFavorite,
  required bool selectionEnabled,
  required VoidCallback onToggleSelected,
  required VoidCallback? onFavoriteTap,
  required VoidCallback? onMoreTap,
  ValueChanged<BuildContext>? onMoreAnchorTap,
  required VoidCallback? onDoubleTap,
  required ValueChanged<PointerDownEvent>? onPointerDown,
}) {
  return _buildDesktopTableRowBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 44,
        child: _buildDesktopBodyText(
          '${index + 1}',
          TextAlign.left,
          DesktopTextTone.tertiary,
        ),
      ),
      _buildDesktopTableSizedCell(
        width: 62,
        child: MediaTableCover(
          coverUrl: item.imageUriForSize(80),
          fallbackIcon: Icons.album_outlined,
          size: 48,
          radius: 8,
          onPlayTap: onDoubleTap,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 5,
        child: _buildDesktopBodyText(
          _prefer(item.name, 'Unknown Album'),
          TextAlign.left,
          DesktopTextTone.primary,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 4,
        child: _buildDesktopBodyText(
          _albumSubtitle(item),
          TextAlign.left,
          DesktopTextTone.secondary,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 2,
        child: _buildDesktopBodyText(
          _mediaTypeLabel(item.albumType),
          TextAlign.left,
          DesktopTextTone.secondary,
        ),
      ),
      _buildDesktopTableSizedCell(
        width: 64,
        child: _buildDesktopBodyText(
          item.year != null && item.year! > 0 ? '${item.year}' : '-',
          TextAlign.right,
          DesktopTextTone.tertiary,
        ),
      ),
      _buildDesktopTableSizedCell(
        width: 72,
        child: Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: <Widget>[
            MediaProviderIcon(
              providerLabel: _providerLabelFromId(item.provider),
              iconSvg: ProviderManifestRegistry.iconSvgForProvider(
                item.provider,
              ),
              size: 16,
            ),
          ],
        ),
      ),
    ],
    isFirst: isFirst,
    isSelected: isSelected,
    isFavorite: isFavorite,
    selectionEnabled: selectionEnabled,
    onToggleSelected: onToggleSelected,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    onMoreAnchorTap: onMoreAnchorTap,
    onDoubleTap: onDoubleTap,
    onPointerDown: onPointerDown,
  );
}

Widget _buildDesktopTableRowPlaylist({
  required Playlist item,
  required int index,
  required bool isFirst,
  required bool isSelected,
  required bool isFavorite,
  required bool selectionEnabled,
  required VoidCallback onToggleSelected,
  required VoidCallback? onFavoriteTap,
  required VoidCallback? onMoreTap,
  ValueChanged<BuildContext>? onMoreAnchorTap,
  required VoidCallback? onDoubleTap,
  required ValueChanged<PointerDownEvent>? onPointerDown,
  ValueChanged<Playlist>? onPlaylistTap,
}) {
  return _buildDesktopTableRowBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 44,
        child: _buildDesktopBodyText(
          '${index + 1}',
          TextAlign.left,
          DesktopTextTone.tertiary,
        ),
      ),
      _buildDesktopTableSizedCell(
        width: 62,
        child: MediaTableCover(
          coverUrl: item.imageUriForSize(80),
          fallbackIcon: Icons.queue_music_outlined,
          size: 48,
          radius: 8,
          onPlayTap: onDoubleTap,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 5,
        child: _buildDesktopPlaylistLinkCell(
          playlist: item,
          onPlaylistTap: onPlaylistTap,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 4,
        child: _buildDesktopBodyText(
          _playlistSubtitle(item),
          TextAlign.left,
          DesktopTextTone.secondary,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 2,
        child: _buildDesktopBodyText(
          item.isEditable ? 'Editierbar' : 'Readonly',
          TextAlign.left,
          DesktopTextTone.tertiary,
        ),
      ),
      _buildDesktopTableSizedCell(
        width: 72,
        child: Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: <Widget>[
            MediaProviderIcon(
              providerLabel: _providerLabelFromId(item.provider),
              iconSvg: ProviderManifestRegistry.iconSvgForProvider(
                item.provider,
              ),
              size: 16,
            ),
          ],
        ),
      ),
    ],
    isFirst: isFirst,
    isSelected: isSelected,
    isFavorite: isFavorite,
    selectionEnabled: selectionEnabled,
    onToggleSelected: onToggleSelected,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    onMoreAnchorTap: onMoreAnchorTap,
    onDoubleTap: onDoubleTap,
    onPointerDown: onPointerDown,
  );
}

Widget _buildDesktopTableRowArtist({
  required Artist item,
  required int index,
  required bool isFirst,
  required bool isSelected,
  required bool isFavorite,
  required bool selectionEnabled,
  required VoidCallback onToggleSelected,
  required VoidCallback? onFavoriteTap,
  required VoidCallback? onMoreTap,
  ValueChanged<BuildContext>? onMoreAnchorTap,
  required VoidCallback? onDoubleTap,
  required ValueChanged<PointerDownEvent>? onPointerDown,
}) {
  return _buildDesktopTableRowBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 44,
        child: _buildDesktopBodyText(
          '${index + 1}',
          TextAlign.left,
          DesktopTextTone.tertiary,
        ),
      ),
      _buildDesktopTableSizedCell(
        width: 62,
        child: MediaTableCover(
          coverUrl: item.imageUriForSize(80),
          fallbackIcon: Icons.person_outlined,
          size: 48,
          radius: 8,
          onPlayTap: onDoubleTap,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 6,
        child: _buildDesktopBodyText(
          _prefer(item.name, 'Unknown Artist'),
          TextAlign.left,
          DesktopTextTone.primary,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopBodyText(
          _providerLabelFromId(item.provider),
          TextAlign.left,
          DesktopTextTone.secondary,
        ),
      ),
      _buildDesktopTableSizedCell(
        width: 72,
        child: Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: <Widget>[
            MediaProviderIcon(
              providerLabel: _providerLabelFromId(item.provider),
              iconSvg: ProviderManifestRegistry.iconSvgForProvider(
                item.provider,
              ),
              size: 16,
            ),
          ],
        ),
      ),
    ],
    isFirst: isFirst,
    isSelected: isSelected,
    isFavorite: isFavorite,
    selectionEnabled: selectionEnabled,
    onToggleSelected: onToggleSelected,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    onMoreAnchorTap: onMoreAnchorTap,
    onDoubleTap: onDoubleTap,
    onPointerDown: onPointerDown,
  );
}

Widget _buildDesktopTableRowQueue({
  required QueueItem item,
  required PlaybackCoordinator? playbackCoordinator,
  required int index,
  required bool isFirst,
  required bool isSelected,
  required bool isFavorite,
  required bool selectionEnabled,
  required VoidCallback onToggleSelected,
  required VoidCallback? onFavoriteTap,
  required VoidCallback? onMoreTap,
  ValueChanged<BuildContext>? onMoreAnchorTap,
  required VoidCallback? onDoubleTap,
  required ValueChanged<PointerDownEvent>? onPointerDown,
  ValueChanged<Artist>? onArtistTap,
  ValueChanged<int>? onArtistPointerDown,
  ValueChanged<Album>? onAlbumTap,
}) {
  final String provider = _resolveQueueProvider(item);
  final String title = (item.mediaItem?.name.trim().isNotEmpty == true)
      ? item.mediaItem!.name.trim()
      : item.name.trim().isNotEmpty
      ? item.name.trim()
      : 'Unbekannter Titel';
  final List<Artist> artists = item.mediaItem?.artists ?? const <Artist>[];
  final String artistsFallbackText = artists
      .map((artist) => artist.name.trim())
      .where((name) => name.isNotEmpty)
      .join(', ');

  return _buildDesktopTableRowBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 40,
        child: Center(
          child: DesktopPlaybackIndicator(
            coordinator: playbackCoordinator,
            item: item.mediaItem,
            fallback: '${index + 1}',
          ),
        ),
      ),
      _buildDesktopTableSizedCell(
        width: 48,
        child: MediaTableCover(
          coverUrl: item.mediaItem?.imageUriForSize(80),
          fallbackIcon: Icons.music_note_outlined,
          size: 48,
          radius: 8,
          onPlayTap: onDoubleTap,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 5,
        child: _buildDesktopTrackTitleCell(
          playbackCoordinator: playbackCoordinator,
          playbackItem: item.mediaItem,
          title: title,
        ),
      ),
      _buildDesktopTableSizedCell(
        width: 80,
        child: _buildDesktopBodyText(
          _formatQueueDurationSeconds((item.duration ?? 0).toDouble()),
          TextAlign.right,
          DesktopTextTone.tertiary,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopAlbumLinkCell(
          album: item.mediaItem?.album,
          onAlbumTap: onAlbumTap,
        ),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopArtistLinkCell(
          artists: artists,
          fallbackText: artistsFallbackText.isEmpty ? '-' : artistsFallbackText,
          onArtistTap: onArtistTap,
          onArtistPointerDown: onArtistPointerDown,
        ),
      ),
      _buildDesktopTableSizedCell(
        width: 80,
        child: Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: <Widget>[
            MediaProviderIcon(
              providerLabel: _providerLabelFromId(provider),
              iconSvg: ProviderManifestRegistry.iconSvgForProvider(provider),
              size: 16,
            ),
          ],
        ),
      ),
    ],
    isFirst: isFirst,
    isSelected: isSelected,
    isFavorite: isFavorite,
    selectionEnabled: selectionEnabled,
    onToggleSelected: onToggleSelected,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    onMoreAnchorTap: onMoreAnchorTap,
    onDoubleTap: onDoubleTap,
    onPointerDown: onPointerDown,
  );
}

Widget _buildDesktopTableRowUnknown({
  required String itemLabel,
  required bool isFirst,
  required bool isSelected,
  required bool isFavorite,
  required bool selectionEnabled,
  required VoidCallback onToggleSelected,
  required VoidCallback? onFavoriteTap,
  required VoidCallback? onMoreTap,
  ValueChanged<BuildContext>? onMoreAnchorTap,
  required VoidCallback? onDoubleTap,
  required ValueChanged<PointerDownEvent>? onPointerDown,
}) {
  return _buildDesktopTableRowBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        flex: 1,
        child: _buildDesktopBodyText(
          itemLabel,
          TextAlign.left,
          DesktopTextTone.primary,
        ),
      ),
    ],
    isFirst: isFirst,
    isSelected: isSelected,
    isFavorite: isFavorite,
    selectionEnabled: selectionEnabled,
    onToggleSelected: onToggleSelected,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    onMoreAnchorTap: onMoreAnchorTap,
    onDoubleTap: onDoubleTap,
    onPointerDown: onPointerDown,
  );
}

Widget _buildDesktopTableRowTrackSkeleton({
  required bool isFirst,
  required bool showSelection,
}) {
  return _buildDesktopTableRowSkeletonBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 40,
        child: _buildDesktopFixedTextSkeleton(width: 12),
      ),
      _buildDesktopTableSizedCell(
        width: 48,
        child: _buildDesktopSkeletonBar(width: 48, height: 48, radius: 8),
      ),
      _buildDesktopTableSizedCell(
        flex: 5,
        child: _buildDesktopFixedTextSkeleton(width: 148),
      ),
      _buildDesktopTableSizedCell(
        width: 80,
        child: _buildDesktopFixedTextSkeleton(width: 36, alignRight: true),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopFixedTextSkeleton(width: 94),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopFixedTextSkeleton(width: 82),
      ),
      _buildDesktopTableSizedCell(
        flex: 1,
        child: _buildDesktopFixedTextSkeleton(width: 30),
      ),
      _buildDesktopTableSizedCell(
        width: 80,
        child: Center(
          child: _buildDesktopSkeletonBar(width: 16, height: 16, radius: 8),
        ),
      ),
    ],
    isFirst: isFirst,
    showSelection: showSelection,
  );
}

Widget _buildDesktopTableRowQueueSkeleton({
  required bool isFirst,
  required bool showSelection,
}) {
  return _buildDesktopTableRowSkeletonBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 40,
        child: _buildDesktopFixedTextSkeleton(width: 12),
      ),
      _buildDesktopTableSizedCell(
        width: 48,
        child: _buildDesktopSkeletonBar(width: 48, height: 48, radius: 8),
      ),
      _buildDesktopTableSizedCell(
        flex: 5,
        child: _buildDesktopFixedTextSkeleton(width: 148),
      ),
      _buildDesktopTableSizedCell(
        width: 80,
        child: _buildDesktopFixedTextSkeleton(width: 36, alignRight: true),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopFixedTextSkeleton(width: 88),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopFixedTextSkeleton(width: 78),
      ),
      _buildDesktopTableSizedCell(
        width: 80,
        child: Center(
          child: _buildDesktopSkeletonBar(width: 16, height: 16, radius: 8),
        ),
      ),
    ],
    isFirst: isFirst,
    showSelection: showSelection,
  );
}

Widget _buildDesktopTableRowAlbumSkeleton({
  required bool isFirst,
  required bool showSelection,
}) {
  return _buildDesktopTableRowSkeletonBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 44,
        child: _buildDesktopFixedTextSkeleton(width: 12),
      ),
      _buildDesktopTableSizedCell(
        width: 62,
        child: _buildDesktopSkeletonBar(width: 48, height: 48, radius: 8),
      ),
      _buildDesktopTableSizedCell(
        flex: 5,
        child: _buildDesktopFixedTextSkeleton(width: 140),
      ),
      _buildDesktopTableSizedCell(
        flex: 4,
        child: _buildDesktopFixedTextSkeleton(width: 102),
      ),
      _buildDesktopTableSizedCell(
        flex: 2,
        child: _buildDesktopFixedTextSkeleton(width: 56),
      ),
      _buildDesktopTableSizedCell(
        width: 64,
        child: _buildDesktopFixedTextSkeleton(width: 28, alignRight: true),
      ),
      _buildDesktopTableSizedCell(
        width: 72,
        child: Center(
          child: _buildDesktopSkeletonBar(width: 16, height: 16, radius: 8),
        ),
      ),
    ],
    isFirst: isFirst,
    showSelection: showSelection,
  );
}

Widget _buildDesktopTableRowPlaylistSkeleton({
  required bool isFirst,
  required bool showSelection,
}) {
  return _buildDesktopTableRowSkeletonBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 44,
        child: _buildDesktopFixedTextSkeleton(width: 12),
      ),
      _buildDesktopTableSizedCell(
        width: 62,
        child: _buildDesktopSkeletonBar(width: 48, height: 48, radius: 8),
      ),
      _buildDesktopTableSizedCell(
        flex: 5,
        child: _buildDesktopFixedTextSkeleton(width: 146),
      ),
      _buildDesktopTableSizedCell(
        flex: 4,
        child: _buildDesktopFixedTextSkeleton(width: 98),
      ),
      _buildDesktopTableSizedCell(
        flex: 2,
        child: _buildDesktopFixedTextSkeleton(width: 64),
      ),
      _buildDesktopTableSizedCell(
        width: 72,
        child: Center(
          child: _buildDesktopSkeletonBar(width: 16, height: 16, radius: 8),
        ),
      ),
    ],
    isFirst: isFirst,
    showSelection: showSelection,
  );
}

Widget _buildDesktopTableRowArtistSkeleton({
  required bool isFirst,
  required bool showSelection,
}) {
  return _buildDesktopTableRowSkeletonBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        width: 44,
        child: _buildDesktopFixedTextSkeleton(width: 12),
      ),
      _buildDesktopTableSizedCell(
        width: 62,
        child: _buildDesktopSkeletonBar(width: 48, height: 48, radius: 8),
      ),
      _buildDesktopTableSizedCell(
        flex: 6,
        child: _buildDesktopFixedTextSkeleton(width: 154),
      ),
      _buildDesktopTableSizedCell(
        flex: 3,
        child: _buildDesktopFixedTextSkeleton(width: 82),
      ),
      _buildDesktopTableSizedCell(
        width: 72,
        child: Center(
          child: _buildDesktopSkeletonBar(width: 16, height: 16, radius: 8),
        ),
      ),
    ],
    isFirst: isFirst,
    showSelection: showSelection,
  );
}

Widget _buildDesktopTableRowUnknownSkeleton({
  required bool isFirst,
  required bool showSelection,
}) {
  return _buildDesktopTableRowSkeletonBase(
    cells: <Widget>[
      _buildDesktopTableSizedCell(
        flex: 1,
        child: _buildDesktopFixedTextSkeleton(width: 120),
      ),
    ],
    isFirst: isFirst,
    showSelection: showSelection,
  );
}

Widget _buildDesktopTableHeaderBase({
  required List<Widget> cells,
  required bool showSelection,
  required bool isAllSelected,
  required VoidCallback onToggleSelectAll,
  required VoidCallback? onMoreTap,
}) {
  return Builder(
    builder: (BuildContext context) {
      final colors = context.mashinColors;

      return Container(
        decoration: BoxDecoration(
          border: Border(
            top: BorderSide(color: colors.border),
            bottom: BorderSide(color: colors.border),
          ),
        ),
        padding: const EdgeInsets.symmetric(vertical: 5),
        child: Row(
          children: <Widget>[
            ...cells,
            SizedBox(
              width: 40,
              child: Center(
                child: Icon(
                  Icons.favorite_border_outlined,
                  size: 18,
                  color: colors.textSecondary,
                ),
              ),
            ),
            SizedBox(
              width: 40,
              child: Center(
                child: _DesktopIconActionButton(
                  icon: Icons.more_horiz_outlined,
                  onTap: onMoreTap,
                  baseColor: colors.textSecondary,
                  hoverColor: colors.textPrimary,
                ),
              ),
            ),
            if (showSelection)
              SizedBox(
                width: 40,
                child: Center(
                  child: _DesktopSelectionCheckBox(
                    isChecked: isAllSelected,
                    isVisible: true,
                    onTap: onToggleSelectAll,
                  ),
                ),
              ),
          ],
        ),
      );
    },
  );
}

class _DesktopTableRowBase extends StatefulWidget {
  final List<Widget> cells;
  final bool isFirst;
  final bool isSelected;
  final bool isFavorite;
  final bool selectionEnabled;
  final VoidCallback onToggleSelected;
  final VoidCallback? onFavoriteTap;
  final VoidCallback? onMoreTap;
  final ValueChanged<BuildContext>? onMoreAnchorTap;
  final VoidCallback? onDoubleTap;
  final ValueChanged<PointerDownEvent>? onPointerDown;

  const _DesktopTableRowBase({
    required this.cells,
    required this.isFirst,
    required this.isSelected,
    required this.isFavorite,
    required this.selectionEnabled,
    required this.onToggleSelected,
    this.onFavoriteTap,
    this.onMoreTap,
    this.onMoreAnchorTap,
    this.onDoubleTap,
    this.onPointerDown,
  });

  @override
  State<_DesktopTableRowBase> createState() => _DesktopTableRowBaseState();
}

class _DesktopTableRowBaseState extends State<_DesktopTableRowBase> {
  bool _isHovered = false;

  void _handleRowPointerDown(PointerDownEvent event) {
    if (widget.onPointerDown == null) {
      return;
    }

    // Right-clicks are forwarded from anywhere on the row so the context menu
    // can also open above the trailing action columns.
    final bool isSecondaryButton =
        event.kind == PointerDeviceKind.mouse &&
        (event.buttons & kSecondaryMouseButton) != 0;
    if (isSecondaryButton) {
      widget.onPointerDown!(event);
      return;
    }

    final RenderObject? renderObject = context.findRenderObject();
    if (renderObject is! RenderBox) {
      widget.onPointerDown!(event);
      return;
    }

    // Exclude favorite/more/checkbox action columns from row selection pointer handling.
    final double trailingActionWidth = widget.selectionEnabled ? 120 : 80;
    final double rowWidth = renderObject.size.width;
    if (rowWidth > 0 &&
        event.localPosition.dx >= rowWidth - trailingActionWidth) {
      return;
    }

    widget.onPointerDown!(event);
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final Color selectedBackground = colors.selection;
    final Color hoverBackground = colors.hover;

    Color rowBackground = Colors.transparent;
    if (widget.isSelected) {
      rowBackground = _isHovered ? hoverBackground : selectedBackground;
    } else if (_isHovered) {
      rowBackground = hoverBackground;
    }

    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: Listener(
        onPointerDown: _handleRowPointerDown,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onDoubleTap: widget.onDoubleTap,
          child: Container(
            height: 64,
            decoration: BoxDecoration(
              color: rowBackground,
              border: widget.isFirst
                  ? null
                  : Border(top: BorderSide(color: colors.border)),
            ),
            child: Row(
              children: <Widget>[
                Expanded(child: Row(children: widget.cells)),
                SizedBox(
                  width: 40,
                  child: Center(
                    child: _DesktopIconActionButton(
                      icon: widget.isFavorite
                          ? Icons.favorite_outlined
                          : Icons.favorite_border_outlined,
                      onTap: widget.onFavoriteTap,
                      baseColor: colors.textTertiary,
                      hoverColor: colors.textSecondary,
                    ),
                  ),
                ),
                SizedBox(
                  width: 40,
                  child: Center(
                    child: _DesktopIconActionButton(
                      icon: Icons.more_horiz_outlined,
                      onTap: widget.onMoreTap,
                      onAnchorTap: widget.onMoreAnchorTap,
                      baseColor: colors.textTertiary,
                      hoverColor: colors.textSecondary,
                    ),
                  ),
                ),
                if (widget.selectionEnabled)
                  SizedBox(
                    width: 40,
                    child: Center(
                      child: _DesktopSelectionCheckBox(
                        isChecked: widget.isSelected,
                        isVisible: _isHovered || widget.isSelected,
                        onTap: widget.onToggleSelected,
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

Widget _buildDesktopTableRowBase({
  required List<Widget> cells,
  required bool isFirst,
  required bool isSelected,
  required bool isFavorite,
  required bool selectionEnabled,
  required VoidCallback onToggleSelected,
  required VoidCallback? onFavoriteTap,
  required VoidCallback? onMoreTap,
  ValueChanged<BuildContext>? onMoreAnchorTap,
  required VoidCallback? onDoubleTap,
  required ValueChanged<PointerDownEvent>? onPointerDown,
}) {
  return _DesktopTableRowBase(
    cells: cells,
    isFirst: isFirst,
    isSelected: isSelected,
    isFavorite: isFavorite,
    selectionEnabled: selectionEnabled,
    onToggleSelected: onToggleSelected,
    onFavoriteTap: onFavoriteTap,
    onMoreTap: onMoreTap,
    onMoreAnchorTap: onMoreAnchorTap,
    onDoubleTap: onDoubleTap,
    onPointerDown: onPointerDown,
  );
}

Widget _buildDesktopTableRowSkeletonBase({
  required List<Widget> cells,
  required bool isFirst,
  required bool showSelection,
}) {
  return Builder(
    builder: (BuildContext context) {
      final colors = context.mashinColors;

      return Container(
        height: 64,
        decoration: BoxDecoration(
          color: Colors.transparent,
          border: isFirst
              ? null
              : Border(top: BorderSide(color: colors.border)),
        ),
        child: Row(
          children: <Widget>[
            Expanded(child: Row(children: cells)),
            SizedBox(
              width: 40,
              child: Center(
                child: _buildDesktopSkeletonBar(
                  width: 16,
                  height: 16,
                  radius: 8,
                ),
              ),
            ),
            SizedBox(
              width: 40,
              child: Center(
                child: _buildDesktopSkeletonBar(
                  width: 16,
                  height: 16,
                  radius: 8,
                ),
              ),
            ),
            if (showSelection)
              const SizedBox(
                width: 40,
                child: Center(
                  child: _DesktopSelectionCheckBox(
                    isChecked: false,
                    isVisible: false,
                  ),
                ),
              ),
          ],
        ),
      );
    },
  );
}

Widget _buildDesktopTableSizedCell({
  required Widget child,
  int? flex,
  double? width,
}) {
  if (width != null) {
    return SizedBox(width: width, child: child);
  }

  if (flex != null) {
    return Expanded(flex: flex, child: child);
  }

  return Expanded(child: child);
}

Widget _buildDesktopHeaderText(String text, TextAlign textAlign) {
  return Builder(
    builder: (BuildContext context) {
      final textStyles = context.mashinTableTextStyles;
      return Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8),
        child: Text(
          text,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          textAlign: textAlign,
          style: textStyles.header,
        ),
      );
    },
  );
}

Widget _buildDesktopBodyText(
  String text,
  TextAlign textAlign,
  DesktopTextTone tone,
) {
  return Builder(
    builder: (BuildContext context) {
      final textStyles = context.mashinTableTextStyles;
      final TextStyle base = switch (tone) {
        DesktopTextTone.primary => textStyles.bodyPrimary,
        DesktopTextTone.secondary => textStyles.bodySecondary,
        DesktopTextTone.tertiary => textStyles.bodyTertiary,
      };

      return Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8),
        child: Text(
          text,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          textAlign: textAlign,
          style: base.copyWith(fontSize: 13),
        ),
      );
    },
  );
}

// Colors the title accent when it matches the currently playing track.
Widget _buildDesktopTrackTitleCell({
  required PlaybackCoordinator? playbackCoordinator,
  required MediaItem? playbackItem,
  required String title,
}) {
  return PlaybackAwareBuilder(
    coordinator: playbackCoordinator,
    item: playbackItem,
    builder: (BuildContext context, bool isCurrent) {
      final textStyles = context.mashinTableTextStyles;
      final colors = context.mashinColors;

      return Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8),
        child: Text(
          title,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          textAlign: TextAlign.left,
          style: textStyles.bodyPrimary.copyWith(
            fontSize: 13,
            color: isCurrent ? colors.accent : null,
          ),
        ),
      );
    },
  );
}

Widget _buildDesktopArtistLinkCell({
  required List<Artist> artists,
  required String fallbackText,
  required ValueChanged<Artist>? onArtistTap,
  ValueChanged<int>? onArtistPointerDown,
}) {
  return Builder(
    builder: (BuildContext context) {
      final textStyles = context.mashinTableTextStyles;
      return Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8),
        child: MediaLinkText(
          items: artists,
          fallbackText: fallbackText,
          onItemTap: onArtistTap == null
              ? null
              : (MediaItem media) => onArtistTap(media as Artist),
          onItemPointerDown: onArtistPointerDown,
          style: textStyles.bodySecondary.copyWith(fontSize: 13),
        ),
      );
    },
  );
}

Widget _buildDesktopPlaylistLinkCell({
  required Playlist playlist,
  required ValueChanged<Playlist>? onPlaylistTap,
}) {
  return Builder(
    builder: (BuildContext context) {
      final textStyles = context.mashinTableTextStyles;
      return Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8),
        child: MediaLinkText(
          items: <MediaItem>[playlist],
          fallbackText: _prefer(playlist.name, 'Unknown Playlist'),
          onItemTap: onPlaylistTap == null
              ? null
              : (MediaItem media) => onPlaylistTap(media as Playlist),
          style: textStyles.bodyPrimary.copyWith(fontSize: 13),
        ),
      );
    },
  );
}

Widget _buildDesktopAlbumLinkCell({
  required Album? album,
  required ValueChanged<Album>? onAlbumTap,
}) {
  return Builder(
    builder: (BuildContext context) {
      final textStyles = context.mashinTableTextStyles;
      final String name = _prefer(album?.name ?? '', '-');
      return Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8),
        child: MediaLinkText(
          items: album == null ? const <MediaItem>[] : <MediaItem>[album],
          fallbackText: name,
          onItemTap: album == null || onAlbumTap == null
              ? null
              : (MediaItem media) => onAlbumTap(media as Album),
          style: textStyles.bodySecondary.copyWith(fontSize: 13),
        ),
      );
    },
  );
}

Widget _buildDesktopSkeletonBar({
  required double width,
  double height = 14,
  double radius = 4,
}) {
  return Builder(
    builder: (BuildContext context) {
      final colors = context.mashinColors;
      return Container(
        width: width,
        height: height,
        decoration: BoxDecoration(
          color: colors.bgTertiary,
          borderRadius: BorderRadius.circular(radius),
        ),
      );
    },
  );
}

Widget _buildDesktopFixedTextSkeleton({
  required double width,
  bool alignRight = false,
  double height = 12,
}) {
  return Padding(
    padding: const EdgeInsets.symmetric(horizontal: 8),
    child: Align(
      alignment: alignRight ? Alignment.centerRight : Alignment.centerLeft,
      child: _buildDesktopSkeletonBar(width: width, height: height),
    ),
  );
}

class _DesktopIconActionButton extends StatefulWidget {
  final IconData icon;
  final VoidCallback? onTap;
  final ValueChanged<BuildContext>? onAnchorTap;
  final Color baseColor;
  final Color hoverColor;

  const _DesktopIconActionButton({
    required this.icon,
    required this.onTap,
    this.onAnchorTap,
    required this.baseColor,
    required this.hoverColor,
  });

  @override
  State<_DesktopIconActionButton> createState() =>
      _DesktopIconActionButtonState();
}

class _DesktopIconActionButtonState extends State<_DesktopIconActionButton> {
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

class _DesktopSelectionCheckBox extends StatelessWidget {
  final bool isChecked;
  final bool isVisible;
  final VoidCallback? onTap;

  const _DesktopSelectionCheckBox({
    required this.isChecked,
    required this.isVisible,
    this.onTap,
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

    return IgnorePointer(
      ignoring: !isVisible,
      child: Listener(
        onPointerDown: onTap == null
            ? null
            : (PointerDownEvent event) {
                if (_isPrimaryActivationPointer(event)) {
                  onTap!.call();
                }
              },
        child: Opacity(
          opacity: isVisible ? 1 : 0,
          child: Container(
            width: 20,
            height: 20,
            decoration: BoxDecoration(
              color: isChecked ? colors.accent : Colors.transparent,
              borderRadius: BorderRadius.circular(4),
              border: Border.all(
                color: isChecked ? colors.accent : colors.textSecondary,
                width: 0.9,
              ),
            ),
            child: isChecked
                ? Icon(
                    Icons.check,
                    size: 14,
                    color: Theme.of(context).colorScheme.onPrimary,
                  )
                : null,
          ),
        ),
      ),
    );
  }
}

// Intents for selection actions
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

/// Reveals a newly appended expanded-table row by fading and sliding it in
/// from above, mirroring the row-section expand animation. Runs once on
/// insert; the optional [delay] staggers rows page by page.
class _ExpandRevealRow extends StatefulWidget {
  final Widget child;
  final Duration delay;

  const _ExpandRevealRow({
    super.key,
    required this.child,
    this.delay = Duration.zero,
  });

  @override
  State<_ExpandRevealRow> createState() => _ExpandRevealRowState();
}

class _ExpandRevealRowState extends State<_ExpandRevealRow> {
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

enum DesktopTextTone { primary, secondary, tertiary }

class DesktopPlaybackIndicator extends StatelessWidget {
  final PlaybackCoordinator? coordinator;
  final MediaItem? item;
  final String? fallback;
  final double width;

  const DesktopPlaybackIndicator({
    super.key,
    required this.coordinator,
    required this.item,
    required this.fallback,
    this.width = 40,
  });

  @override
  Widget build(BuildContext context) {
    final playbackCoordinator = coordinator;
    final mediaItem = item;
    if (playbackCoordinator == null || mediaItem == null) {
      return _fallback(context);
    }

    return ListenableBuilder(
      listenable: Listenable.merge(<Listenable>[
        playbackCoordinator.playbackStateChanges,
        playbackCoordinator.playbackQueueChanges,
      ]),
      builder: (BuildContext context, Widget? child) {
        final currentIndex = playbackCoordinator.currentQueueIndex;
        final queueItems = playbackCoordinator.queueItems;
        final currentItem =
            currentIndex != null &&
                currentIndex >= 0 &&
                currentIndex < queueItems.length
            ? queueItems[currentIndex].mediaItem
            : null;

        if (currentItem == null ||
            !isSamePlaybackTrack(mediaItem, currentItem)) {
          return _fallback(context);
        }

        final playbackStateType =
            playbackCoordinator.playbackState.playbackState;
        if (playbackStateType == PlaybackStateType.buffering ||
            playbackStateType == PlaybackStateType.seeking) {
          return SizedBox(
            width: width,
            height: 20,
            child: Center(
              child: SizedBox(
                width: 16,
                height: 16,
                child: CircularProgressIndicator(
                  strokeWidth: 2,
                  color: context.mashinColors.accent,
                ),
              ),
            ),
          );
        }

        return SizedBox(
          width: width,
          height: 20,
          child: Center(
            child: Icon(
              playbackStateType == PlaybackStateType.playing
                  ? Icons.pause
                  : Icons.play_arrow,
              size: 16,
              color: context.mashinColors.accent,
            ),
          ),
        );
      },
    );
  }

  Widget _fallback(BuildContext context) {
    final fallbackText = fallback;
    if (fallbackText == null) {
      return const SizedBox.shrink();
    }

    return SizedBox(
      width: width,
      height: 20,
      child: Center(
        child: _buildDesktopBodyText(
          fallbackText,
          TextAlign.center,
          DesktopTextTone.tertiary,
        ),
      ),
    );
  }
}

/// Compares two media items by uri (or item id as fallback) to determine if
/// they refer to the same playback track.
bool isSamePlaybackTrack(MediaItem left, MediaItem right) {
  final String leftUri = left.uri?.trim() ?? '';
  final String rightUri = right.uri?.trim() ?? '';
  if (leftUri.isNotEmpty && rightUri.isNotEmpty) {
    return leftUri.toLowerCase() == rightUri.toLowerCase();
  }

  final String leftItemId = left.itemId.trim();
  final String rightItemId = right.itemId.trim();
  return leftItemId.isNotEmpty &&
      rightItemId.isNotEmpty &&
      leftItemId == rightItemId;
}

/// Rebuilds [builder] with whether [item] is the currently active playback
/// track, so callers can style it (e.g. accent-colored title) without
/// showing a separate indicator icon.
class PlaybackAwareBuilder extends StatelessWidget {
  final PlaybackCoordinator? coordinator;
  final MediaItem? item;
  final Widget Function(BuildContext context, bool isCurrent) builder;

  const PlaybackAwareBuilder({
    super.key,
    required this.coordinator,
    required this.item,
    required this.builder,
  });

  @override
  Widget build(BuildContext context) {
    final PlaybackCoordinator? playbackCoordinator = coordinator;
    final MediaItem? mediaItem = item;
    if (playbackCoordinator == null || mediaItem == null) {
      return builder(context, false);
    }

    return ListenableBuilder(
      listenable: Listenable.merge(<Listenable>[
        playbackCoordinator.playbackStateChanges,
        playbackCoordinator.playbackQueueChanges,
      ]),
      builder: (BuildContext context, Widget? child) {
        final int? currentIndex = playbackCoordinator.currentQueueIndex;
        final List<QueueItem> queueItems = playbackCoordinator.queueItems;
        final MediaItem? currentItem =
            currentIndex != null &&
                currentIndex >= 0 &&
                currentIndex < queueItems.length
            ? queueItems[currentIndex].mediaItem
            : null;
        final bool isCurrent =
            currentItem != null && isSamePlaybackTrack(mediaItem, currentItem);
        return builder(context, isCurrent);
      },
    );
  }
}

class MediaTableCover extends StatefulWidget {
  final String? coverUrl;
  final IconData fallbackIcon;
  final double size;
  final double radius;
  final VoidCallback? onPlayTap;

  const MediaTableCover({
    super.key,
    required this.coverUrl,
    required this.fallbackIcon,
    this.size = 48,
    this.radius = 8,
    this.onPlayTap,
  });

  @override
  State<MediaTableCover> createState() => _MediaTableCoverState();
}

class _MediaTableCoverState extends State<MediaTableCover> {
  static const Duration _overlayFadeDuration = Duration(milliseconds: 120);
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
    final String? trimmedCover = widget.coverUrl?.trim();
    final bool hasCover = trimmedCover != null && trimmedCover.isNotEmpty;
    final bool showPlayOverlay = widget.onPlayTap != null;

    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: ClipRRect(
        borderRadius: BorderRadius.circular(widget.radius),
        child: SizedBox(
          width: widget.size,
          height: widget.size,
          child: Stack(
            fit: StackFit.expand,
            children: <Widget>[
              Container(
                color: colors.bgTertiary,
                child: hasCover
                    ? MediaImage(
                        source: trimmedCover,
                        fit: BoxFit.cover,
                        fallback: Icon(
                          widget.fallbackIcon,
                          color: colors.accent,
                          size: widget.size * 0.5,
                        ),
                      )
                    : Icon(
                        widget.fallbackIcon,
                        color: colors.accent,
                        size: widget.size * 0.5,
                      ),
              ),
              if (showPlayOverlay)
                IgnorePointer(
                  child: AnimatedOpacity(
                    duration: _overlayFadeDuration,
                    curve: Curves.easeOut,
                    opacity: _isHovered ? 1 : 0,
                    child: Container(
                      color: Colors.black.withValues(
                        alpha: colors.coverHoverOverlayOpacity,
                      ),
                    ),
                  ),
                ),
              if (showPlayOverlay)
                Positioned.fill(
                  child: Center(
                    child: AnimatedOpacity(
                      duration: _overlayFadeDuration,
                      curve: Curves.easeOut,
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
                              width: 35,
                              height: 35,
                              decoration: BoxDecoration(
                                color: colors.overlayPlayButtonBackground,
                                borderRadius: BorderRadius.circular(18),
                              ),
                              child: Icon(
                                Icons.play_arrow_sharp,
                                size: 24,
                                color: colors.overlayPlayButtonForeground,
                              ),
                            ),
                          ),
                        ),
                      ),
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

// Section header badge showing item count, matching the MAUI Album/Playlist detail header pill.
class SectionCountBadge extends StatelessWidget {
  final int count;

  const SectionCountBadge({super.key, required this.count});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Container(
      height: 22,
      padding: const EdgeInsets.symmetric(horizontal: 6),
      alignment: Alignment.center,
      decoration: BoxDecoration(
        border: Border.all(color: colors.border),
        borderRadius: BorderRadius.circular(3),
      ),
      child: Text(
        '$count',
        style: TextStyle(
          fontWeight: FontWeight.bold,
          fontSize: 10,
          color: colors.textPrimary,
        ),
      ),
    );
  }
}

// Horizontal separator filling the space between the title/badge and the header actions.
class SectionHeaderDivider extends StatelessWidget {
  const SectionHeaderDivider({super.key});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return Container(height: 1, color: colors.border.withValues(alpha: 0.75));
  }
}

// Vertical separator between header actions and pagination controls.
class SectionHeaderVerticalDivider extends StatelessWidget {
  const SectionHeaderVerticalDivider({super.key});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return Container(
      width: 1,
      height: 18,
      margin: const EdgeInsets.symmetric(horizontal: 4),
      color: colors.border.withValues(alpha: 0.75),
    );
  }
}

class SectionHeaderIconButton extends StatefulWidget {
  final IconData icon;
  final VoidCallback? onTap;
  final String? tooltip;
  final double iconSize;

  const SectionHeaderIconButton({
    super.key,
    required this.icon,
    required this.onTap,
    this.tooltip,
    this.iconSize = 18,
  });

  @override
  State<SectionHeaderIconButton> createState() =>
      _SectionHeaderIconButtonState();
}

class _SectionHeaderIconButtonState extends State<SectionHeaderIconButton> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool isDisabled = widget.onTap == null;

    final Widget button = MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: GestureDetector(
        onTap: widget.onTap,
        behavior: HitTestBehavior.opaque,
        child: Opacity(
          opacity: isDisabled ? 0.4 : 1,
          child: Container(
            width: 28,
            height: 28,
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: _isHovered && !isDisabled
                  ? colors.hover
                  : Colors.transparent,
              borderRadius: BorderRadius.circular(14),
            ),
            child: Icon(
              widget.icon,
              size: widget.iconSize,
              color: colors.textSecondary,
            ),
          ),
        ),
      ),
    );

    if (widget.tooltip == null || widget.tooltip!.isEmpty) {
      return button;
    }
    return Tooltip(message: widget.tooltip!, child: button);
  }
}

class MediaProviderIcon extends StatelessWidget {
  final String providerLabel;
  final String? iconSvg;
  final double size;

  const MediaProviderIcon({
    super.key,
    required this.providerLabel,
    required this.iconSvg,
    this.size = 16,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final String? trimmed = iconSvg?.trim();
    if (trimmed == null || trimmed.isEmpty) {
      return Icon(
        Icons.radio_button_checked_outlined,
        size: size,
        color: colors.textSecondary,
      );
    }

    // Provider manifests may deliver raw XML, XML with declaration, or URL-encoded XML.
    final String decoded = _maybeDecodeSvgPayload(trimmed);
    final bool isInlineSvg =
        decoded.startsWith('<svg') ||
        decoded.startsWith('<?xml') ||
        decoded.contains('<svg');

    if (isInlineSvg) {
      return SvgPicture.string(decoded, width: size, height: size);
    }

    final Uri? uri = Uri.tryParse(trimmed);
    final bool isNetworkSvg =
        uri != null && (uri.scheme == 'http' || uri.scheme == 'https');
    if (!isNetworkSvg) {
      return Icon(
        Icons.radio_button_checked_outlined,
        size: size,
        color: colors.textSecondary,
      );
    }

    return SvgPicture.network(
      trimmed,
      width: size,
      height: size,
      placeholderBuilder: (_) => Icon(
        Icons.radio_button_checked_outlined,
        size: size,
        color: colors.textSecondary,
      ),
    );
  }
}

String _maybeDecodeSvgPayload(String raw) {
  if (!raw.contains('%')) {
    return raw;
  }

  try {
    return Uri.decodeFull(raw);
  } catch (_) {
    return raw;
  }
}

// Helper Functions
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

String _playlistSubtitle(Playlist playlist) {
  final owner = _stripOrEmpty(playlist.owner);
  if (owner.isNotEmpty) {
    return owner;
  }
  return 'Playlist';
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

  final Duration duration = Duration(seconds: seconds);
  final int minutes = duration.inMinutes;
  final int remainingSeconds = duration.inSeconds % 60;
  return '$minutes:${remainingSeconds.toString().padLeft(2, '0')}';
}

String _formatQueueDurationSeconds(double seconds) {
  if (!seconds.isFinite || seconds <= 0) {
    return '0:00';
  }

  final int totalSeconds = seconds.round();
  final int minutes = totalSeconds ~/ 60;
  final int remainingSeconds = totalSeconds % 60;
  return '$minutes:${remainingSeconds.toString().padLeft(2, '0')}';
}

String _resolveQueueProvider(QueueItem item) {
  final String directProvider = _stripOrEmpty(item.mediaItem?.provider);
  if (directProvider.isNotEmpty) {
    return directProvider;
  }

  final List<ProviderMapping> mappings =
      item.mediaItem?.providerMappings ?? const <ProviderMapping>[];
  for (final ProviderMapping mapping in mappings) {
    final String domain = _stripOrEmpty(mapping.providerDomain);
    if (domain.isNotEmpty) {
      return domain;
    }

    final String instance = _stripOrEmpty(mapping.providerInstance);
    if (instance.isNotEmpty) {
      return instance;
    }
  }

  return '';
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
