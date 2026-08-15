import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_svg/flutter_svg.dart';

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../selection/selection_session_controller.dart';

typedef DesktopRowAction<T> = void Function(T item, int index);
typedef DesktopRowFavoriteResolver<T> = bool Function(T item, int index);

class MediaTableSectionDesktop<T> extends StatefulWidget {
  final String title;
  final List<T> items;
  final bool isLoading;
  final String emptyText;
  final MediaTableRowData Function(T item, int index) rowMapper;
  final List<DesktopTableColumn<T>> desktopColumns;
  final int skeletonRowCount;
  final int maxItemsPerPage;
  final bool enableSelection;
  final ValueChanged<List<T>>? onSelectionChanged;
  final VoidCallback? onSelectionMenuTap;
  final VoidCallback? onHeaderMoreTap;
  final DesktopRowAction<T>? onRowMoreTap;
  final DesktopRowAction<T>? onRowFavoriteTap;
  final DesktopRowAction<T>? onRowDoubleTap;
  final DesktopRowFavoriteResolver<T>? isRowFavorite;

  const MediaTableSectionDesktop({
    super.key,
    required this.title,
    required this.items,
    required this.isLoading,
    required this.emptyText,
    required this.rowMapper,
    required this.desktopColumns,
    this.skeletonRowCount = 10,
    this.maxItemsPerPage = 10,
    this.enableSelection = true,
    this.onSelectionChanged,
    this.onSelectionMenuTap,
    this.onHeaderMoreTap,
    this.onRowMoreTap,
    this.onRowFavoriteTap,
    this.onRowDoubleTap,
    this.isRowFavorite,
  });

  @override
  State<MediaTableSectionDesktop<T>> createState() => _MediaTableSectionDesktopState<T>();
}

class _MediaTableSectionDesktopState<T> extends State<MediaTableSectionDesktop<T>> {
  static const double _desktopRowExtent = 64;
  static const Duration _desktopPageFadeDuration = Duration(milliseconds: 140);

  final FocusNode _sectionFocusNode = FocusNode(debugLabel: 'MediaTableDesktopSectionFocus');
  final Set<int> _selectedIndexes = <int>{};
  final Object _selectionSessionOwner = Object();
  final SelectionSessionController _selectionSessionController = SelectionSessionController.instance;
  final PageController _desktopChunkPageController = PageController();
  int? _selectionAnchorIndex;
  int _currentPage = 0;
  bool _isPageFading = false;

  @override
  void didUpdateWidget(covariant MediaTableSectionDesktop<T> oldWidget) {
    super.didUpdateWidget(oldWidget);

    _selectedIndexes.removeWhere((int index) => index < 0 || index >= widget.items.length);
    if (_selectionAnchorIndex != null && (_selectionAnchorIndex! < 0 || _selectionAnchorIndex! >= widget.items.length)) {
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

    final int lastPage = _pageCount > 0 ? _pageCount - 1 : 0;
    if (_currentPage > lastPage) {
      _currentPage = lastPage;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted || !_desktopChunkPageController.hasClients) {
          return;
        }
        _desktopChunkPageController.jumpToPage(_currentPage);
      });
    }
  }

  @override
  void dispose() {
    _sectionFocusNode.dispose();
    _desktopChunkPageController.dispose();
    _deactivateSelectionSession();
    super.dispose();
  }

  // Selection and Paging
  bool get _isPagingEnabled => widget.maxItemsPerPage > 0;
  bool get _isAllSelected => widget.items.isNotEmpty && _selectedIndexes.length == widget.items.length;
  int get _chunkSize => _isPagingEnabled ? widget.maxItemsPerPage : 0;

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
    return widget.isRowFavorite?.call(item, index) ?? false;
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
        ..addAll(List<int>.generate(end - start + 1, (int offset) => start + offset));
    });
    _syncSelectionSession();
    _emitSelectionChanged();
  }

  void _handleRowPointerDown(int index, PointerDownEvent event) {
    if (!widget.enableSelection || event.kind != PointerDeviceKind.mouse || event.buttons != kPrimaryMouseButton) {
      return;
    }

    final Set<LogicalKeyboardKey> keys = HardwareKeyboard.instance.logicalKeysPressed;
    final bool isCtrlPressed = keys.contains(LogicalKeyboardKey.controlLeft) || keys.contains(LogicalKeyboardKey.controlRight);
    final bool isShiftPressed = keys.contains(LogicalKeyboardKey.shiftLeft) || keys.contains(LogicalKeyboardKey.shiftRight);

    if (!isCtrlPressed && !isShiftPressed) {
      return;
    }

    _requestSectionFocus();

    if (isShiftPressed) {
      _selectRangeTo(index);
      return;
    }

    if (isCtrlPressed) {
      _toggleSelectedIndex(index);
    }
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
          ..addAll(List<int>.generate(widget.items.length, (int index) => index));
      }
    });
    _syncSelectionSession();
    _emitSelectionChanged();
  }

  void _goToPreviousPage() {
    if (!_isPagingEnabled || _currentPage <= 0) {
      return;
    }
    _fadeToPage(_currentPage - 1);
  }

  void _goToNextPage() {
    if (!_isPagingEnabled || _currentPage >= _pageCount - 1) {
      return;
    }
    _fadeToPage(_currentPage + 1);
  }

  Future<void> _fadeToPage(int targetPage) async {
    if (_isPageFading || targetPage < 0 || targetPage >= _pageCount || targetPage == _currentPage) {
      return;
    }

    setState(() {
      _isPageFading = true;
    });

    await Future<void>.delayed(_desktopPageFadeDuration);
    if (!mounted) {
      return;
    }

    if (_desktopChunkPageController.hasClients) {
      _desktopChunkPageController.jumpToPage(targetPage);
    }

    setState(() {
      _currentPage = targetPage;
      _isPageFading = false;
    });
  }


  // Build
  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    final Widget content = LayoutBuilder(
      builder: (BuildContext context, BoxConstraints constraints) {
        final bool hasBoundedHeight = constraints.maxHeight.isFinite;
        final bool fillScrollableViewport = hasBoundedHeight && !_isPagingEnabled;

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
            Row(
              children: <Widget>[
                Expanded(
                  child: Text(
                    widget.title,
                    style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
                  ),
                ),
                if (_isPagingEnabled && widget.items.isNotEmpty) _buildPaginationControls(context),
              ],
            ),
            const SizedBox(height: 10),
            if (hasBoundedHeight)
              Expanded(child: ClipRect(child: tableContent))
            else
              tableContent,
          ],
        );
      },
    );

    final Widget focusableContent = Focus(
      focusNode: _sectionFocusNode,
      child: Listener(
        onPointerDown: _handleSectionPointerDown,
        child: content,
      ),
    );

    if (!widget.enableSelection) {
      return focusableContent;
    }

    return Shortcuts(
      shortcuts: const <ShortcutActivator, Intent>{
        SingleActivator(LogicalKeyboardKey.keyA, control: true): _SelectAllIntent(),
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
    final columns = widget.desktopColumns;
    final bool useInternalScroll = fillScrollableViewport || constrainToHeight;
    final double pagedContentHeight = _chunkSize * _desktopRowExtent;

    if (widget.isLoading) {
      final Widget body = ListView.builder(
        primary: false,
        shrinkWrap: !useInternalScroll,
        physics: useInternalScroll ? const ClampingScrollPhysics() : const NeverScrollableScrollPhysics(),
        addAutomaticKeepAlives: false,
        addRepaintBoundaries: true,
        addSemanticIndexes: false,
        itemCount: widget.skeletonRowCount,
        itemExtent: _desktopRowExtent,
        itemBuilder: (BuildContext context, int index) {
          return _DesktopTableRowSkeleton<T>(
            columns: columns,
            isFirst: index == 0,
            showSelection: widget.enableSelection,
          );
        },
      );

      return Column(
        mainAxisSize: useInternalScroll ? MainAxisSize.max : MainAxisSize.min,
        children: <Widget>[
          _DesktopTableHeader<T>(
            columns: columns,
            showSelection: widget.enableSelection,
            isAllSelected: _isAllSelected,
            onToggleSelectAll: _toggleAllSelectedIndexes,
            onMoreTap: widget.onHeaderMoreTap,
          ),
          if (useInternalScroll)
            Expanded(child: body)
          else
            body,
        ],
      );
    }

    if (widget.items.isEmpty) {
      final textStyles = context.mashinTableTextStyles;
      return Column(
        mainAxisSize: MainAxisSize.min,
        children: <Widget>[
          _DesktopTableHeader<T>(
            columns: columns,
            showSelection: widget.enableSelection,
            isAllSelected: false,
            onToggleSelectAll: _toggleAllSelectedIndexes,
            onMoreTap: widget.onHeaderMoreTap,
          ),
          Padding(
            padding: const EdgeInsets.all(16),
            child: Align(
              alignment: Alignment.centerLeft,
              child: Text(
                widget.emptyText,
                style: textStyles.emptyState,
              ),
            ),
          ),
        ],
      );
    }

    final Widget list = _isPagingEnabled
        ? AnimatedOpacity(
            duration: _desktopPageFadeDuration,
            curve: Curves.easeOut,
            opacity: _isPageFading ? 0 : 1,
            child: PageView.builder(
              controller: _desktopChunkPageController,
              physics: const NeverScrollableScrollPhysics(),
              itemCount: _pageCount,
              onPageChanged: (int page) {
                if (page == _currentPage) {
                  return;
                }
                setState(() {
                  _currentPage = page;
                });
              },
              itemBuilder: (BuildContext context, int pageIndex) {
                final int start = pageIndex * _chunkSize;
                final int end = (start + _chunkSize).clamp(0, widget.items.length);
                final List<T> chunkItems = widget.items.sublist(start, end);

                return ListView.builder(
                  primary: false,
                  shrinkWrap: !useInternalScroll,
                  physics: useInternalScroll ? const ClampingScrollPhysics() : const NeverScrollableScrollPhysics(),
                  addAutomaticKeepAlives: false,
                  addRepaintBoundaries: true,
                  addSemanticIndexes: false,
                  itemCount: chunkItems.length,
                  itemExtent: _desktopRowExtent,
                  itemBuilder: (BuildContext context, int index) {
                    final int itemIndex = start + index;
                    return _DesktopTableRow<T>(
                      columns: columns,
                      item: chunkItems[index],
                      index: itemIndex,
                      isFirst: index == 0,
                      isSelected: _selectedIndexes.contains(itemIndex),
                      isFavorite: _isItemFavorite(chunkItems[index], itemIndex),
                      selectionEnabled: widget.enableSelection,
                      onToggleSelected: () => _toggleSelectedIndex(itemIndex),
                      onFavoriteTap: widget.onRowFavoriteTap == null
                          ? null
                          : () => widget.onRowFavoriteTap!(chunkItems[index], itemIndex),
                      onMoreTap: widget.onRowMoreTap == null
                          ? null
                          : () => widget.onRowMoreTap!(chunkItems[index], itemIndex),
                        onDoubleTap: widget.onRowDoubleTap == null
                          ? null
                          : () => widget.onRowDoubleTap!(chunkItems[index], itemIndex),
                      onPointerDown: (PointerDownEvent event) => _handleRowPointerDown(itemIndex, event),
                    );
                  },
                );
              },
            ),
          )
        : ListView.builder(
            primary: false,
            shrinkWrap: !useInternalScroll,
            physics: useInternalScroll ? const ClampingScrollPhysics() : const NeverScrollableScrollPhysics(),
            addAutomaticKeepAlives: false,
            addRepaintBoundaries: true,
            addSemanticIndexes: false,
            itemCount: widget.items.length,
            itemExtent: _desktopRowExtent,
            itemBuilder: (BuildContext context, int index) {
              final int itemIndex = index;
              return _DesktopTableRow<T>(
                columns: columns,
                item: widget.items[index],
                index: itemIndex,
                isFirst: index == 0,
                isSelected: _selectedIndexes.contains(itemIndex),
                isFavorite: _isItemFavorite(widget.items[index], itemIndex),
                selectionEnabled: widget.enableSelection,
                onToggleSelected: () => _toggleSelectedIndex(itemIndex),
                onFavoriteTap:
                    widget.onRowFavoriteTap == null ? null : () => widget.onRowFavoriteTap!(widget.items[index], itemIndex),
                onMoreTap: widget.onRowMoreTap == null ? null : () => widget.onRowMoreTap!(widget.items[index], itemIndex),
                onDoubleTap: widget.onRowDoubleTap == null ? null : () => widget.onRowDoubleTap!(widget.items[index], itemIndex),
                onPointerDown: (PointerDownEvent event) => _handleRowPointerDown(itemIndex, event),
              );
            },
          );

    return Column(
      mainAxisSize: useInternalScroll ? MainAxisSize.max : MainAxisSize.min,
      children: <Widget>[
        _DesktopTableHeader<T>(
          columns: columns,
          showSelection: widget.enableSelection,
          isAllSelected: _isAllSelected,
          onToggleSelectAll: _toggleAllSelectedIndexes,
          onMoreTap: widget.onHeaderMoreTap,
        ),
        if (useInternalScroll)
          Expanded(child: list)
        else if (_isPagingEnabled)
          SizedBox(
            height: pagedContentHeight,
            child: list,
          )
        else
          list,
      ],
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
          icon: const Icon(Icons.chevron_left_rounded),
          visualDensity: VisualDensity.compact,
        ),
        IconButton(
          tooltip: 'Nächste Seite',
          onPressed: canGoNext ? _goToNextPage : null,
          icon: const Icon(Icons.chevron_right_rounded),
          visualDensity: VisualDensity.compact,
        ),
      ],
    );
  }

  Widget _buildPagingTransition(Widget child) {
    return child;
  }

}


// Desktop Table Widgets
class _DesktopTableHeader<T> extends StatelessWidget {
  final List<DesktopTableColumn<T>> columns;
  final bool showSelection;
  final bool isAllSelected;
  final VoidCallback onToggleSelectAll;
  final VoidCallback? onMoreTap;

  const _DesktopTableHeader({
    required this.columns,
    required this.showSelection,
    required this.isAllSelected,
    required this.onToggleSelectAll,
    required this.onMoreTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final textStyles = context.mashinTableTextStyles;

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
          ...columns
              .map(
                (column) => buildDesktopCellWidget(
                  flex: column.flex,
                  width: column.width,
                  child: Padding(
                    padding: column.padding,
                    child: Text(
                      column.header,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      textAlign: column.textAlign,
                      style: textStyles.header,
                    ),
                  ),
                ),
              ),
          SizedBox(
            width: 40,
            child: Center(
              child: Icon(
                Icons.favorite_border_rounded,
                size: 18,
                color: colors.textSecondary,
              ),
            ),
          ),
          SizedBox(
            width: 40,
            child: Center(
              child: _DesktopIconActionButton(
                icon: Icons.more_horiz_rounded,
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
  }
}

class _DesktopTableRow<T> extends StatefulWidget {
  final List<DesktopTableColumn<T>> columns;
  final T item;
  final int index;
  final bool isFirst;
  final bool isSelected;
  final bool isFavorite;
  final bool selectionEnabled;
  final VoidCallback onToggleSelected;
  final VoidCallback? onFavoriteTap;
  final VoidCallback? onMoreTap;
  final VoidCallback? onDoubleTap;
  final ValueChanged<PointerDownEvent>? onPointerDown;

  const _DesktopTableRow({
    required this.columns,
    required this.item,
    required this.index,
    required this.isFirst,
    required this.isSelected,
    required this.isFavorite,
    required this.selectionEnabled,
    required this.onToggleSelected,
    this.onFavoriteTap,
    this.onMoreTap,
    this.onDoubleTap,
    this.onPointerDown,
  });

  @override
  State<_DesktopTableRow<T>> createState() => _DesktopTableRowState<T>();
}

class _DesktopTableRowState<T> extends State<_DesktopTableRow<T>> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final textStyles = context.mashinTableTextStyles;

    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onDoubleTap: widget.onDoubleTap,
        child: Container(
          height: 64,
          decoration: BoxDecoration(
            color: widget.isSelected ? colors.hover : (_isHovered ? colors.hover : Colors.transparent),
            border: widget.isFirst ? null : Border(top: BorderSide(color: colors.border)),
          ),
          child: Row(
            children: <Widget>[
              Expanded(
                child: Listener(
                  onPointerDown: widget.onPointerDown,
                  child: Row(
                    children: <Widget>[
                      ...widget.columns
                          .map(
                            (column) => buildDesktopCellWidget(
                              flex: column.flex,
                              width: column.width,
                              child: Padding(
                                padding: column.padding,
                                child: column.childBuilder != null
                                    ? column.childBuilder!(context, widget.item, widget.index)
                                    : Text(
                                        column.valueBuilder!(widget.item, widget.index),
                                        maxLines: 1,
                                        overflow: TextOverflow.ellipsis,
                                        textAlign: column.textAlign,
                                        style: (() {
                                          switch (column.textTone) {
                                            case DesktopTextTone.primary:
                                              return textStyles.bodyPrimary;
                                            case DesktopTextTone.secondary:
                                              return textStyles.bodySecondary;
                                            case DesktopTextTone.tertiary:
                                              return textStyles.bodyTertiary;
                                          }
                                        })().copyWith(fontSize: column.fontSize ?? 13),
                                      ),
                              ),
                            ),
                          ),
                    ],
                  ),
                ),
              ),
              SizedBox(
                width: 40,
                child: Center(
                  child: _DesktopIconActionButton(
                    icon: widget.isFavorite ? Icons.favorite_rounded : Icons.favorite_border_rounded,
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
                    icon: Icons.more_horiz_rounded,
                    onTap: widget.onMoreTap,
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
    );
  }
}

class _DesktopTableRowSkeleton<T> extends StatelessWidget {
  final List<DesktopTableColumn<T>> columns;
  final bool isFirst;
  final bool showSelection;

  const _DesktopTableRowSkeleton({
    required this.columns,
    required this.isFirst,
    required this.showSelection,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    Widget bar({required double width, required double height, double radius = 4}) {
      return Container(
        width: width,
        height: height,
        decoration: BoxDecoration(
          color: colors.bgTertiary,
          borderRadius: BorderRadius.circular(radius),
        ),
      );
    }

    Widget skeletonFor(DesktopTableColumn<T> column, int columnIndex) {
      if (columnIndex == 1) {
        return bar(width: 48, height: 48, radius: 8);
      }

      if (column.width != null) {
        return Align(
          alignment: alignmentForText(column.textAlign),
          child: bar(
            width: (column.width! * 0.5).clamp(16, 60),
            height: 14,
          ),
        );
      }

      return Align(
        alignment: alignmentForText(column.textAlign),
        child: bar(width: columnIndex == 2 ? 220 : 120, height: 14),
      );
    }

    return Container(
      height: 64,
      decoration: BoxDecoration(
        color: Colors.transparent,
        border: isFirst ? null : Border(top: BorderSide(color: colors.border)),
      ),
      child: Row(
        children: <Widget>[
          ...columns.asMap().entries.map(
                (entry) => buildDesktopCellWidget(
                  flex: entry.value.flex,
                  width: entry.value.width,
                  child: Padding(
                    padding: entry.value.padding,
                    child: skeletonFor(entry.value, entry.key),
                  ),
                ),
              ),
          SizedBox(
            width: 40,
            child: Center(
              child: bar(width: 16, height: 16, radius: 8),
            ),
          ),
          SizedBox(
            width: 40,
            child: Center(
              child: bar(width: 16, height: 16, radius: 8),
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
  }
}

class _DesktopIconActionButton extends StatefulWidget {
  final IconData icon;
  final VoidCallback? onTap;
  final Color baseColor;
  final Color hoverColor;

  const _DesktopIconActionButton({
    required this.icon,
    required this.onTap,
    required this.baseColor,
    required this.hoverColor,
  });

  @override
  State<_DesktopIconActionButton> createState() => _DesktopIconActionButtonState();
}

class _DesktopIconActionButtonState extends State<_DesktopIconActionButton> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    final Color iconColor = _isHovered ? widget.hoverColor : widget.baseColor;

    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: IconButton(
        onPressed: widget.onTap,
        padding: EdgeInsets.zero,
        constraints: const BoxConstraints(minWidth: 32, minHeight: 32),
        splashRadius: 18,
        visualDensity: VisualDensity.compact,
        icon: AnimatedScale(
          scale: _isHovered ? 1.08 : 1,
          duration: const Duration(milliseconds: 120),
          curve: Curves.easeOutCubic,
          child: Icon(
            widget.icon,
            size: 18,
            color: iconColor,
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

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return IgnorePointer(
      ignoring: !isVisible,
      child: GestureDetector(
        onTap: onTap,
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


// Base Row and Column Data Models
class MediaTableRowData {
  final String title;
  final String subtitle;
  final String trailing;
  final String? coverUrl;
  final IconData fallbackIcon;
  final String providerLabel;
  final String? providerIconSvg;

  const MediaTableRowData({
    required this.title,
    required this.subtitle,
    required this.trailing,
    required this.coverUrl,
    required this.fallbackIcon,
    required this.providerLabel,
    required this.providerIconSvg,
  });
}

enum DesktopTextTone {
  primary,
  secondary,
  tertiary,
}

class DesktopTableColumn<T> {
  final String header;
  final int? flex;
  final double? width;
  final EdgeInsetsGeometry padding;
  final TextAlign textAlign;
  final DesktopTextTone textTone;
  final double? fontSize;
  final String Function(T item, int index)? valueBuilder;
  final Widget Function(BuildContext context, T item, int index)? childBuilder;

  const DesktopTableColumn({
    required this.header,
    this.flex,
    this.width,
    this.padding = const EdgeInsets.symmetric(horizontal: 8),
    this.textAlign = TextAlign.left,
    this.textTone = DesktopTextTone.primary,
    this.fontSize,
    this.valueBuilder,
    this.childBuilder,
  }) : assert(
          valueBuilder != null || childBuilder != null,
          'Either valueBuilder or childBuilder must be provided.',
        );
}

Widget buildDesktopCellWidget({
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

Alignment alignmentForText(TextAlign textAlign) {
  switch (textAlign) {
    case TextAlign.right:
    case TextAlign.end:
      return Alignment.centerRight;
    case TextAlign.center:
      return Alignment.center;
    case TextAlign.left:
    case TextAlign.start:
    case TextAlign.justify:
      return Alignment.centerLeft;
  }
}

class MediaTableCover extends StatelessWidget {
  final String? coverUrl;
  final IconData fallbackIcon;
  final double size;
  final double radius;

  const MediaTableCover({
    super.key,
    required this.coverUrl,
    required this.fallbackIcon,
    this.size = 48,
    this.radius = 8,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final String? trimmedCover = coverUrl?.trim();
    final bool hasCover = trimmedCover != null && trimmedCover.isNotEmpty;

    return ClipRRect(
      borderRadius: BorderRadius.circular(radius),
      child: Container(
        width: size,
        height: size,
        color: colors.bgTertiary,
        child: hasCover
            ? Image.network(
                trimmedCover,
                fit: BoxFit.cover,
                errorBuilder: (_, _, _) => Icon(fallbackIcon, color: colors.accent, size: size * 0.5),
              )
            : Icon(fallbackIcon, color: colors.accent, size: size * 0.5),
      ),
    );
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
      return Icon(Icons.radio_button_checked_rounded, size: size, color: colors.textSecondary);
    }

    // Provider manifests may deliver raw XML, XML with declaration, or URL-encoded XML.
    final String decoded = _maybeDecodeSvgPayload(trimmed);
    final bool isInlineSvg =
        decoded.startsWith('<svg') || decoded.startsWith('<?xml') || decoded.contains('<svg');

    if (isInlineSvg) {
      return SvgPicture.string(
        decoded,
        width: size,
        height: size,
      );
    }

    final Uri? uri = Uri.tryParse(trimmed);
    final bool isNetworkSvg = uri != null && (uri.scheme == 'http' || uri.scheme == 'https');
    if (!isNetworkSvg) {
      return Icon(Icons.radio_button_checked_rounded, size: size, color: colors.textSecondary);
    }

    return SvgPicture.network(
      trimmed,
      width: size,
      height: size,
      placeholderBuilder: (_) => Icon(Icons.radio_button_checked_rounded, size: size, color: colors.textSecondary),
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


// Row and Column Mappers
MediaTableRowData mapTrackRowData(MaTrack track, int index) {
  final String providerLabel = _providerLabelFromId(track.provider);
  return MediaTableRowData(
    title: _prefer(track.name, 'Unknown Track'),
    subtitle: _trackSubtitle(track),
    trailing: formatDuration(track.duration),
    coverUrl: track.imageUri,
    fallbackIcon: Icons.music_note_rounded,
    providerLabel: providerLabel,
    providerIconSvg: MaProviderManifestRegistry.iconSvgForProvider(track.provider),
  );
}

MediaTableRowData mapAlbumRowData(MaAlbum album, int index) {
  final String providerLabel = _providerLabelFromId(album.provider);
  return MediaTableRowData(
    title: _prefer(album.name, 'Unknown Album'),
    subtitle: _albumSubtitle(album),
    trailing: album.year != null && album.year! > 0 ? '${album.year}' : '-',
    coverUrl: album.imageUri,
    fallbackIcon: Icons.album_rounded,
    providerLabel: providerLabel,
    providerIconSvg: MaProviderManifestRegistry.iconSvgForProvider(album.provider),
  );
}

MediaTableRowData mapPlaylistRowData(MaPlaylist playlist, int index) {
  final String providerLabel = _providerLabelFromId(playlist.provider);
  return MediaTableRowData(
    title: _prefer(playlist.name, 'Unknown Playlist'),
    subtitle: _playlistSubtitle(playlist),
    trailing: playlist.isEditable ? 'Editierbar' : 'Readonly',
    coverUrl: playlist.imageUri,
    fallbackIcon: Icons.queue_music_rounded,
    providerLabel: providerLabel,
    providerIconSvg: MaProviderManifestRegistry.iconSvgForProvider(playlist.provider),
  );
}

MediaTableRowData mapArtistRowData(MaArtist artist, int index) {
  final String providerLabel = _providerLabelFromId(artist.provider);
  return MediaTableRowData(
    title: _prefer(artist.name, 'Unknown Artist'),
    subtitle: 'Artist',
    trailing: _artistTertiary(artist),
    coverUrl: artist.imageUri,
    fallbackIcon: Icons.person_rounded,
    providerLabel: providerLabel,
    providerIconSvg: MaProviderManifestRegistry.iconSvgForProvider(artist.provider),
  );
}

List<DesktopTableColumn<MaTrack>> buildTrackDesktopColumns() {
  return <DesktopTableColumn<MaTrack>>[
    const DesktopTableColumn<MaTrack>(
      header: '#',
      width: 44,
      textAlign: TextAlign.left,
      textTone: DesktopTextTone.tertiary,
      fontSize: 12,
      valueBuilder: _trackIndexValue,
    ),
    DesktopTableColumn<MaTrack>(
      header: 'Cover',
      width: 62,
      childBuilder: (BuildContext context, MaTrack track, int index) {
        return MediaTableCover(
          coverUrl: track.imageUri,
          fallbackIcon: Icons.music_note_rounded,
          size: 48,
          radius: 8,
        );
      },
    ),
    const DesktopTableColumn<MaTrack>(
      header: 'Titel',
      flex: 5,
      textTone: DesktopTextTone.primary,
      valueBuilder: _trackTitleValue,
    ),
    const DesktopTableColumn<MaTrack>(
      header: 'Artist',
      flex: 3,
      textTone: DesktopTextTone.secondary,
      valueBuilder: _trackArtistValue,
    ),
    const DesktopTableColumn<MaTrack>(
      header: 'Album',
      flex: 3,
      textTone: DesktopTextTone.secondary,
      valueBuilder: _trackAlbumValue,
    ),
    const DesktopTableColumn<MaTrack>(
      header: 'Länge',
      width: 76,
      textAlign: TextAlign.right,
      textTone: DesktopTextTone.tertiary,
      valueBuilder: _trackDurationValue,
    ),
    DesktopTableColumn<MaTrack>(
      header: 'Quelle',
      width: 72,
      textAlign: TextAlign.center,
      childBuilder: (BuildContext context, MaTrack track, int index) {
        final String providerLabel = _providerLabelFromId(track.provider);
        return Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: <Widget>[
            MediaProviderIcon(
              providerLabel: providerLabel,
              iconSvg: MaProviderManifestRegistry.iconSvgForProvider(track.provider),
              size: 16,
            ),
          ],
        );
      },
    ),
  ];
}

List<DesktopTableColumn<MaAlbum>> buildAlbumDesktopColumns() {
  return <DesktopTableColumn<MaAlbum>>[
    const DesktopTableColumn<MaAlbum>(
      header: '#',
      width: 44,
      textAlign: TextAlign.left,
      textTone: DesktopTextTone.tertiary,
      fontSize: 12,
      valueBuilder: _albumIndexValue,
    ),
    DesktopTableColumn<MaAlbum>(
      header: 'Cover',
      width: 62,
      childBuilder: (BuildContext context, MaAlbum album, int index) {
        return MediaTableCover(
          coverUrl: album.imageUri,
          fallbackIcon: Icons.album_rounded,
          size: 48,
          radius: 8,
        );
      },
    ),
    const DesktopTableColumn<MaAlbum>(
      header: 'Titel',
      flex: 5,
      textTone: DesktopTextTone.primary,
      valueBuilder: _albumTitleValue,
    ),
    const DesktopTableColumn<MaAlbum>(
      header: 'Artist',
      flex: 4,
      textTone: DesktopTextTone.secondary,
      valueBuilder: _albumArtistValue,
    ),
    const DesktopTableColumn<MaAlbum>(
      header: 'Typ',
      flex: 2,
      textTone: DesktopTextTone.secondary,
      valueBuilder: _albumTypeValue,
    ),
    const DesktopTableColumn<MaAlbum>(
      header: 'Jahr',
      width: 64,
      textAlign: TextAlign.right,
      textTone: DesktopTextTone.tertiary,
      valueBuilder: _albumYearValue,
    ),
    DesktopTableColumn<MaAlbum>(
      header: 'Quelle',
      width: 72,
      textAlign: TextAlign.center,
      childBuilder: (BuildContext context, MaAlbum album, int index) {
        final String providerLabel = _providerLabelFromId(album.provider);
        return Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: <Widget>[
            MediaProviderIcon(
              providerLabel: providerLabel,
              iconSvg: MaProviderManifestRegistry.iconSvgForProvider(album.provider),
              size: 16,
            ),
          ],
        );
      },
    ),
  ];
}

List<DesktopTableColumn<MaPlaylist>> buildPlaylistDesktopColumns() {
  return <DesktopTableColumn<MaPlaylist>>[
    const DesktopTableColumn<MaPlaylist>(
      header: '#',
      width: 44,
      textAlign: TextAlign.left,
      textTone: DesktopTextTone.tertiary,
      fontSize: 12,
      valueBuilder: _playlistIndexValue,
    ),
    DesktopTableColumn<MaPlaylist>(
      header: 'Cover',
      width: 62,
      childBuilder: (BuildContext context, MaPlaylist playlist, int index) {
        return MediaTableCover(
          coverUrl: playlist.imageUri,
          fallbackIcon: Icons.queue_music_rounded,
          size: 48,
          radius: 8,
        );
      },
    ),
    const DesktopTableColumn<MaPlaylist>(
      header: 'Titel',
      flex: 5,
      textTone: DesktopTextTone.primary,
      valueBuilder: _playlistTitleValue,
    ),
    const DesktopTableColumn<MaPlaylist>(
      header: 'Owner',
      flex: 4,
      textTone: DesktopTextTone.secondary,
      valueBuilder: _playlistOwnerValue,
    ),
    const DesktopTableColumn<MaPlaylist>(
      header: 'Status',
      flex: 2,
      textTone: DesktopTextTone.tertiary,
      valueBuilder: _playlistStatusValue,
    ),
    DesktopTableColumn<MaPlaylist>(
      header: 'Quelle',
      width: 72,
      textAlign: TextAlign.center,
      childBuilder: (BuildContext context, MaPlaylist playlist, int index) {
        final String providerLabel = _providerLabelFromId(playlist.provider);
        return Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: <Widget>[
            MediaProviderIcon(
              providerLabel: providerLabel,
              iconSvg: MaProviderManifestRegistry.iconSvgForProvider(playlist.provider),
              size: 16,
            ),
          ],
        );
      },
    ),
  ];
}

List<DesktopTableColumn<MaArtist>> buildArtistDesktopColumns() {
  return <DesktopTableColumn<MaArtist>>[
    const DesktopTableColumn<MaArtist>(
      header: '#',
      width: 44,
      textAlign: TextAlign.left,
      textTone: DesktopTextTone.tertiary,
      fontSize: 12,
      valueBuilder: _artistIndexValue,
    ),
    DesktopTableColumn<MaArtist>(
      header: 'Bild',
      width: 62,
      childBuilder: (BuildContext context, MaArtist artist, int index) {
        return MediaTableCover(
          coverUrl: artist.imageUri,
          fallbackIcon: Icons.person_rounded,
          size: 48,
          radius: 8,
        );
      },
    ),
    const DesktopTableColumn<MaArtist>(
      header: 'Artist',
      flex: 6,
      textTone: DesktopTextTone.primary,
      valueBuilder: _artistTitleValue,
    ),
    const DesktopTableColumn<MaArtist>(
      header: 'Provider',
      flex: 3,
      textTone: DesktopTextTone.secondary,
      valueBuilder: _artistProviderValue,
    ),
    DesktopTableColumn<MaArtist>(
      header: 'Quelle',
      width: 72,
      textAlign: TextAlign.center,
      childBuilder: (BuildContext context, MaArtist artist, int index) {
        final String providerLabel = _providerLabelFromId(artist.provider);
        return Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: <Widget>[
            MediaProviderIcon(
              providerLabel: providerLabel,
              iconSvg: MaProviderManifestRegistry.iconSvgForProvider(artist.provider),
              size: 16,
            ),
          ],
        );
      },
    ),
  ];
}


// Value Builders for Desktop Table Columns
String _trackIndexValue(MaTrack track, int index) => '${index + 1}';
String _trackTitleValue(MaTrack track, int index) => _prefer(track.name, 'Unknown Track');
String _trackArtistValue(MaTrack track, int index) => _trackSubtitle(track);
String _trackAlbumValue(MaTrack track, int index) => _prefer(track.album?.name ?? '', '-');
String _trackDurationValue(MaTrack track, int index) => formatDuration(track.duration);

String _albumIndexValue(MaAlbum album, int index) => '${index + 1}';
String _albumTitleValue(MaAlbum album, int index) => _prefer(album.name, 'Unknown Album');
String _albumArtistValue(MaAlbum album, int index) => _albumSubtitle(album);
String _albumTypeValue(MaAlbum album, int index) => _mediaTypeLabel(album.albumType);
String _albumYearValue(MaAlbum album, int index) => album.year != null && album.year! > 0 ? '${album.year}' : '-';

String _playlistIndexValue(MaPlaylist playlist, int index) => '${index + 1}';
String _playlistTitleValue(MaPlaylist playlist, int index) => _prefer(playlist.name, 'Unknown Playlist');
String _playlistOwnerValue(MaPlaylist playlist, int index) => _playlistSubtitle(playlist);
String _playlistStatusValue(MaPlaylist playlist, int index) => playlist.isEditable ? 'Editierbar' : 'Readonly';

String _artistIndexValue(MaArtist artist, int index) => '${index + 1}';
String _artistTitleValue(MaArtist artist, int index) => _prefer(artist.name, 'Unknown Artist');
String _artistProviderValue(MaArtist artist, int index) => _providerLabelFromId(artist.provider);

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

String _albumSubtitle(MaAlbum album) {
  final artist = firstArtistName(album.artists);
  if (artist != '-') {
    return artist;
  }
  return _mediaTypeLabel(album.albumType);
}

String _trackSubtitle(MaTrack track) {
  final artist = firstArtistName(track.artists);
  if (artist != '-') {
    return artist;
  }
  return _prefer(track.album?.name ?? '', 'Track');
}

String _playlistSubtitle(MaPlaylist playlist) {
  final owner = _stripOrEmpty(playlist.owner);
  if (owner.isNotEmpty) {
    return owner;
  }
  return 'Playlist';
}

String _artistTertiary(MaArtist artist) {
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

String firstArtistName(List<MaArtist> artists) {
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
