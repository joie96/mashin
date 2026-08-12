import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';

// #region Core Section

class MediaTableSection<T> extends StatefulWidget {
  final String title;
  final List<T> items;
  final bool isLoading;
  final String emptyText;
  final MediaTableRowData Function(T item, int index) rowMapper;
  final List<DesktopTableColumn<T>>? desktopColumns;
  final int skeletonRowCount;
  final double desktopBreakpoint;
  final bool enableSelection;
  final ValueChanged<List<T>>? onSelectionChanged;
  final VoidCallback? onSelectionMenuTap;

  const MediaTableSection({
    super.key,
    required this.title,
    required this.items,
    required this.isLoading,
    required this.emptyText,
    required this.rowMapper,
    this.desktopColumns,
    this.skeletonRowCount = 8,
    this.desktopBreakpoint = 920,
    this.enableSelection = true,
    this.onSelectionChanged,
    this.onSelectionMenuTap,
  });

  @override
  State<MediaTableSection<T>> createState() => _MediaTableSectionState<T>();
}

class _MediaTableSectionState<T> extends State<MediaTableSection<T>> {
  final Set<int> _selectedIndexes = <int>{};
  bool _isMobileSelectionMode = false;

  @override
  void didUpdateWidget(covariant MediaTableSection<T> oldWidget) {
    super.didUpdateWidget(oldWidget);

    // Keep selection stable while preventing out-of-range indices when data updates.
    _selectedIndexes.removeWhere((int index) => index < 0 || index >= widget.items.length);

    if (_selectedIndexes.isEmpty) {
      _isMobileSelectionMode = false;
    }
  }

  // #region Selection State

  bool get _isAllSelected => widget.items.isNotEmpty && _selectedIndexes.length == widget.items.length;

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

  void _toggleDesktopSelectedIndex(int index) {
    if (!widget.enableSelection) {
      return;
    }

    setState(() {
      if (_selectedIndexes.contains(index)) {
        _selectedIndexes.remove(index);
      } else {
        _selectedIndexes.add(index);
      }
    });
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
          ..addAll(List<int>.generate(widget.items.length, (int index) => index));
      }
    });
    _emitSelectionChanged();
  }

  void _toggleDesktopSelectAll() {
    _toggleAllSelectedIndexes();
  }

  void _enterMobileSelectionMode(int index) {
    if (!widget.enableSelection || index < 0 || index >= widget.items.length) {
      return;
    }

    setState(() {
      _isMobileSelectionMode = true;
      _selectedIndexes.add(index);
    });
    _emitSelectionChanged();
  }

  void _toggleMobileSelectedIndex(int index) {
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
      }
    });
    _emitSelectionChanged();
  }

  void _clearMobileSelectionMode() {
    setState(() {
      _selectedIndexes.clear();
      _isMobileSelectionMode = false;
    });
    _emitSelectionChanged();
  }

  // #endregion

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return LayoutBuilder(
      builder: (BuildContext context, BoxConstraints constraints) {
        final isDesktop =
            constraints.maxWidth >= widget.desktopBreakpoint && widget.desktopColumns != null && widget.desktopColumns!.isNotEmpty;

        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Text(
              widget.title,
              style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
            ),
            const SizedBox(height: 10),
            isDesktop ? _buildDesktop(context, colors) : _buildMobile(context),
          ],
        );
      },
    );
  }

  Widget _buildMobile(BuildContext context) {
    if (widget.isLoading) {
      return Column(
        children: List<Widget>.generate(
          widget.skeletonRowCount,
          (int index) => _MobileTableRowSkeleton(isFirst: index == 0),
        ),
      );
    }

    if (widget.items.isEmpty) {
      final colors = context.mashinColors;
      return Padding(
        padding: const EdgeInsets.all(16),
        child: Align(
          alignment: Alignment.centerLeft,
          child: Text(
            widget.emptyText,
            style: TextStyle(
              color: colors.textSecondary,
              fontFamily: 'PoppinsRegular',
              fontSize: 14,
            ),
          ),
        ),
      );
    }

    return Stack(
      children: <Widget>[
        Column(
          children: <Widget>[
            ...widget.items
                .asMap()
                .entries
                .map(
                  (entry) => _MobileTableRow(
                    data: widget.rowMapper(entry.value, entry.key),
                    isFirst: entry.key == 0,
                    isSelected: _selectedIndexes.contains(entry.key),
                    selectionModeActive: _isMobileSelectionMode,
                    selectionEnabled: widget.enableSelection,
                    onToggleSelected: () => _toggleMobileSelectedIndex(entry.key),
                    onLongPressSelect: () => _enterMobileSelectionMode(entry.key),
                  ),
                )
                .toList(),
            if (_isMobileSelectionMode) const SizedBox(height: 76),
          ],
        ),
        if (_isMobileSelectionMode)
          Positioned(
            right: 8,
            bottom: 8,
            child: _MobileSelectionOverlay(
              allSelected: _isAllSelected,
              onToggleAll: _toggleMobileSelectAll,
              onMenuTap: widget.onSelectionMenuTap,
              onClose: _clearMobileSelectionMode,
            ),
          ),
      ],
    );
  }

  void _toggleMobileSelectAll() {
    _toggleAllSelectedIndexes();
  }

  Widget _buildDesktop(BuildContext context, MashinColors colors) {
    final columns = widget.desktopColumns!;

    if (widget.isLoading) {
      return Column(
        children: <Widget>[
          _DesktopTableHeader<T>(
            columns: columns,
            showSelection: widget.enableSelection,
            isAllSelected: _isAllSelected,
            onToggleSelectAll: _toggleDesktopSelectAll,
          ),
          ...List<Widget>.generate(
            widget.skeletonRowCount,
            (int index) => _DesktopTableRowSkeleton<T>(
              columns: columns,
              isFirst: index == 0,
              showSelection: widget.enableSelection,
            ),
          ),
        ],
      );
    }

    if (widget.items.isEmpty) {
      return Column(
        children: <Widget>[
          _DesktopTableHeader<T>(
            columns: columns,
            showSelection: widget.enableSelection,
            isAllSelected: false,
            onToggleSelectAll: _toggleDesktopSelectAll,
          ),
          Padding(
            padding: const EdgeInsets.all(16),
            child: Align(
              alignment: Alignment.centerLeft,
              child: Text(
                widget.emptyText,
                style: TextStyle(
                  color: colors.textSecondary,
                  fontFamily: 'PoppinsRegular',
                  fontSize: 14,
                ),
              ),
            ),
          ),
        ],
      );
    }

    return Column(
      children: <Widget>[
        _DesktopTableHeader<T>(
          columns: columns,
          showSelection: widget.enableSelection,
          isAllSelected: _isAllSelected,
          onToggleSelectAll: _toggleDesktopSelectAll,
        ),
        ...widget.items.asMap().entries.map(
              (entry) => _DesktopTableRow<T>(
                columns: columns,
                item: entry.value,
                index: entry.key,
                isFirst: entry.key == 0,
                isSelected: _selectedIndexes.contains(entry.key),
                selectionEnabled: widget.enableSelection,
                onToggleSelected: () => _toggleDesktopSelectedIndex(entry.key),
              ),
            ),
      ],
    );
  }
}

// #endregion

// #region Desktop Table Schema

enum DesktopTextTone {
  primary,
  secondary,
  tertiary,
}

class DesktopTableColumn<T> {
  final String header;
  final int? flex;
  final double? width;
  final TextAlign textAlign;
  final EdgeInsetsGeometry padding;
  final DesktopTextTone textTone;
  final double? fontSize;
  final String Function(T item, int index)? valueBuilder;
  final Widget Function(BuildContext context, T item, int index)? childBuilder;

  const DesktopTableColumn({
    required this.header,
    required this.textAlign,
    this.padding = EdgeInsets.zero,
    this.textTone = DesktopTextTone.secondary,
    this.fontSize,
    this.flex,
    this.width,
    this.valueBuilder,
    this.childBuilder,
  }) : assert(
          (flex != null && width == null) || (flex == null && width != null),
          'Either flex or width must be set, but not both.',
        ),
        assert(
          (valueBuilder != null) != (childBuilder != null),
          'Either valueBuilder or childBuilder must be set.',
        );
}

// #endregion

// #region Shared Row DTO

class MediaTableRowData {
  final String title;
  final String subtitle;
  final String trailing;
  final String? coverUrl;
  final IconData fallbackIcon;
  final String? providerLabel;
  final String? providerIconSvg;

  const MediaTableRowData({
    required this.title,
    required this.subtitle,
    required this.trailing,
    required this.fallbackIcon,
    this.coverUrl,
    this.providerLabel,
    this.providerIconSvg,
  });
}

// #endregion

// #region Mobile Rendering

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
              _TableCover(
                coverUrl: widget.data.coverUrl,
                fallbackIcon: widget.data.fallbackIcon,
                size: 52,
                radius: 8,
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: <Widget>[
                    Text(
                      widget.data.title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                            fontFamily: 'PoppinsRegular',
                            fontSize: 14,
                            color: colors.textPrimary,
                          ),
                    ),
                    const SizedBox(height: 1),
                    Text(
                      widget.data.subtitle,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                            fontFamily: 'PoppinsRegular',
                            fontSize: 12,
                            color: colors.textSecondary,
                          ),
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 8),
              SizedBox(
                width: 52,
                child: Text(
                  widget.data.trailing,
                  textAlign: TextAlign.right,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                        fontFamily: 'PoppinsRegular',
                        fontSize: 12,
                        color: colors.textSecondary,
                      ),
                ),
              ),
              const SizedBox(width: 8),
              SizedBox(
                width: 20,
                child: _ProviderIcon(
                  providerLabel: widget.data.providerLabel,
                  iconSvg: widget.data.providerIconSvg,
                  size: 16,
                ),
              ),
              SizedBox(width: widget.selectionEnabled && widget.selectionModeActive ? 10 : 4),
              if (widget.selectionEnabled && widget.selectionModeActive)
                SizedBox(
                  width: 24,
                  child: Align(
                    alignment: Alignment.center,
                    child: _MobileSelectionCheckBox(
                      isChecked: widget.isSelected,
                      isVisible: true,
                      onTap: widget.onToggleSelected,
                    ),
                  ),
                )
              else
                SizedBox(
                  width: 40,
                  height: 40,
                  child: Center(
                    child: Icon(
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

// #endregion

// #region Desktop Rendering

class _DesktopTableHeader<T> extends StatelessWidget {
  final List<DesktopTableColumn<T>> columns;
  final bool showSelection;
  final bool isAllSelected;
  final VoidCallback onToggleSelectAll;

  const _DesktopTableHeader({
    required this.columns,
    required this.showSelection,
    required this.isAllSelected,
    required this.onToggleSelectAll,
  });

  @override
  Widget build(BuildContext context) {
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
          ...columns
            .map(
              (column) => _desktopCellWidget(
                flex: column.flex,
                width: column.width,
                child: Padding(
                  padding: column.padding,
                  child: Text(
                    column.header,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    textAlign: column.textAlign,
                    style: Theme.of(context).textTheme.labelSmall?.copyWith(
                          fontFamily: 'PoppinsRegular',
                          color: colors.textSecondary,
                          fontWeight: FontWeight.w700,
                          fontSize: 12,
                        ),
                  ),
                ),
              ),
            )
            .toList(),
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

Color _desktopToneColor(MashinColors colors, DesktopTextTone tone) {
  switch (tone) {
    case DesktopTextTone.primary:
      return colors.textPrimary;
    case DesktopTextTone.secondary:
      return colors.textSecondary;
    case DesktopTextTone.tertiary:
      return colors.textTertiary;
  }
}

class _DesktopTableRow<T> extends StatefulWidget {
  final List<DesktopTableColumn<T>> columns;
  final T item;
  final int index;
  final bool isFirst;
  final bool isSelected;
  final bool selectionEnabled;
  final VoidCallback onToggleSelected;

  const _DesktopTableRow({
    required this.columns,
    required this.item,
    required this.index,
    required this.isFirst,
    required this.isSelected,
    required this.selectionEnabled,
    required this.onToggleSelected,
  });

  @override
  State<_DesktopTableRow<T>> createState() => _DesktopTableRowState<T>();
}

class _DesktopTableRowState<T> extends State<_DesktopTableRow<T>> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: Container(
        height: 64,
        decoration: BoxDecoration(
          color: widget.isSelected ? colors.hover : (_isHovered ? colors.hover : Colors.transparent),
          border: widget.isFirst ? null : Border(top: BorderSide(color: colors.border)),
        ),
        child: Row(
          children: <Widget>[
            ...widget.columns
              .map(
                (column) => _desktopCellWidget(
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
                            style: Theme.of(context).textTheme.bodySmall?.copyWith(
                                  fontFamily: 'PoppinsRegular',
                                  color: _desktopToneColor(colors, column.textTone),
                                  fontSize: column.fontSize ?? 13,
                                ),
                          ),
                  ),
                ),
              )
              .toList(),
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
          alignment: _alignForText(column.textAlign),
          child: bar(
            width: (column.width! * 0.5).clamp(16, 60),
            height: 14,
          ),
        );
      }

      return Align(
        alignment: _alignForText(column.textAlign),
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
          ...columns
            .asMap()
            .entries
            .map(
              (entry) => _desktopCellWidget(
                flex: entry.value.flex,
                width: entry.value.width,
                child: Padding(
                  padding: entry.value.padding,
                  child: skeletonFor(entry.value, entry.key),
                ),
              ),
            )
            .toList(),
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

// #endregion

// #region Selection Controls

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

class _MobileSelectionOverlay extends StatelessWidget {
  final bool allSelected;
  final VoidCallback onToggleAll;
  final VoidCallback? onMenuTap;
  final VoidCallback onClose;

  const _MobileSelectionOverlay({
    required this.allSelected,
    required this.onToggleAll,
    required this.onMenuTap,
    required this.onClose,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    Widget actionButton({required IconData icon, required VoidCallback? onTap}) {
      return Container(
        width: 40,
        height: 40,
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(20),
          border: Border.all(color: colors.border),
        ),
        child: InkWell(
          borderRadius: BorderRadius.circular(20),
          onTap: onTap,
          child: Icon(icon, size: 18, color: colors.textPrimary),
        ),
      );
    }

    return Material(
      color: Colors.transparent,
      child: Container(
        padding: const EdgeInsets.all(6),
        decoration: BoxDecoration(
          color: colors.bgSecondary,
          borderRadius: BorderRadius.circular(16),
          border: Border.all(color: colors.border),
          boxShadow: <BoxShadow>[
            BoxShadow(
              color: Colors.black.withValues(alpha: 0.15),
              blurRadius: 12,
              offset: const Offset(0, 4),
            ),
          ],
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            actionButton(icon: allSelected ? Icons.deselect_rounded : Icons.select_all_rounded, onTap: onToggleAll),
            const SizedBox(width: 6),
            actionButton(icon: Icons.more_vert_rounded, onTap: onMenuTap),
            const SizedBox(width: 6),
            actionButton(icon: Icons.close_rounded, onTap: onClose),
          ],
        ),
      ),
    );
  }
}

// #endregion

// #region Shared Presentation Helpers

class _TableCover extends StatelessWidget {
  final String? coverUrl;
  final IconData fallbackIcon;
  final double size;
  final double radius;

  const _TableCover({
    required this.coverUrl,
    required this.fallbackIcon,
    required this.size,
    required this.radius,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final resolved = (coverUrl ?? '').trim();

    return ClipRRect(
      borderRadius: BorderRadius.circular(radius),
      child: Container(
        width: size,
        height: size,
        color: colors.bgTertiary,
        child: resolved.isEmpty
            ? Icon(fallbackIcon, size: size * 0.45, color: colors.accent)
            : Image.network(
                resolved,
                fit: BoxFit.cover,
                errorBuilder: (_, _, _) => Icon(fallbackIcon, size: size * 0.45, color: colors.accent),
              ),
      ),
    );
  }
}

class _ProviderIcon extends StatelessWidget {
  final String? providerLabel;
  final String? iconSvg;
  final double size;

  const _ProviderIcon({
    required this.providerLabel,
    required this.iconSvg,
    this.size = 16,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final src = (iconSvg ?? '').trim();
    final lower = src.toLowerCase();

    if (src.isNotEmpty) {
      if (lower.startsWith('data:image/svg+xml;base64,')) {
        final base64Data = src.substring('data:image/svg+xml;base64,'.length);
        return SvgPicture.memory(
          UriData.parse('data:image/svg+xml;base64,$base64Data').contentAsBytes(),
          width: size,
          height: size,
          placeholderBuilder: (_) => _providerFallback(providerLabel, colors, size),
        );
      }

      if (lower.startsWith('data:image/svg+xml,')) {
        final raw = Uri.decodeComponent(src.substring('data:image/svg+xml,'.length));
        return SvgPicture.string(
          raw,
          width: size,
          height: size,
          placeholderBuilder: (_) => _providerFallback(providerLabel, colors, size),
        );
      }

      if (lower.startsWith('data:image/svg+xml;utf8,')) {
        final raw = Uri.decodeComponent(src.substring('data:image/svg+xml;utf8,'.length));
        return SvgPicture.string(
          raw,
          width: size,
          height: size,
          placeholderBuilder: (_) => _providerFallback(providerLabel, colors, size),
        );
      }

      if (lower.startsWith('<?xml') || lower.startsWith('<svg')) {
        return SvgPicture.string(
          src,
          width: size,
          height: size,
          placeholderBuilder: (_) => _providerFallback(providerLabel, colors, size),
        );
      }

      if (lower.startsWith('http://') || lower.startsWith('https://')) {
        return SvgPicture.network(
          src,
          width: size,
          height: size,
          placeholderBuilder: (_) => _providerFallback(providerLabel, colors, size),
        );
      }
    }

    return _providerFallback(providerLabel, colors, size);
  }

  Widget _providerFallback(String? provider, MashinColors colors, double size) {
    return Text(
      _providerShort(provider),
      textAlign: TextAlign.center,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: TextStyle(
        fontFamily: 'PoppinsRegular',
        color: colors.textSecondary,
        fontWeight: FontWeight.w700,
        fontSize: size <= 16 ? 11 : 12,
      ),
    );
  }
}

Widget _desktopCellWidget({
  required Widget child,
  int? flex,
  double? width,
}) {
  if (flex != null) {
    return Expanded(flex: flex, child: child);
  }

  return SizedBox(width: width!, child: child);
}

Alignment _alignForText(TextAlign align) {
  switch (align) {
    case TextAlign.right:
      return Alignment.centerRight;
    case TextAlign.center:
      return Alignment.center;
    default:
      return Alignment.centerLeft;
  }
}

// #endregion

// #region Concrete Sections

class TrackTableSection extends StatelessWidget {
  final List<MaTrack> tracks;
  final bool isLoading;

  const TrackTableSection({
    super.key,
    required this.tracks,
    required this.isLoading,
  });

  @override
  Widget build(BuildContext context) {
    return MediaTableSection<MaTrack>(
      title: 'Tracks',
      isLoading: isLoading,
      items: tracks,
      emptyText: 'Keine Tracks gefunden.',
      rowMapper: (MaTrack track, int _) => MediaTableRowData(
        title: track.name,
        subtitle: firstArtistName(track.artists),
        trailing: formatDuration(track.duration),
        coverUrl: _sizedTrackImage(track, 80),
        fallbackIcon: Icons.music_note_rounded,
        providerLabel: track.provider,
        providerIconSvg: MaProviderManifestRegistry.iconSvgForProvider(track.provider),
      ),
      desktopColumns: <DesktopTableColumn<MaTrack>>[
        DesktopTableColumn<MaTrack>(
          header: '#',
          width: 40,
          textAlign: TextAlign.center,
          textTone: DesktopTextTone.tertiary,
          valueBuilder: (_, index) => '${index + 1}',
        ),
        DesktopTableColumn<MaTrack>(
          header: '',
          width: 48,
          textAlign: TextAlign.center,
          childBuilder: (BuildContext context, MaTrack track, int _) => _TableCover(
            coverUrl: _sizedTrackImage(track, 80),
            fallbackIcon: Icons.music_note_rounded,
            size: 48,
            radius: 6,
          ),
        ),
        DesktopTableColumn<MaTrack>(
          header: 'TITEL',
          flex: 5,
          textAlign: TextAlign.left,
          padding: const EdgeInsets.only(left: 12),
          textTone: DesktopTextTone.secondary,
          fontSize: 15,
          valueBuilder: (track, _) => track.name,
        ),
        DesktopTableColumn<MaTrack>(
          header: 'DAUER',
          width: 80,
          textAlign: TextAlign.center,
          textTone: DesktopTextTone.tertiary,
          valueBuilder: (track, _) => formatDuration(track.duration),
        ),
        DesktopTableColumn<MaTrack>(
          header: 'ALBUM',
          flex: 3,
          textAlign: TextAlign.left,
          textTone: DesktopTextTone.tertiary,
          fontSize: 14,
          valueBuilder: (track, _) => track.album?.name ?? '-',
        ),
        DesktopTableColumn<MaTrack>(
          header: 'ARTIST',
          flex: 3,
          textAlign: TextAlign.left,
          textTone: DesktopTextTone.tertiary,
          fontSize: 14,
          valueBuilder: (track, _) => firstArtistName(track.artists),
        ),
        DesktopTableColumn<MaTrack>(
          header: 'JAHR',
          width: 56,
          textAlign: TextAlign.center,
          textTone: DesktopTextTone.tertiary,
          valueBuilder: (track, _) => track.album?.year?.toString() ?? '-',
        ),
        DesktopTableColumn<MaTrack>(
          header: 'PROVIDER',
          width: 80,
          textAlign: TextAlign.center,
          childBuilder: (BuildContext context, MaTrack track, int _) => Center(
            child: _ProviderIcon(
              providerLabel: track.provider,
              iconSvg: MaProviderManifestRegistry.iconSvgForProvider(track.provider),
              size: 16,
            ),
          ),
        ),
        DesktopTableColumn<MaTrack>(
          header: 'FAV',
          width: 40,
          textAlign: TextAlign.center,
          childBuilder: (BuildContext context, MaTrack _, int _) {
            final colors = context.mashinColors;
            return Icon(Icons.favorite_border_rounded, size: 16, color: colors.textTertiary);
          },
        ),
        DesktopTableColumn<MaTrack>(
          header: '',
          width: 40,
          textAlign: TextAlign.center,
          childBuilder: (BuildContext context, MaTrack _, int _) {
            final colors = context.mashinColors;
            return Icon(Icons.more_horiz_rounded, size: 18, color: colors.textTertiary);
          },
        ),
      ],
    );
  }
}

class AlbumTableSection extends StatelessWidget {
  final List<MaAlbum> albums;
  final bool isLoading;

  const AlbumTableSection({
    super.key,
    required this.albums,
    required this.isLoading,
  });

  @override
  Widget build(BuildContext context) {
    return MediaTableSection<MaAlbum>(
      title: 'Alben',
      isLoading: isLoading,
      items: albums,
      emptyText: 'Keine Alben gefunden.',
      rowMapper: (MaAlbum album, int _) => MediaTableRowData(
        title: album.name,
        subtitle: firstArtistName(album.artists),
        trailing: album.year?.toString() ?? '-',
        coverUrl: _sizedAlbumImage(album, 80),
        fallbackIcon: Icons.album_rounded,
        providerLabel: album.provider,
        providerIconSvg: MaProviderManifestRegistry.iconSvgForProvider(album.provider),
      ),
      desktopColumns: <DesktopTableColumn<MaAlbum>>[
        DesktopTableColumn<MaAlbum>(
          header: '#',
          width: 40,
          textAlign: TextAlign.center,
          textTone: DesktopTextTone.tertiary,
          valueBuilder: (_, index) => '${index + 1}',
        ),
        DesktopTableColumn<MaAlbum>(
          header: '',
          width: 48,
          textAlign: TextAlign.center,
          childBuilder: (BuildContext context, MaAlbum album, int _) => _TableCover(
            coverUrl: _sizedAlbumImage(album, 80),
            fallbackIcon: Icons.album_rounded,
            size: 48,
            radius: 6,
          ),
        ),
        DesktopTableColumn<MaAlbum>(
          header: 'TITLE',
          flex: 5,
          textAlign: TextAlign.left,
          padding: const EdgeInsets.only(left: 12),
          textTone: DesktopTextTone.secondary,
          fontSize: 15,
          valueBuilder: (album, _) => album.name,
        ),
        DesktopTableColumn<MaAlbum>(
          header: 'ARTIST',
          flex: 3,
          textAlign: TextAlign.left,
          textTone: DesktopTextTone.tertiary,
          fontSize: 14,
          valueBuilder: (album, _) => firstArtistName(album.artists),
        ),
        DesktopTableColumn<MaAlbum>(
          header: 'YEAR',
          width: 56,
          textAlign: TextAlign.center,
          textTone: DesktopTextTone.tertiary,
          valueBuilder: (album, _) => album.year?.toString() ?? '-',
        ),
        DesktopTableColumn<MaAlbum>(
          header: 'TYPE',
          flex: 2,
          textAlign: TextAlign.left,
          textTone: DesktopTextTone.tertiary,
          valueBuilder: (album, _) => album.albumType ?? '-',
        ),
        DesktopTableColumn<MaAlbum>(
          header: 'PROVIDER',
          width: 80,
          textAlign: TextAlign.center,
          childBuilder: (BuildContext context, MaAlbum album, int _) => Center(
            child: _ProviderIcon(
              providerLabel: album.provider,
              iconSvg: MaProviderManifestRegistry.iconSvgForProvider(album.provider),
              size: 16,
            ),
          ),
        ),
        DesktopTableColumn<MaAlbum>(
          header: '',
          width: 40,
          textAlign: TextAlign.center,
          childBuilder: (BuildContext context, MaAlbum _, int _) {
            final colors = context.mashinColors;
            return Icon(Icons.more_horiz_rounded, size: 18, color: colors.textTertiary);
          },
        ),
      ],
    );
  }
}

class PlaylistTableSection extends StatelessWidget {
  final List<MaPlaylist> playlists;
  final bool isLoading;

  const PlaylistTableSection({
    super.key,
    required this.playlists,
    required this.isLoading,
  });

  @override
  Widget build(BuildContext context) {
    return MediaTableSection<MaPlaylist>(
      title: 'Playlists',
      isLoading: isLoading,
      items: playlists,
      emptyText: 'Keine Playlists gefunden.',
      rowMapper: (MaPlaylist playlist, int _) => MediaTableRowData(
        title: playlist.name,
        subtitle: _playlistSubtitle(playlist),
        trailing: playlist.isEditable ? 'Edit' : 'RO',
        coverUrl: _sizedPlaylistImage(playlist, 80),
        fallbackIcon: Icons.queue_music_rounded,
        providerLabel: playlist.provider,
        providerIconSvg: MaProviderManifestRegistry.iconSvgForProvider(playlist.provider),
      ),
      desktopColumns: <DesktopTableColumn<MaPlaylist>>[
        DesktopTableColumn<MaPlaylist>(
          header: '#',
          width: 40,
          textAlign: TextAlign.center,
          textTone: DesktopTextTone.tertiary,
          valueBuilder: (_, index) => '${index + 1}',
        ),
        DesktopTableColumn<MaPlaylist>(
          header: '',
          width: 48,
          textAlign: TextAlign.center,
          childBuilder: (BuildContext context, MaPlaylist playlist, int _) => _TableCover(
            coverUrl: _sizedPlaylistImage(playlist, 80),
            fallbackIcon: Icons.queue_music_rounded,
            size: 48,
            radius: 6,
          ),
        ),
        DesktopTableColumn<MaPlaylist>(
          header: 'TITLE',
          flex: 5,
          textAlign: TextAlign.left,
          padding: const EdgeInsets.only(left: 12),
          textTone: DesktopTextTone.secondary,
          fontSize: 15,
          valueBuilder: (playlist, _) => playlist.name,
        ),
        DesktopTableColumn<MaPlaylist>(
          header: 'OWNER',
          flex: 3,
          textAlign: TextAlign.left,
          textTone: DesktopTextTone.tertiary,
          fontSize: 14,
          valueBuilder: (playlist, _) => _playlistSubtitle(playlist),
        ),
        DesktopTableColumn<MaPlaylist>(
          header: 'STATUS',
          width: 80,
          textAlign: TextAlign.center,
          textTone: DesktopTextTone.tertiary,
          valueBuilder: (playlist, _) => playlist.isEditable ? 'EDIT' : 'RO',
        ),
        DesktopTableColumn<MaPlaylist>(
          header: 'PROVIDER',
          width: 80,
          textAlign: TextAlign.center,
          childBuilder: (BuildContext context, MaPlaylist playlist, int _) => Center(
            child: _ProviderIcon(
              providerLabel: playlist.provider,
              iconSvg: MaProviderManifestRegistry.iconSvgForProvider(playlist.provider),
              size: 16,
            ),
          ),
        ),
        DesktopTableColumn<MaPlaylist>(
          header: '',
          width: 40,
          textAlign: TextAlign.center,
          childBuilder: (BuildContext context, MaPlaylist _, int _) {
            final colors = context.mashinColors;
            return Icon(Icons.more_horiz_rounded, size: 18, color: colors.textTertiary);
          },
        ),
      ],
    );
  }
}

class ArtistTableSection extends StatelessWidget {
  final List<MaArtist> artists;
  final bool isLoading;

  const ArtistTableSection({
    super.key,
    required this.artists,
    required this.isLoading,
  });

  @override
  Widget build(BuildContext context) {
    return MediaTableSection<MaArtist>(
      title: 'Artists',
      isLoading: isLoading,
      items: artists,
      emptyText: 'Keine Artists gefunden.',
      rowMapper: (MaArtist artist, int _) => MediaTableRowData(
        title: artist.name,
        subtitle: 'Artist',
        trailing: '',
        coverUrl: _sizedArtistImage(artist, 80),
        fallbackIcon: Icons.person_rounded,
        providerLabel: artist.provider,
        providerIconSvg: MaProviderManifestRegistry.iconSvgForProvider(artist.provider),
      ),
      desktopColumns: <DesktopTableColumn<MaArtist>>[
        DesktopTableColumn<MaArtist>(
          header: '#',
          width: 40,
          textAlign: TextAlign.center,
          textTone: DesktopTextTone.tertiary,
          valueBuilder: (_, index) => '${index + 1}',
        ),
        DesktopTableColumn<MaArtist>(
          header: '',
          width: 48,
          textAlign: TextAlign.center,
          childBuilder: (BuildContext context, MaArtist artist, int _) => _TableCover(
            coverUrl: _sizedArtistImage(artist, 80),
            fallbackIcon: Icons.person_rounded,
            size: 48,
            radius: 6,
          ),
        ),
        DesktopTableColumn<MaArtist>(
          header: 'NAME',
          flex: 6,
          textAlign: TextAlign.left,
          padding: const EdgeInsets.only(left: 12),
          textTone: DesktopTextTone.secondary,
          fontSize: 15,
          valueBuilder: (artist, _) => artist.name,
        ),
        DesktopTableColumn<MaArtist>(
          header: 'PROVIDER',
          width: 80,
          textAlign: TextAlign.center,
          childBuilder: (BuildContext context, MaArtist artist, int _) => Center(
            child: _ProviderIcon(
              providerLabel: artist.provider,
              iconSvg: MaProviderManifestRegistry.iconSvgForProvider(artist.provider),
              size: 16,
            ),
          ),
        ),
        DesktopTableColumn<MaArtist>(
          header: '',
          width: 40,
          textAlign: TextAlign.center,
          childBuilder: (BuildContext context, MaArtist _, int _) {
            final colors = context.mashinColors;
            return Icon(Icons.more_horiz_rounded, size: 18, color: colors.textTertiary);
          },
        ),
      ],
    );
  }
}

// #endregion

// #region Mapping Helpers

String _providerShort(String? provider) {
  final value = (provider ?? '').trim();
  if (value.isEmpty) {
    return '';
  }

  final domain = value.split('--').first.trim();
  if (domain.isEmpty) {
    return '';
  }

  return domain.length <= 2 ? domain.toUpperCase() : domain.substring(0, 2).toUpperCase();
}

String _playlistSubtitle(MaPlaylist playlist) {
  final owner = playlist.owner?.trim() ?? '';
  if (owner.isNotEmpty) {
    return owner;
  }
  return 'Playlist';
}

String? _sizedAlbumImage(MaAlbum album, int size) {
  return album.primaryImage?.getProxyPath(
    checksum: album.metadata?.cacheChecksum,
    size: size,
  );
}

String? _sizedArtistImage(MaArtist artist, int size) {
  return artist.primaryImage?.getProxyPath(
    checksum: artist.metadata?.cacheChecksum,
    size: size,
  );
}

String? _sizedPlaylistImage(MaPlaylist playlist, int size) {
  return playlist.primaryImage?.getProxyPath(
    checksum: playlist.metadata?.cacheChecksum,
    size: size,
  );
}

String? _sizedTrackImage(MaTrack track, int size) {
  final albumImage = track.album?.primaryImage;
  if (albumImage != null) {
    return albumImage.getProxyPath(
      checksum: track.album?.metadata?.cacheChecksum,
      size: size,
    );
  }

  return track.primaryImage?.getProxyPath(
    checksum: track.metadata?.cacheChecksum,
    size: size,
  );
}

// #endregion

// #region Shared Formatters

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

  final duration = Duration(seconds: seconds);
  final minutes = duration.inMinutes;
  final remainingSeconds = duration.inSeconds % 60;

  return '$minutes:${remainingSeconds.toString().padLeft(2, '0')}';
}

// #endregion
