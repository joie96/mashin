import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../../app/theme/app_theme.dart';
import '../../core/core_bridge.dart';
import '../../core/core_events.dart';
import '../../core/core_models.dart';
import '../selection/selection_controller.dart';
import 'detail_provider_switcher.dart';
import 'media_link_text.dart';
import 'media_table_section_desktop.dart';

class MediaListSectionDesktop<T extends MediaItem> extends StatefulWidget {
  final String title;
  final List<T> items;
  final bool isLoading;
  final String emptyText;
  final MashinCoreBridgeApi? coreBridge;
  final MashinCoreBridgeEvents? coreEvents;
  final ValueChanged<T>? onItemTap;
  final ValueChanged<Album>? onAlbumTap;
  final DesktopRowContextMenuAction<T>? onItemContextMenu;
  final DesktopRowMoreAnchorAction<T>? onItemMoreAnchorTap;
  final bool enableSelection;
  final ValueChanged<List<T>>? onSelectionChanged;
  final VoidCallback? onSelectionMenuTap;
  final DesktopRowSelectionContextMenuAction<T>? onItemSelectionContextMenu;

  const MediaListSectionDesktop({
    super.key,
    required this.title,
    required this.items,
    required this.isLoading,
    required this.emptyText,
    this.coreBridge,
    this.coreEvents,
    this.onItemTap,
    this.onAlbumTap,
    this.onItemContextMenu,
    this.onItemMoreAnchorTap,
    this.enableSelection = true,
    this.onSelectionChanged,
    this.onSelectionMenuTap,
    this.onItemSelectionContextMenu,
  });

  @override
  State<MediaListSectionDesktop<T>> createState() =>
      _MediaListSectionDesktopState<T>();
}

class _MediaListSectionDesktopState<T extends MediaItem>
    extends State<MediaListSectionDesktop<T>> {
  static const _pageTransitionDuration = Duration(milliseconds: 250);
  static const _pageTransitionCurve = Curves.easeInOutCubic;
  static const int _rowsPerColumn = 3;
  static const double _columnGap = 12;
  int _pageStartIndex = 0;
  bool _isPageTransitionRunning = false;
  int _navigationDirection = 1;
  bool _isExpanded = false;
  int _expandedItemCount = 0;
  int _expandedBaseCount = 0;
  int _expandLoadRunId = 0;
  int _lastVisibleCount = _rowsPerColumn;
  int _transitionRunId = 0;
  Duration _pageHeightAnimationDuration = Duration.zero;
  final FocusNode _sectionFocusNode = FocusNode(
    debugLabel: 'MediaListDesktopSectionFocus',
  );
  final Set<int> _selectedIndexes = <int>{};
  final Object _selectionSessionOwner = Object();
  final SelectionController _selectionSessionController =
      SelectionController.instance;

  // Clamp selections when a refreshed collection removes the current page.
  @override
  void didUpdateWidget(covariant MediaListSectionDesktop<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    _pageStartIndex = _pageStartIndex.clamp(
      0,
      math.max(0, widget.items.length - 1),
    );
    if (widget.items.length <= _lastVisibleCount) {
      _isExpanded = false;
      _expandLoadRunId++;
    }
    _expandedItemCount = _expandedItemCount.clamp(0, widget.items.length);
    _expandedBaseCount = _expandedBaseCount.clamp(0, widget.items.length);
    if (_isExpanded && oldWidget.items.length != widget.items.length) {
      _expandLoadRunId++;
      unawaited(_loadExpandedItemsIncrementally());
    }
    _pageHeightAnimationDuration = Duration.zero;
    if (!identical(oldWidget.items, widget.items)) {
      final selectedIdentities = _selectedIndexes
          .where((index) => index < oldWidget.items.length)
          .map(
            (index) => (
              oldWidget.items[index].provider,
              oldWidget.items[index].itemId,
            ),
          )
          .toSet();
      _selectedIndexes
        ..clear()
        ..addAll(
          Iterable<int>.generate(widget.items.length).where(
            (index) => selectedIdentities.contains((
              widget.items[index].provider,
              widget.items[index].itemId,
            )),
          ),
        );
    }
    if (!widget.enableSelection || !_isTrackSection) _selectedIndexes.clear();
    _syncSelectionSession();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _emitSelectionChanged();
    });
  }

  @override
  void dispose() {
    _sectionFocusNode.dispose();
    _selectionSessionController.deactivate(_selectionSessionOwner);
    super.dispose();
  }

  // Track selection commands share the desktop selection overlay.
  void _onTrackTap(int index) {
    if (!widget.enableSelection) {
      widget.onItemTap?.call(widget.items[index]);
      return;
    }
    _sectionFocusNode.requestFocus();
    final keys = HardwareKeyboard.instance.logicalKeysPressed;
    final isCtrlPressed =
        keys.contains(LogicalKeyboardKey.controlLeft) ||
        keys.contains(LogicalKeyboardKey.controlRight);
    if (isCtrlPressed) {
      _toggleSelectedIndex(index);
    } else if (_selectedIndexes.length == 1 &&
        _selectedIndexes.contains(index)) {
      _clearSelectionFromController();
    } else {
      _selectSingleIndex(index);
    }
  }

  void _selectSingleIndex(int index) {
    setState(() {
      _selectedIndexes
        ..clear()
        ..add(index);
    });
    _syncSelectionSession();
    _emitSelectionChanged();
  }

  void _onTrackTitleTap(int index, Album album) {
    final keys = HardwareKeyboard.instance.logicalKeysPressed;
    final isCtrlPressed =
        keys.contains(LogicalKeyboardKey.controlLeft) ||
        keys.contains(LogicalKeyboardKey.controlRight);
    if (widget.enableSelection && isCtrlPressed) {
      _onTrackTap(index);
    } else {
      widget.onAlbumTap?.call(album);
    }
  }

  void _toggleSelectedIndex(int index) {
    _sectionFocusNode.requestFocus();
    setState(() {
      if (!_selectedIndexes.remove(index)) _selectedIndexes.add(index);
    });
    _syncSelectionSession();
    _emitSelectionChanged();
  }

  void _toggleAllSelectedIndexes() {
    setState(() {
      if (_isAllSelected) {
        _selectedIndexes.clear();
      } else {
        _selectedIndexes
          ..clear()
          ..addAll(List<int>.generate(widget.items.length, (index) => index));
      }
    });
    _syncSelectionSession();
    _emitSelectionChanged();
  }

  void _clearSelectionFromController() {
    if (!mounted) return;
    setState(_selectedIndexes.clear);
    _selectionSessionController.deactivate(_selectionSessionOwner);
    _emitSelectionChanged();
  }

  void _showTrackContextMenu(int index, Offset position) {
    if (widget.enableSelection && !_selectedIndexes.contains(index)) {
      _sectionFocusNode.requestFocus();
      _selectSingleIndex(index);
    }
    final selectionHandler = widget.onItemSelectionContextMenu;
    if (selectionHandler != null && _selectedIndexes.isNotEmpty) {
      selectionHandler(_selectedItems(), position);
    } else {
      widget.onItemContextMenu?.call(widget.items[index], index, position);
    }
  }

  // Paging commands.
  void _goToPreviousPage(int visibleCount) {
    if (_isExpanded) return;
    final startIndex = _clampStartIndex(visibleCount: visibleCount);
    if (startIndex <= 0) return;
    unawaited(
      _startPageTransition(
        targetStartIndex: startIndex - visibleCount,
        direction: -1,
      ),
    );
  }

  void _goToNextPage(int visibleCount, int pageCount) {
    if (_isExpanded) return;
    final startIndex = _clampStartIndex(visibleCount: visibleCount);
    if (startIndex >= (pageCount - 1) * visibleCount) return;
    unawaited(
      _startPageTransition(
        targetStartIndex: startIndex + visibleCount,
        direction: 1,
      ),
    );
  }

  Future<void> _startPageTransition({
    required int targetStartIndex,
    required int direction,
  }) async {
    if (_isExpanded ||
        targetStartIndex < 0 ||
        targetStartIndex >= widget.items.length ||
        targetStartIndex == _pageStartIndex) {
      return;
    }
    final runId = ++_transitionRunId;
    setState(() {
      _navigationDirection = direction;
      _pageStartIndex = targetStartIndex;
      _isPageTransitionRunning = true;
    });
    await Future<void>.delayed(_pageTransitionDuration);
    if (!mounted || runId != _transitionRunId) return;
    setState(() => _isPageTransitionRunning = false);
  }

  // Expansion resets paging and cancels any in-flight page transition.
  void _toggleExpanded() {
    if (widget.isLoading || widget.items.length <= _lastVisibleCount) return;
    setState(() {
      _isExpanded = !_isExpanded;
      _expandLoadRunId++;
      if (_isExpanded) {
        _expandedItemCount = math.min(
          _pageStartIndex + _lastVisibleCount,
          widget.items.length,
        );
        _expandedBaseCount = _expandedItemCount;
      }
      _pageStartIndex = 0;
      _isPageTransitionRunning = false;
      _transitionRunId++;
      _pageHeightAnimationDuration = const Duration(milliseconds: 380);
    });
    if (_isExpanded) unawaited(_loadExpandedItemsIncrementally());
  }

  Future<void> _loadExpandedItemsIncrementally() async {
    final runId = _expandLoadRunId;
    while (mounted && _isExpanded && runId == _expandLoadRunId) {
      if (_expandedItemCount >= widget.items.length) return;
      await Future<void>.delayed(const Duration(milliseconds: 220));
      if (!mounted || !_isExpanded || runId != _expandLoadRunId) return;
      setState(() {
        _expandedItemCount = math.min(
          _expandedItemCount + _lastVisibleCount,
          widget.items.length,
        );
        _pageHeightAnimationDuration = const Duration(milliseconds: 380);
      });
    }
  }

  // Playback commands.
  void _onPlayAllTap() {
    final MashinCoreBridgeApi? bridge = widget.coreBridge;
    final List<MediaItem> playItems = _playableItems();
    if (bridge == null || playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await bridge.queuePlayMedia(
          _isTrackSection ? playItems : <MediaItem>[playItems.first],
        );
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

  // Playback errors stay local so paging and selection remain usable.
  Future<void> _playTracksFromIndex(int index) async {
    try {
      await widget.coreBridge?.queuePlayMedia(
        widget.items
            .skip(index)
            .where(_isValidPlayable)
            .toList(growable: false),
      );
    } catch (_) {}
  }

  // Playback eligibility and page layout.
  bool get _isTrackSection => T == Track;
  bool get _isAllSelected =>
      widget.items.isNotEmpty && _selectedIndexes.length == widget.items.length;

  List<T> _selectedItems() => (_selectedIndexes.toList()..sort())
      .map((index) => widget.items[index])
      .toList(growable: false);

  double get _rowExtent => _isTrackSection ? 64 : 48;

  List<MediaItem> _playableItems() {
    return widget.items.where(_isValidPlayable).toList(growable: false);
  }

  int _resolveColumnCount(double width) {
    if (!width.isFinite || width <= 0) {
      return 1;
    }

    final minimumWidth = _isTrackSection ? 340.0 : 220.0;
    final int count = ((width + _columnGap) / (minimumWidth + _columnGap))
        .floor();
    return count.clamp(1, 6);
  }

  int _clampStartIndex({required int visibleCount}) {
    final pageCount = math.max(1, (widget.items.length / visibleCount).ceil());
    return (_pageStartIndex ~/ visibleCount).clamp(0, pageCount - 1) *
        visibleCount;
  }

  double _contentHeight({required int columnCount}) {
    final pageSize = columnCount * _rowsPerColumn;
    final itemCount = math.min(_expandedItemCount, widget.items.length);
    final rowCount = _isExpanded && widget.items.length > pageSize
        ? (itemCount ~/ pageSize) * _rowsPerColumn +
              math.min(_rowsPerColumn, itemCount % pageSize)
        : _rowsPerColumn;
    return rowCount * _rowExtent + (rowCount - 1) * _columnGap;
  }

  // Section and row rendering.
  @override
  Widget build(BuildContext context) {
    final bool hasTitle = widget.title.trim().isNotEmpty;

    return Shortcuts(
      shortcuts: const <ShortcutActivator, Intent>{
        SingleActivator(LogicalKeyboardKey.escape): _ClearSelectionIntent(),
        SingleActivator(LogicalKeyboardKey.keyA, control: true):
            _SelectAllIntent(),
      },
      child: Actions(
        actions: <Type, Action<Intent>>{
          _ClearSelectionIntent: CallbackAction<_ClearSelectionIntent>(
            onInvoke: (_) {
              _clearSelectionFromController();
              return null;
            },
          ),
          _SelectAllIntent: CallbackAction<_SelectAllIntent>(
            onInvoke: (_) {
              if (widget.enableSelection && _isTrackSection) {
                _toggleAllSelectedIndexes();
              }
              return null;
            },
          ),
        },
        child: Focus(
          focusNode: _sectionFocusNode,
          child: LayoutBuilder(
            builder: (context, constraints) {
              final columnCount = _resolveColumnCount(constraints.maxWidth);
              final pageSize = columnCount * _rowsPerColumn;
              _lastVisibleCount = pageSize;
              final pageCount = math.max(
                1,
                (widget.items.length / pageSize).ceil(),
              );
              final canToggleExpand = !widget.isLoading && pageCount > 1;
              return Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  if (hasTitle)
                    _buildHeader(
                      context,
                      visibleCount: pageSize,
                      pageCount: pageCount,
                    ),
                  if (hasTitle) const SizedBox(height: 10),
                  _buildContent(context, columnCount: columnCount),
                  Padding(
                    padding: const EdgeInsets.only(top: 8),
                    child: SizedBox(
                      height: 24,
                      child: canToggleExpand
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
          ),
        ),
      ),
    );
  }

  Widget _buildHeader(
    BuildContext context, {
    required int visibleCount,
    required int pageCount,
  }) {
    final hasPlayableContent = !widget.isLoading && _playableItems().isNotEmpty;
    return Row(
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
        if (widget.coreBridge != null) ...<Widget>[
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
        if (pageCount > 1 && !widget.isLoading && !_isExpanded) ...<Widget>[
          if (widget.coreBridge != null) ...<Widget>[
            const SizedBox(width: 4),
            const SectionHeaderVerticalDivider(),
          ],
          _buildPaginationControls(
            context,
            visibleCount: visibleCount,
            pageCount: pageCount,
          ),
        ],
      ],
    );
  }

  Widget _buildContent(BuildContext context, {required int columnCount}) {
    if (widget.isLoading) {
      return _buildTypedListSkeleton(columnCount: columnCount);
    }
    if (widget.items.isEmpty) {
      return Text(
        widget.emptyText,
        style: context.mashinTableTextStyles.emptyState,
      );
    }
    return AnimatedContainer(
      alignment: Alignment.topCenter,
      height: _contentHeight(columnCount: columnCount),
      duration: _pageHeightAnimationDuration,
      curve: Curves.easeInOutCubic,
      onEnd: _onPageHeightAnimationEnd,
      clipBehavior: Clip.hardEdge,
      decoration: const BoxDecoration(),
      child: OverflowBox(
        alignment: Alignment.topCenter,
        minHeight: 0,
        maxHeight: double.infinity,
        child: _isExpanded && widget.items.length > columnCount * _rowsPerColumn
            ? _buildExpandedList(columnCount: columnCount)
            : IgnorePointer(
                ignoring: _isPageTransitionRunning,
                child: AnimatedSwitcher(
                  duration: _pageTransitionDuration,
                  switchInCurve: _pageTransitionCurve,
                  switchOutCurve: _pageTransitionCurve,
                  transitionBuilder: (child, animation) {
                    final isIncoming =
                        child.key == ValueKey<int>(_pageStartIndex);
                    final beginOffset = isIncoming
                        ? _navigationDirection.toDouble()
                        : -_navigationDirection.toDouble();
                    return FadeTransition(
                      opacity: animation,
                      child: SlideTransition(
                        position: Tween<Offset>(
                          begin: Offset(beginOffset * 0.08, 0),
                          end: Offset.zero,
                        ).animate(animation),
                        child: child,
                      ),
                    );
                  },
                  child: KeyedSubtree(
                    key: ValueKey<int>(_pageStartIndex),
                    child: _buildPage(columnCount: columnCount),
                  ),
                ),
              ),
      ),
    );
  }

  Widget _buildPage({required int columnCount, int? startIndex}) {
    final pageSize = columnCount * _rowsPerColumn;
    final pageStart = startIndex ?? _clampStartIndex(visibleCount: pageSize);
    final itemCount = startIndex == null
        ? widget.items.length
        : math.min(_expandedItemCount, widget.items.length);
    final visibleCount = math.min(pageSize, itemCount - pageStart);
    final rowCount = startIndex == null
        ? _rowsPerColumn
        : math.min(_rowsPerColumn, visibleCount);
    return SizedBox(
      height: rowCount * _rowExtent + (rowCount - 1) * _columnGap,
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (var column = 0; column < columnCount; column++) ...[
            if (column > 0) const SizedBox(width: _columnGap),
            Expanded(
              child: Column(
                children: [
                  for (var row = 0; row < _rowsPerColumn; row++)
                    if (column * _rowsPerColumn + row < visibleCount) ...[
                      if (row > 0) const SizedBox(height: _columnGap),
                      SizedBox(
                        height: _rowExtent,
                        child: startIndex == null
                            ? _buildTypedListTile(
                                pageStart + column * _rowsPerColumn + row,
                              )
                            : _buildExpandedListItem(
                                pageStart + column * _rowsPerColumn + row,
                                pageSize: pageSize,
                              ),
                      ),
                    ],
                ],
              ),
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildExpandedList({required int columnCount}) {
    final pageSize = columnCount * _rowsPerColumn;
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        for (
          var startIndex = 0;
          startIndex < math.min(_expandedItemCount, widget.items.length);
          startIndex += pageSize
        ) ...[
          if (startIndex > 0) const SizedBox(height: _columnGap),
          _buildPage(columnCount: columnCount, startIndex: startIndex),
        ],
      ],
    );
  }

  // Newly appended visual rows reveal from above; the initial page stays static.
  Widget _buildExpandedListItem(int index, {required int pageSize}) {
    final child = _buildTypedListTile(index);
    if (index < _expandedBaseCount) return child;
    final relativeIndex = index - _expandedBaseCount;
    final rowIndex =
        (relativeIndex ~/ pageSize) * _rowsPerColumn +
        relativeIndex % _rowsPerColumn;
    return _ExpandRevealItem(
      key: ValueKey(('expand-reveal', index)),
      delay: Duration(milliseconds: 70 * rowIndex),
      child: child,
    );
  }

  // Typed list rendering uses the section type for both rows and skeletons.
  Widget _buildTypedListTile(int index) {
    if (_isTrackSection) return _buildTrackListTile(index);
    return _buildMediaListTile(index);
  }

  Widget _buildTypedListSkeleton({required int columnCount}) =>
      _DesktopListSkeletonGrid(columnCount: columnCount, rowExtent: _rowExtent);

  // Track rows keep absolute source indexes for playback and context commands.
  Widget _buildTrackListTile(int index) {
    final item = widget.items[index];
    final track = item as Track;
    final album = track.album;
    final hasAlbumLink =
        widget.onAlbumTap != null &&
        album != null &&
        album.itemId.trim().isNotEmpty &&
        album.provider.trim().isNotEmpty;
    return _DesktopListTrackTile(
      key: ValueKey((item.provider, item.itemId, index)),
      track: track,
      coreEvents: widget.coreEvents,
      onTitleTap: hasAlbumLink ? () => _onTrackTitleTap(index, album) : null,
      isSelected: _selectedIndexes.contains(index),
      selectionModeActive:
          widget.enableSelection && _selectedIndexes.isNotEmpty,
      onToggleSelected: () => _toggleSelectedIndex(index),
      onTap: () => _onTrackTap(index),
      onPlayTap: widget.coreBridge == null
          ? null
          : () => unawaited(_playTracksFromIndex(index)),
      onMoreTap: widget.onItemMoreAnchorTap != null
          ? (anchor) => widget.onItemMoreAnchorTap!(item, index, anchor)
          : widget.onItemContextMenu == null
          ? null
          : (anchor) {
              final box = anchor.findRenderObject() as RenderBox;
              widget.onItemContextMenu!(
                item,
                index,
                box.localToGlobal(box.size.center(Offset.zero)),
              );
            },
      onContextMenu: (position) => _showTrackContextMenu(index, position),
    );
  }

  Widget _buildMediaListTile(int index) {
    final item = widget.items[index];
    return _MediaListTile<T>(
      key: ValueKey((item.provider, item.itemId, index)),
      item: item,
      coreBridge: widget.coreBridge,
      onItemTap: widget.onItemTap,
      onContextMenu: widget.onItemContextMenu == null
          ? null
          : (position) => widget.onItemContextMenu!(item, index, position),
    );
  }

  Widget _buildPaginationControls(
    BuildContext context, {
    required int visibleCount,
    required int pageCount,
  }) {
    final startIndex = _clampStartIndex(visibleCount: visibleCount);
    final canGoBack = startIndex > 0;
    final canGoNext = startIndex < (pageCount - 1) * visibleCount;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: <Widget>[
        IconButton(
          tooltip: 'Vorherige Seite',
          visualDensity: VisualDensity.compact,
          onPressed: canGoBack ? () => _goToPreviousPage(visibleCount) : null,
          icon: const Icon(Icons.chevron_left_outlined),
        ),
        IconButton(
          tooltip: 'Nächste Seite',
          visualDensity: VisualDensity.compact,
          onPressed: canGoNext
              ? () => _goToNextPage(visibleCount, pageCount)
              : null,
          icon: const Icon(Icons.chevron_right_outlined),
        ),
      ],
    );
  }

  // Unrelated layout changes resize immediately after explicit expansion ends.
  void _onPageHeightAnimationEnd() {
    if (_pageHeightAnimationDuration != Duration.zero) {
      setState(() => _pageHeightAnimationDuration = Duration.zero);
    }
  }

  // Selection notifications keep the owning screen and overlay synchronized.
  void _emitSelectionChanged() =>
      widget.onSelectionChanged?.call(_selectedItems());

  void _syncSelectionSession() {
    if (_selectedIndexes.isEmpty) {
      _selectionSessionController.deactivate(_selectionSessionOwner);
      return;
    }
    _selectionSessionController.activate(
      owner: _selectionSessionOwner,
      registration: SelectionSessionRegistration(
        allSelected: _isAllSelected,
        selectedCount: _selectedIndexes.length,
        itemTypeLabel: selectionTypeLabel(T, _selectedIndexes.length),
        onToggleAll: _toggleAllSelectedIndexes,
        onMenuTap: widget.onSelectionMenuTap,
        onClose: _clearSelectionFromController,
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

class _ExpandToggleButton extends StatefulWidget {
  const _ExpandToggleButton({required this.isExpanded, required this.onTap});

  final bool isExpanded;
  final VoidCallback onTap;

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
            splashColor: Colors.transparent,
            highlightColor: Colors.transparent,
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

class _ExpandRevealItem extends StatefulWidget {
  const _ExpandRevealItem({
    super.key,
    required this.child,
    required this.delay,
  });
  final Widget child;
  final Duration delay;

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

  @override
  void dispose() {
    _delayTimer?.cancel();
    super.dispose();
  }

  void _startReveal() {
    if (!mounted || _revealed) return;
    setState(() => _revealed = true);
  }

  @override
  Widget build(BuildContext context) => TweenAnimationBuilder<double>(
    tween: Tween<double>(begin: 0, end: _revealed ? 1 : 0),
    duration: const Duration(milliseconds: 400),
    curve: Curves.easeInOutCubic,
    builder: (context, value, child) => IgnorePointer(
      ignoring: value < 1,
      child: Opacity(
        opacity: value,
        child: Transform.translate(
          offset: Offset(0, -50 * (1 - value)),
          child: child,
        ),
      ),
    ),
    child: widget.child,
  );
}

class _DesktopListTrackTile extends StatelessWidget {
  const _DesktopListTrackTile({
    super.key,
    required this.track,
    this.coreEvents,
    this.onTap,
    this.onTitleTap,
    this.onPlayTap,
    this.onMoreTap,
    this.onContextMenu,
    required this.isSelected,
    required this.selectionModeActive,
    required this.onToggleSelected,
  });

  final Track track;
  final MashinCoreBridgeEvents? coreEvents;
  final VoidCallback? onTap;
  final VoidCallback? onTitleTap;
  final VoidCallback? onPlayTap;
  final ValueChanged<BuildContext>? onMoreTap;
  final ValueChanged<Offset>? onContextMenu;
  final bool isSelected;
  final bool selectionModeActive;
  final VoidCallback onToggleSelected;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final styles = context.mashinTableTextStyles;
    final artists = track.artists
        .map((artist) => artist.name.trim())
        .where((name) => name.isNotEmpty)
        .join(', ');
    final subtitle = artists.isNotEmpty
        ? artists
        : track.album?.name ?? 'Track';
    return Opacity(
      opacity: track.canPlay ? 1 : 0.35,
      child: Listener(
        onPointerDown: (event) {
          if (MediaItemProviderIcon.handlesPointer(event)) return;
          if (event.kind == PointerDeviceKind.mouse &&
              (event.buttons & kSecondaryMouseButton) != 0) {
            onContextMenu?.call(event.position);
          }
        },
        child: Material(
          color: isSelected ? colors.selection : colors.bgSecondary,
          borderRadius: BorderRadius.circular(5),
          clipBehavior: Clip.antiAlias,
          child: InkWell(
            onTap: track.canPlay ? onTap : null,
            child: SizedBox(
              height: 64,
              child: Padding(
                padding: const EdgeInsets.only(left: 8),
                child: Row(
                  children: [
                    GestureDetector(
                      onTap: () {},
                      child: MediaTableCover(
                        coverUrl: track.imageUriForSize(80),
                        fallbackIcon: Icons.music_note_outlined,
                        size: 48,
                        radius: 4,
                        onPlayTap: track.canPlay ? onPlayTap : null,
                      ),
                    ),
                    const SizedBox(width: 10),
                    Expanded(
                      child: Column(
                        mainAxisAlignment: MainAxisAlignment.center,
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          PlaybackAwareBuilder(
                            coreEvents: coreEvents,
                            item: track,
                            builder: (context, isCurrent) => MediaLinkText(
                              items:
                                  onTitleTap != null &&
                                      track.name.trim().isNotEmpty
                                  ? <MediaItem>[track]
                                  : const <MediaItem>[],
                              fallbackText: _displayName(track),
                              onItemTap: onTitleTap == null
                                  ? null
                                  : (_) => onTitleTap!(),
                              style: styles.mobileTitle.copyWith(
                                color: isCurrent ? colors.accent : null,
                              ),
                            ),
                          ),
                          const SizedBox(height: 1),
                          Text(
                            subtitle,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: styles.mobileSubtitle,
                          ),
                        ],
                      ),
                    ),
                    const SizedBox(width: 8),
                    SizedBox(
                      width: 42,
                      child: Text(
                        formatDuration(track.duration),
                        maxLines: 1,
                        textAlign: TextAlign.right,
                        style: styles.mobileTrailing,
                      ),
                    ),
                    const SizedBox(width: 4),
                    SizedBox(
                      width: 48,
                      child: MediaItemProviderIcon(item: track),
                    ),
                    Builder(
                      builder: (anchor) => SizedBox(
                        width: 40,
                        height: 48,
                        child: Center(
                          child: selectionModeActive
                              ? _DesktopSelectionCheckBox(
                                  isChecked: isSelected,
                                  onTap: onToggleSelected,
                                )
                              : Tooltip(
                                  message: 'Weitere Optionen',
                                  child: SectionHeaderIconButton(
                                    icon: Icons.more_vert_outlined,
                                    onTap: onMoreTap == null
                                        ? null
                                        : () => onMoreTap!(anchor),
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
        ),
      ),
    );
  }
}

class _DesktopSelectionCheckBox extends StatelessWidget {
  const _DesktopSelectionCheckBox({
    required this.isChecked,
    required this.onTap,
  });
  final bool isChecked;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return Semantics(
      checked: isChecked,
      button: true,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: onTap,
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

class _MediaListTile<T extends MediaItem> extends StatefulWidget {
  final T item;
  final MashinCoreBridgeApi? coreBridge;
  final ValueChanged<T>? onItemTap;
  final ValueChanged<Offset>? onContextMenu;

  const _MediaListTile({
    super.key,
    required this.item,
    required this.coreBridge,
    required this.onItemTap,
    required this.onContextMenu,
  });

  @override
  State<_MediaListTile<T>> createState() => _MediaListTileState<T>();
}

class _MediaListTileState<T extends MediaItem>
    extends State<_MediaListTile<T>> {
  bool _isHovered = false;
  bool _isPlayHovered = false;

  void _play() {
    final MashinCoreBridgeApi? bridge = widget.coreBridge;
    if (bridge == null || !_isValidPlayable(widget.item)) {
      return;
    }

    unawaited(() async {
      try {
        await bridge.queuePlayMedia(<MediaItem>[widget.item]);
      } catch (_) {
        // Playback failures should not block list hover actions.
      }
    }());
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final String title = _displayName(widget.item);

    return Opacity(
      opacity: widget.item.canPlay ? 1 : 0.35,
      child: MouseRegion(
        onEnter: (_) => setState(() => _isHovered = true),
        onExit: (_) => setState(() {
          _isHovered = false;
          _isPlayHovered = false;
        }),
        child: Listener(
          onPointerDown: (PointerDownEvent event) {
            if (event.kind == PointerDeviceKind.mouse &&
                (event.buttons & kSecondaryMouseButton) != 0) {
              widget.onContextMenu?.call(event.position);
            }
          },
          child: AnimatedContainer(
            duration: const Duration(milliseconds: 150),
            decoration: BoxDecoration(
              color: colors.bgSecondary,
              borderRadius: BorderRadius.circular(5),
              boxShadow: _isHovered
                  ? <BoxShadow>[
                      BoxShadow(
                        color: Colors.black.withValues(alpha: 0.20),
                        blurRadius: 8,
                        offset: const Offset(0, 4),
                      ),
                    ]
                  : null,
            ),
            clipBehavior: Clip.antiAlias,
            child: Stack(
              children: <Widget>[
                Positioned(
                  top: 0,
                  bottom: 0,
                  left: 0,
                  width: 8,
                  child: ColoredBox(color: accentColorFromText(title)),
                ),
                Padding(
                  padding: const EdgeInsets.only(left: 24, right: 56),
                  child: Align(
                    alignment: Alignment.centerLeft,
                    child: MediaLinkText(
                      items: <MediaItem>[widget.item],
                      fallbackText: title,
                      onItemTap: widget.onItemTap == null
                          ? null
                          : (MediaItem item) => widget.onItemTap!(item as T),
                      style: TextStyle(
                        color: colors.textSecondary,
                        fontWeight: FontWeight.w500,
                      ),
                    ),
                  ),
                ),
                Positioned(
                  top: 8,
                  right: 12,
                  child: IgnorePointer(
                    ignoring: !_isHovered,
                    child: AnimatedOpacity(
                      duration: const Duration(milliseconds: 150),
                      opacity: _isHovered ? 1 : 0,
                      child: AnimatedSlide(
                        duration: const Duration(milliseconds: 150),
                        offset: _isHovered
                            ? Offset.zero
                            : const Offset(0.25, 0),
                        child: MouseRegion(
                          onEnter: (_) => setState(() => _isPlayHovered = true),
                          onExit: (_) => setState(() => _isPlayHovered = false),
                          child: AnimatedScale(
                            duration: const Duration(milliseconds: 100),
                            scale: _isPlayHovered ? 1.1 : 1,
                            child: GestureDetector(
                              onTap: _play,
                              child: Container(
                                width: 32,
                                height: 32,
                                decoration: BoxDecoration(
                                  color: colors.overlayPlayButtonBackground,
                                  borderRadius: BorderRadius.circular(16),
                                ),
                                child: Icon(
                                  Icons.play_arrow_sharp,
                                  size: 21,
                                  color: colors.overlayPlayButtonForeground,
                                ),
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
      ),
    );
  }
}

class _DesktopListSkeletonGrid extends StatelessWidget {
  const _DesktopListSkeletonGrid({
    required this.columnCount,
    required this.rowExtent,
  });

  final int columnCount;
  final double rowExtent;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return GridView.builder(
      primary: false,
      shrinkWrap: true,
      physics: const NeverScrollableScrollPhysics(),
      gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
        crossAxisCount: columnCount,
        crossAxisSpacing: 12,
        mainAxisSpacing: 12,
        mainAxisExtent: rowExtent,
      ),
      itemCount: columnCount * 3,
      itemBuilder: (BuildContext context, int index) {
        return DecoratedBox(
          decoration: BoxDecoration(
            color: colors.bgSecondary,
            borderRadius: BorderRadius.circular(5),
          ),
          child: Row(
            children: <Widget>[
              Container(
                width: rowExtent == 64 ? 48 : 8,
                color: colors.bgTertiary,
              ),
              const SizedBox(width: 16),
              Expanded(
                child: Container(
                  height: 16,
                  decoration: BoxDecoration(
                    color: colors.bgTertiary,
                    borderRadius: BorderRadius.circular(8),
                  ),
                ),
              ),
              const SizedBox(width: 16),
            ],
          ),
        );
      },
    );
  }
}

String _displayName(MediaItem item) {
  final String name = item.name.trim();
  return name.isEmpty ? 'Unbekannt' : name;
}

bool _isValidPlayable(MediaItem item) {
  return item.canPlay &&
      item.itemId.trim().isNotEmpty &&
      item.provider.trim().isNotEmpty;
}

Color accentColorFromText(String text) {
  final String normalized = text.trim();
  if (normalized.isEmpty) {
    return const Color(0xFF7CA8FF);
  }

  final int hash = _stableTextHash('121212:$normalized');
  final double hue = (hash & 0xFFFF) % 360;
  final double saturation = 0.56 + (((hash >> 16) & 0xFF) / 255 * 0.16);
  final double lightness = 0.50 + (((hash >> 24) & 0xFF) / 255 * 0.12);
  return HSLColor.fromAHSL(1, hue, saturation, lightness).toColor();
}

int _stableTextHash(String text) {
  int hash = 2166136261;
  for (final int unit in text.codeUnits) {
    hash ^= unit;
    hash = (hash * 16777619) & 0xFFFFFFFF;
  }
  return hash;
}
