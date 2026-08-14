import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';
import '../selection/selection_session_controller.dart';
import 'media_table_section_desktop.dart';

class MediaTableSectionMobile<T> extends StatefulWidget {
  final String title;
  final List<T> items;
  final bool isLoading;
  final String emptyText;
  final MediaTableRowData Function(T item, int index) rowMapper;
  final int skeletonRowCount;
  final int maxItemsPerPage;
  final bool enableSelection;
  final ValueChanged<List<T>>? onSelectionChanged;
  final VoidCallback? onSelectionMenuTap;

  const MediaTableSectionMobile({
    super.key,
    required this.title,
    required this.items,
    required this.isLoading,
    required this.emptyText,
    required this.rowMapper,
    this.skeletonRowCount = 10,
    this.maxItemsPerPage = 10,
    this.enableSelection = true,
    this.onSelectionChanged,
    this.onSelectionMenuTap,
  });

  @override
  State<MediaTableSectionMobile<T>> createState() => _MediaTableSectionMobileState<T>();
}

class _MediaTableSectionMobileState<T> extends State<MediaTableSectionMobile<T>> {
  static const double _mobileRowExtent = 64;
  static const double _mobileChunkViewportFraction = 0.92;
  static const double _mobileChunkGap = 8;

  final Set<int> _selectedIndexes = <int>{};
  final Object _selectionSessionOwner = Object();
  final SelectionSessionController _selectionSessionController = SelectionSessionController.instance;
  final PageController _mobileChunkPageController =
      PageController(viewportFraction: _mobileChunkViewportFraction);

  bool _isMobileSelectionMode = false;

  bool get _isChunkModeEnabled => widget.maxItemsPerPage > 0;
  int get _chunkSize => _isChunkModeEnabled ? widget.maxItemsPerPage : 0;
  bool get _isAllSelected => widget.items.isNotEmpty && _selectedIndexes.length == widget.items.length;

  @override
  void didUpdateWidget(covariant MediaTableSectionMobile<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    _selectedIndexes.removeWhere((int index) => index < 0 || index >= widget.items.length);

    if (_selectedIndexes.isEmpty) {
      _isMobileSelectionMode = false;
      _deactivateSelectionSession();
    } else {
      _syncSelectionSession();
    }
  }

  @override
  void dispose() {
    _mobileChunkPageController.dispose();
    _deactivateSelectionSession();
    super.dispose();
  }

  // Selection and Paging
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

  void _enterSelectionMode(int index) {
    if (!widget.enableSelection || index < 0 || index >= widget.items.length) {
      return;
    }

    setState(() {
      _isMobileSelectionMode = true;
      _selectedIndexes.add(index);
    });
    _syncSelectionSession();
    _emitSelectionChanged();
  }

  void _toggleSelectedIndex(int index) {
    if (!widget.enableSelection || !_isMobileSelectionMode || index < 0 || index >= widget.items.length) {
      return;
    }

    setState(() {
      if (_selectedIndexes.contains(index)) {
        _selectedIndexes.remove(index);
      } else {
        _selectedIndexes.add(index);
      }

      if (_selectedIndexes.isEmpty) {
        _isMobileSelectionMode = false;
        _deactivateSelectionSession();
      } else {
        _syncSelectionSession();
      }
    });
    _emitSelectionChanged();
  }

  void _clearSelectionFromController() {
    if (!mounted) {
      return;
    }
    setState(() {
      _selectedIndexes.clear();
      _isMobileSelectionMode = false;
    });
    _deactivateSelectionSession();
    _emitSelectionChanged();
  }

  // Build
  @override
  Widget build(BuildContext context) {
    final Widget section = LayoutBuilder(
      builder: (BuildContext context, BoxConstraints constraints) {
        final bool hasBoundedHeight = constraints.maxHeight.isFinite;
        // Mirror desktop behavior: in non-paged mode, fill the constrained parent
        // and scroll inside the table viewport.
        final bool fillScrollableViewport = hasBoundedHeight && !_isChunkModeEnabled;

        final Widget tableContent = _buildMobile(
          context,
          constrainToHeight: hasBoundedHeight,
          fillScrollableViewport: fillScrollableViewport,
        );

        final Widget content = Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: hasBoundedHeight ? MainAxisSize.max : MainAxisSize.min,
          children: <Widget>[
            Text(
              widget.title,
              style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
            ),
            const SizedBox(height: 10),
            if (hasBoundedHeight)
              Expanded(child: ClipRect(child: tableContent))
            else
              tableContent,
          ],
        );

        return content;
      },
    );

    return section;
  }

  // Mobile Table Content
  Widget _buildMobile(
    BuildContext context, {
    required bool constrainToHeight,
    required bool fillScrollableViewport,
  }) {
    final bool useInternalScroll = fillScrollableViewport || constrainToHeight;

    if (widget.isLoading) {
      // Keep loading skeletons scrollable inside bounded viewports to avoid overflow.
      final Widget body = ListView.builder(
        primary: false,
        shrinkWrap: !useInternalScroll,
        physics: useInternalScroll ? const ClampingScrollPhysics() : const NeverScrollableScrollPhysics(),
        addAutomaticKeepAlives: false,
        addRepaintBoundaries: true,
        addSemanticIndexes: false,
        itemCount: widget.skeletonRowCount,
        itemExtent: _mobileRowExtent,
        itemBuilder: (BuildContext context, int index) {
          return _MobileTableRowSkeleton(isFirst: index == 0);
        },
      );

      return Column(
        mainAxisSize: useInternalScroll ? MainAxisSize.max : MainAxisSize.min,
        children: <Widget>[
          if (useInternalScroll)
            Expanded(child: body)
          else
            body,
        ],
      );
    }

    if (widget.items.isEmpty) {
      final textStyles = context.mashinTableTextStyles;
      return Padding(
        padding: const EdgeInsets.all(16),
        child: Align(
          alignment: Alignment.centerLeft,
          child: Text(
            widget.emptyText,
            style: textStyles.emptyState,
          ),
        ),
      );
    }

    if (_isChunkModeEnabled) {
      final int chunkCount = (widget.items.length / _chunkSize).ceil();
      final double pageHeight = _chunkSize * _mobileRowExtent;

      return SizedBox(
        height: pageHeight,
        child: PageView.builder(
          controller: _mobileChunkPageController,
          padEnds: false,
          physics: const PageScrollPhysics(parent: BouncingScrollPhysics()),
          itemCount: chunkCount,
          itemBuilder: (BuildContext context, int pageIndex) {
            final int start = pageIndex * _chunkSize;
            final int end = math.min(start + _chunkSize, widget.items.length);
            final List<T> chunkItems = widget.items.sublist(start, end);

            return Padding(
              padding: EdgeInsets.only(
                right: pageIndex == chunkCount - 1 ? 0 : _mobileChunkGap,
              ),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: List<Widget>.generate(_chunkSize, (int localIndex) {
                  if (localIndex >= chunkItems.length) {
                    return const SizedBox(height: _mobileRowExtent);
                  }

                  final int itemIndex = start + localIndex;
                  return SizedBox(
                    height: _mobileRowExtent,
                    child: _MobileTableRow(
                      data: widget.rowMapper(chunkItems[localIndex], itemIndex),
                      isFirst: localIndex == 0,
                      isSelected: _selectedIndexes.contains(itemIndex),
                      selectionModeActive: _isMobileSelectionMode,
                      selectionEnabled: widget.enableSelection,
                      onToggleSelected: () => _toggleSelectedIndex(itemIndex),
                      onLongPressSelect: () => _enterSelectionMode(itemIndex),
                    ),
                  );
                }),
              ),
            );
          },
        ),
      );
    }

    final Widget list = ListView.builder(
      primary: false,
      shrinkWrap: !useInternalScroll,
      physics: useInternalScroll ? const ClampingScrollPhysics() : const NeverScrollableScrollPhysics(),
      addAutomaticKeepAlives: false,
      addRepaintBoundaries: true,
      addSemanticIndexes: false,
      itemCount: widget.items.length,
      itemExtent: _mobileRowExtent,
      itemBuilder: (BuildContext context, int index) {
        final int itemIndex = index;
        return _MobileTableRow(
          data: widget.rowMapper(widget.items[index], itemIndex),
          isFirst: index == 0,
          isSelected: _selectedIndexes.contains(itemIndex),
          selectionModeActive: _isMobileSelectionMode,
          selectionEnabled: widget.enableSelection,
          onToggleSelected: () => _toggleSelectedIndex(itemIndex),
          onLongPressSelect: () => _enterSelectionMode(itemIndex),
        );
      },
    );

    return list;
  }
}

class _MobileTableRow extends StatefulWidget {
  final MediaTableRowData data;
  final bool isFirst;
  final bool isSelected;
  final bool selectionModeActive;
  final bool selectionEnabled;
  final VoidCallback onToggleSelected;
  final VoidCallback onLongPressSelect;

  const _MobileTableRow({
    required this.data,
    required this.isFirst,
    required this.isSelected,
    required this.selectionModeActive,
    required this.selectionEnabled,
    required this.onToggleSelected,
    required this.onLongPressSelect,
  });

  @override
  State<_MobileTableRow> createState() => _MobileTableRowState();
}

class _MobileTableRowState extends State<_MobileTableRow> {
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
        onLongPress: widget.selectionEnabled ? widget.onLongPressSelect : null,
        onTap: widget.selectionModeActive ? widget.onToggleSelected : null,
        child: Container(
          constraints: const BoxConstraints(minHeight: 64),
          decoration: BoxDecoration(
            color: widget.isSelected ? colors.hover : (_isHovered ? colors.hover : Colors.transparent),
            border: widget.isFirst ? null : Border(top: BorderSide(color: colors.border)),
          ),
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
          child: Row(
            children: <Widget>[
              MediaTableCover(
                coverUrl: widget.data.coverUrl,
                fallbackIcon: widget.data.fallbackIcon,
                size: 52,
                radius: 8,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: <Widget>[
                    Text(
                      widget.data.title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: textStyles.mobileTitle,
                    ),
                    const SizedBox(height: 1),
                    Text(
                      widget.data.subtitle,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: textStyles.mobileSubtitle,
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 0),
              SizedBox(
                width: 52,
                child: Text(
                  widget.data.trailing,
                  textAlign: TextAlign.right,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: textStyles.mobileTrailing,
                ),
              ),
              const SizedBox(width: 16),
              SizedBox(
                width: 20,
                child: MediaProviderIcon(
                  providerLabel: widget.data.providerLabel,
                  iconSvg: widget.data.providerIconSvg,
                  size: 16,
                ),
              ),
              const SizedBox(width: 4),
              SizedBox(
                width: 40,
                height: 40,
                child: Center(
                  child: widget.selectionEnabled && widget.selectionModeActive
                      ? _MobileSelectionCheckBox(
                          isChecked: widget.isSelected,
                          isVisible: true,
                          onTap: widget.onToggleSelected,
                        )
                      : Icon(
                          Icons.more_vert_rounded,
                          size: 18,
                          color: colors.textSecondary,
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

class _MobileTableRowSkeleton extends StatelessWidget {
  final bool isFirst;

  const _MobileTableRowSkeleton({required this.isFirst});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    Widget skeletonBox({required double width, required double height, double radius = 6}) {
      return Container(
        width: width,
        height: height,
        decoration: BoxDecoration(
          color: colors.bgTertiary,
          borderRadius: BorderRadius.circular(radius),
        ),
      );
    }

    return Container(
      constraints: const BoxConstraints(minHeight: 64),
      decoration: BoxDecoration(
        color: Colors.transparent,
        border: isFirst ? null : Border(top: BorderSide(color: colors.border)),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
      child: Row(
        children: <Widget>[
          skeletonBox(width: 52, height: 52, radius: 8),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                skeletonBox(width: 190, height: 16),
                const SizedBox(height: 6),
                skeletonBox(width: 140, height: 13),
              ],
            ),
          ),
          const SizedBox(width: 8),
          skeletonBox(width: 32, height: 20, radius: 4),
          const SizedBox(width: 8),
          skeletonBox(width: 18, height: 18, radius: 4),
          const SizedBox(width: 8),
          skeletonBox(width: 20, height: 20, radius: 10),
        ],
      ),
    );
  }
}

class _MobileSelectionCheckBox extends StatelessWidget {
  final bool isChecked;
  final bool isVisible;
  final VoidCallback? onTap;

  const _MobileSelectionCheckBox({
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
              borderRadius: BorderRadius.circular(10),
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
