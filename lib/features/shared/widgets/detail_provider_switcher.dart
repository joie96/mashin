import 'dart:async';

import 'package:flutter/material.dart';

import '../../../app/layout/layout_utils.dart';
import '../../core/core_models.dart';
import '../item_provider_selection.dart';
import '../detail_provider_selection.dart';
import 'media_table_section_desktop.dart';

class DetailProviderSwitcher extends StatefulWidget {
  const DetailProviderSwitcher({
    super.key,
    required this.selection,
    required this.onSelected,
    required this.isLoading,
    this.item,
  });

  final DetailProviderSelection selection;
  final Future<void> Function(ProviderMapping) onSelected;
  final bool isLoading;
  final MediaItem? item;

  @override
  State<DetailProviderSwitcher> createState() => _DetailProviderSwitcherState();
}

class _DetailProviderSwitcherState extends State<DetailProviderSwitcher> {
  static const double _selectedIconSize = 24;
  static const double _buttonHeight = 48;
  static const int _visibleProviderCount = 3;
  static const double _contentWidth =
      _selectedIconSize +
      (_visibleProviderCount - 1) * _ProviderIconStack.iconOffset;
  static const double _buttonWidth = _contentWidth + 16;

  bool _isSelecting = false;

  // Keep duplicate taps out while the preference and section requests complete.
  Future<void> _select(ProviderMapping mapping) async {
    if (_isSelecting || widget.isLoading) return;
    setState(() => _isSelecting = true);
    try {
      await widget.onSelected(mapping);
    } catch (_) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Provider konnte nicht gewechselt werden.'),
          ),
        );
      }
    } finally {
      if (mounted) setState(() => _isSelecting = false);
    }
  }

  // The original item is only used for an unmapped provider's existing icon.
  @override
  Widget build(BuildContext context) {
    final List<ProviderMapping> mappings = widget.selection.mappings;
    if (mappings.isEmpty) {
      final String? provider = widget.item?.provider;
      if (provider == null || provider == 'library') {
        return const SizedBox.shrink();
      }
      return Tooltip(
        message:
            ProviderManifestRegistry.getByProvider(provider)?.name ?? provider,
        child: SizedBox(
          width: isCompactLayout(context) ? _buttonWidth : 40,
          height: _buttonHeight,
          child: Padding(
            padding: const EdgeInsets.only(left: 8),
            child: Align(
              alignment: Alignment.centerLeft,
              child: MediaProviderIcon(
                providerLabel: provider,
                iconSvg: ProviderManifestRegistry.iconSvgForProvider(provider),
                size: _selectedIconSize,
              ),
            ),
          ),
        ),
      );
    }
    if (!isCompactLayout(context)) {
      return _DesktopProviderRow(
        mappings: mappings,
        selectedProvider: widget.selection.selectedProvider,
        isEnabled: !widget.isLoading && !_isSelecting,
        onSelected: _select,
      );
    }
    return _buildSwitcher(mappings);
  }

  // A single button advances through the providers shown behind the active icon.
  Widget _buildSwitcher(List<ProviderMapping> mappings) {
    final List<ProviderMapping> availableMappings = mappings
        .where((mapping) => mapping.available)
        .toList(growable: false);
    final List<ProviderMapping> visibleMappings =
        (availableMappings.isEmpty ? mappings : availableMappings)
            .take(_visibleProviderCount)
            .toList(growable: false);
    final ProviderMapping? next = widget.selection.nextProvider;
    final String activeLabel = _providerLabel(visibleMappings.first);

    return Semantics(
      value: activeLabel,
      child: IconButton(
        key: const ValueKey<String>('detail-provider-cycle'),
        tooltip: next == null
            ? activeLabel
            : 'Zu ${_providerLabel(next)} wechseln',
        onPressed: next == null || widget.isLoading || _isSelecting
            ? null
            : () => _select(next),
        style:
            IconButton.styleFrom(
              fixedSize: const Size(_buttonWidth, _buttonHeight),
              enabledMouseCursor: SystemMouseCursors.click,
              disabledMouseCursor: SystemMouseCursors.basic,
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
              backgroundColor: Colors.transparent,
              disabledBackgroundColor: Colors.transparent,
              side: BorderSide.none,
            ).copyWith(
              overlayColor: const WidgetStatePropertyAll<Color>(
                Colors.transparent,
              ),
            ),
        icon: ExcludeSemantics(
          child: _ProviderIconStack(
            mappings: visibleMappings,
            width: _contentWidth,
          ),
        ),
      ),
    );
  }

  // Provider labels.
  String _providerLabel(ProviderMapping mapping) =>
      ProviderManifestRegistry.getByProvider(mapping.providerDomain)?.name ??
      mapping.providerDomain;
}

/// Cycles a loaded item's providers without changing the global preference.
class MediaItemProviderIcon extends StatefulWidget {
  const MediaItemProviderIcon({
    super.key,
    required this.item,
    this.size = 16,
    this.primaryIconOffset,
  });

  final MediaItem item;
  final double size;
  final Offset? primaryIconOffset;
  static final Set<int> _providerPointers = <int>{};

  /// Raw selection listeners must ignore provider-button pointer dispatches.
  static bool handlesPointer(PointerDownEvent event) =>
      _providerPointers.contains(event.pointer);

  @override
  State<MediaItemProviderIcon> createState() => _MediaItemProviderIconState();
}

class _MediaItemProviderIconState extends State<MediaItemProviderIcon> {
  static const Size _iconArea = Size(48, 48);
  static const double _iconInset = 8;
  static const double _stackIconSize = 24;
  static const double _stackHeight = 40;
  late ItemProviderSelection _selection;
  bool _isCycling = false;

  // The selection belongs to the object, so multiple visible controls share it.
  @override
  void initState() {
    super.initState();
    _selection = ItemProviderSelection.forItem(widget.item);
  }

  @override
  void didUpdateWidget(covariant MediaItemProviderIcon oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.item, widget.item)) {
      _selection = ItemProviderSelection.forItem(widget.item);
    }
  }

  Future<void> _cycle() async {
    if (_isCycling) return;
    setState(() => _isCycling = true);
    try {
      await _selection.cycle();
    } catch (_) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Provider konnte nicht gewechselt werden.'),
          ),
        );
      }
    } finally {
      if (mounted) setState(() => _isCycling = false);
    }
  }

  @override
  Widget build(BuildContext context) => FutureBuilder<void>(
    future: _selection.initialize(),
    builder: (context, snapshot) => ListenableBuilder(
      listenable: _selection,
      builder: (context, child) {
        final mappings = _selection.mappings
            .where((mapping) => mapping.available)
            .toList();
        final String provider =
            mappings.firstOrNull?.providerDomain ?? widget.item.provider;
        final String label =
            ProviderManifestRegistry.getByProvider(provider)?.name ?? provider;
        final primaryIconOffset =
            widget.primaryIconOffset ??
            Offset(_iconInset, (_iconArea.height - widget.size) / 2);
        if (!_selection.canCycle) {
          return SizedBox.fromSize(
            size: _iconArea,
            child: Stack(
              children: <Widget>[
                Positioned(
                  left: primaryIconOffset.dx,
                  top: primaryIconOffset.dy,
                  child: MediaProviderIcon(
                    key: const ValueKey<String>('item-provider-primary'),
                    providerLabel: label,
                    iconSvg: ProviderManifestRegistry.iconSvgForProvider(
                      provider,
                    ),
                    size: widget.size,
                  ),
                ),
              ],
            ),
          );
        }
        final visible = mappings.take(3).toList();
        final String next =
            ProviderManifestRegistry.getByProvider(
              mappings[1].providerDomain,
            )?.name ??
            mappings[1].providerDomain;
        return Listener(
          behavior: HitTestBehavior.opaque,
          onPointerDown: (event) {
            MediaItemProviderIcon._providerPointers.add(event.pointer);
            scheduleMicrotask(
              () =>
                  MediaItemProviderIcon._providerPointers.remove(event.pointer),
            );
          },
          child: GestureDetector(
            onDoubleTap: _cycle,
            onLongPress: () {},
            child: Semantics(
              value: label,
              child: IconButton(
                tooltip: 'Zu $next wechseln',
                onPressed: _isCycling || snapshot.hasError ? null : _cycle,
                style:
                    IconButton.styleFrom(
                      fixedSize: _iconArea,
                      enabledMouseCursor: SystemMouseCursors.click,
                      disabledMouseCursor: SystemMouseCursors.basic,
                      padding: EdgeInsets.zero,
                      visualDensity: VisualDensity.standard,
                      backgroundColor: Colors.transparent,
                      disabledBackgroundColor: Colors.transparent,
                    ).copyWith(
                      overlayColor: const WidgetStatePropertyAll<Color>(
                        Colors.transparent,
                      ),
                    ),
                icon: ExcludeSemantics(
                  child: SizedBox.fromSize(
                    size: _iconArea,
                    child: Stack(
                      clipBehavior: Clip.none,
                      children: <Widget>[
                        Positioned(
                          left: primaryIconOffset.dx,
                          top:
                              primaryIconOffset.dy -
                              (_stackHeight - _stackIconSize) *
                                  widget.size /
                                  _stackIconSize /
                                  2,
                          child: Transform.scale(
                            alignment: Alignment.topLeft,
                            scale: widget.size / _stackIconSize,
                            child: _ProviderIconStack(
                              mappings: visible,
                              width:
                                  _stackIconSize +
                                  (visible.length - 1) *
                                      _ProviderIconStack.iconOffset,
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
      },
    ),
  );
}

class _DesktopProviderRow extends StatefulWidget {
  const _DesktopProviderRow({
    required this.mappings,
    required this.selectedProvider,
    required this.isEnabled,
    required this.onSelected,
  });

  final List<ProviderMapping> mappings;
  final String selectedProvider;
  final bool isEnabled;
  final Future<void> Function(ProviderMapping) onSelected;

  @override
  State<_DesktopProviderRow> createState() => _DesktopProviderRowState();
}

class _DesktopProviderRowState extends State<_DesktopProviderRow> {
  static const double _cellWidth = 40;
  static const double _cellHeight = 48;
  static const Duration _switchDuration = Duration(milliseconds: 320);
  static const double _hoverScale = 1.5;
  static const Duration _hoverDuration = Duration(milliseconds: 140);
  late List<ProviderMapping> _orderedMappings;
  String? _hoveredProvider;

  // Preserve the other providers' positions when the selection swaps with the front.
  @override
  void initState() {
    super.initState();
    _orderedMappings = widget.mappings.toList();
  }

  @override
  void didUpdateWidget(covariant _DesktopProviderRow oldWidget) {
    super.didUpdateWidget(oldWidget);
    final Map<String, ProviderMapping> current = <String, ProviderMapping>{
      for (final mapping in widget.mappings) mapping.providerInstance: mapping,
    };
    _orderedMappings = <ProviderMapping>[
      for (final mapping in _orderedMappings)
        if (current.remove(mapping.providerInstance)
            case final ProviderMapping updated)
          updated,
      ...current.values,
    ];
    final int selectedIndex = _orderedMappings.indexWhere(
      (mapping) => mapping.providerInstance == widget.selectedProvider,
    );
    if (selectedIndex > 0) {
      final ProviderMapping previous = _orderedMappings.first;
      _orderedMappings[0] = _orderedMappings[selectedIndex];
      _orderedMappings[selectedIndex] = previous;
    }
  }

  // Hover affects only the icon, not its position or hit area.
  void _setHoveredProvider(String? provider) {
    if (_hoveredProvider == provider) return;
    setState(() => _hoveredProvider = provider);
  }

  // Wrap without scrolling if a header cannot fit every provider on one line.
  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final int columns = constraints.maxWidth.isFinite
          ? (constraints.maxWidth / _cellWidth).floor().clamp(
              1,
              _orderedMappings.length,
            )
          : _orderedMappings.length;
      final double cellWidth = constraints.maxWidth
          .clamp(0, _cellWidth)
          .toDouble();
      final Duration duration = MediaQuery.disableAnimationsOf(context)
          ? Duration.zero
          : _switchDuration;
      return SizedBox(
        width: columns * cellWidth,
        height:
            ((_orderedMappings.length + columns - 1) ~/ columns) * _cellHeight,
        child: Stack(
          children: <Widget>[
            for (final entry in _orderedMappings.indexed)
              AnimatedPositioned(
                key: ValueKey<String>(
                  'provider-position-${entry.$2.providerInstance}',
                ),
                duration: duration,
                curve: Curves.easeOutCubic,
                left: (entry.$1 % columns) * cellWidth,
                top: (entry.$1 ~/ columns) * _cellHeight,
                width: cellWidth,
                height: _cellHeight,
                child: MouseRegion(
                  onEnter: (_) =>
                      _setHoveredProvider(entry.$2.providerInstance),
                  onExit: (_) => _setHoveredProvider(null),
                  child: Semantics(
                    selected:
                        entry.$2.providerInstance == widget.selectedProvider,
                    child: IconButton(
                      key: ValueKey<String>(entry.$2.providerInstance),
                      tooltip: _providerTooltip(entry.$2),
                      onPressed:
                          widget.isEnabled &&
                              entry.$2.available &&
                              entry.$2.providerInstance !=
                                  widget.selectedProvider
                          ? () => widget.onSelected(entry.$2)
                          : null,
                      style:
                          IconButton.styleFrom(
                            padding: EdgeInsets.zero,
                            visualDensity: VisualDensity.standard,
                            backgroundColor: Colors.transparent,
                            disabledBackgroundColor: Colors.transparent,
                            side: BorderSide.none,
                          ).copyWith(
                            overlayColor: const WidgetStatePropertyAll<Color>(
                              Colors.transparent,
                            ),
                          ),
                      icon: TweenAnimationBuilder<double>(
                        tween: Tween<double>(
                          end:
                              entry.$2.providerInstance ==
                                  widget.selectedProvider
                              ? 24
                              : 16,
                        ),
                        duration: duration,
                        curve: Curves.easeOutCubic,
                        builder: (context, size, child) => AnimatedScale(
                          scale:
                              _hoveredProvider == entry.$2.providerInstance &&
                                  widget.isEnabled &&
                                  entry.$2.available &&
                                  entry.$2.providerInstance !=
                                      widget.selectedProvider
                              ? _hoverScale
                              : 1,
                          duration: MediaQuery.disableAnimationsOf(context)
                              ? Duration.zero
                              : _hoverDuration,
                          curve: Curves.easeOutCubic,
                          child: Opacity(
                            opacity: entry.$2.available ? 1 : 0.4,
                            child: MediaProviderIcon(
                              key: ValueKey<String>(
                                'desktop-provider-${entry.$2.providerInstance}',
                              ),
                              providerLabel: entry.$2.providerDomain,
                              iconSvg:
                                  ProviderManifestRegistry.iconSvgForProvider(
                                    entry.$2.providerDomain,
                                  ),
                              size: size,
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
      );
    },
  );
  // Provider action labels.
  String _providerTooltip(ProviderMapping mapping) {
    final String label =
        ProviderManifestRegistry.getByProvider(mapping.providerDomain)?.name ??
        mapping.providerDomain;
    return mapping.available &&
            mapping.providerInstance != widget.selectedProvider
        ? 'Zu $label wechseln'
        : label;
  }
}

class _ProviderIconStack extends StatefulWidget {
  const _ProviderIconStack({required this.mappings, required this.width});

  static const double iconOffset = 16;
  final List<ProviderMapping> mappings;
  final double width;

  @override
  State<_ProviderIconStack> createState() => _ProviderIconStackState();
}

class _ProviderIconStackState extends State<_ProviderIconStack>
    with SingleTickerProviderStateMixin {
  static const Duration _switchDuration = Duration(milliseconds: 320);
  static const double _stackHeight = 40;
  static const double _verticalOffset = 5;
  static const List<double> _iconSizes = <double>[24, 20, 18];
  static const List<double> _opacities = <double>[1, 0.82, 0.62];
  late final AnimationController _transition;
  late List<ProviderMapping> _mappings;
  final Map<String, Rect> _startingBounds = <String, Rect>{};
  final Map<String, double> _startingOpacities = <String, double>{};

  // The animation owns only visual positions, never the selected provider.
  @override
  void initState() {
    super.initState();
    _mappings = widget.mappings;
    _transition = AnimationController(
      vsync: this,
      duration: _switchDuration,
      value: 1,
    );
  }

  @override
  void didUpdateWidget(covariant _ProviderIconStack oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.mappings.length == widget.mappings.length &&
        oldWidget.mappings.indexed.every(
          (entry) =>
              entry.$2.providerInstance ==
              widget.mappings[entry.$1].providerInstance,
        )) {
      return;
    }
    final Map<String, Rect> currentBounds = <String, Rect>{
      for (final entry in _mappings.indexed)
        entry.$2.providerInstance: _bounds(entry.$2, entry.$1),
    };
    final Map<String, double> currentOpacities = <String, double>{
      for (final entry in _mappings.indexed)
        entry.$2.providerInstance: _opacity(entry.$2, entry.$1),
    };
    _startingBounds
      ..clear()
      ..addAll(currentBounds);
    _startingOpacities
      ..clear()
      ..addAll(currentOpacities);
    _mappings = widget.mappings;
    if (MediaQuery.disableAnimationsOf(context)) {
      _transition.value = 1;
    } else {
      _transition.forward(from: 0);
    }
  }

  @override
  void dispose() {
    _transition.dispose();
    super.dispose();
  }

  // Clip away covered logo regions so transparent SVGs do not bleed through.
  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: _transition,
    builder: (context, child) {
      final List<Rect> bounds = <Rect>[
        for (final entry in _mappings.indexed) _bounds(entry.$2, entry.$1),
      ];
      return SizedBox(
        width: widget.width,
        height: _stackHeight,
        child: Stack(
          children: <Widget>[
            for (int index = _mappings.length - 1; index >= 0; index--)
              Positioned.fromRect(
                rect: bounds[index],
                child: ClipPath(
                  clipper: _ProviderStackClipper(
                    bounds: bounds[index],
                    foregroundBounds: bounds
                        .take(index)
                        .toList(growable: false),
                  ),
                  child: Opacity(
                    opacity: _mappings[index].available
                        ? _opacity(_mappings[index], index)
                        : 0.4,
                    child: MediaProviderIcon(
                      key: ValueKey<String>(
                        'mobile-provider-${_mappings[index].providerInstance}',
                      ),
                      providerLabel:
                          ProviderManifestRegistry.getByProvider(
                            _mappings[index].providerDomain,
                          )?.name ??
                          _mappings[index].providerDomain,
                      iconSvg: ProviderManifestRegistry.iconSvgForProvider(
                        _mappings[index].providerDomain,
                      ),
                      size: bounds[index].width,
                    ),
                  ),
                ),
              ),
          ],
        ),
      );
    },
  );

  // Retarget from the visible frame, including when another tap interrupts a switch.
  Rect _bounds(ProviderMapping mapping, int index) {
    final Rect target = Rect.fromLTWH(
      index * _ProviderIconStack.iconOffset,
      (_stackHeight - _iconSizes[index]) / 2 - index * _verticalOffset,
      _iconSizes[index],
      _iconSizes[index],
    );
    return Rect.lerp(
      _startingBounds[mapping.providerInstance] ?? target,
      target,
      Curves.easeOutCubic.transform(_transition.value),
    )!;
  }

  double _opacity(ProviderMapping mapping, int index) {
    final double start =
        _startingOpacities[mapping.providerInstance] ?? _opacities[index];
    return start +
        (_opacities[index] - start) *
            Curves.easeOutCubic.transform(_transition.value);
  }
}

class _ProviderStackClipper extends CustomClipper<Path> {
  const _ProviderStackClipper({
    required this.bounds,
    required this.foregroundBounds,
  });

  static const double _separation = 1;
  static const Radius _cornerRadius = Radius.circular(6);
  final Rect bounds;
  final List<Rect> foregroundBounds;

  // A transparent gap separates the layers without adding button backgrounds.
  @override
  Path getClip(Size size) {
    Path visible = Path()..addRect(Offset.zero & size);
    for (final Rect foreground in foregroundBounds) {
      final Path covered = Path()
        ..addRRect(
          RRect.fromRectAndRadius(
            foreground.shift(-bounds.topLeft).inflate(_separation),
            _cornerRadius,
          ),
        );
      visible = Path.combine(PathOperation.difference, visible, covered);
    }
    return visible;
  }

  @override
  bool shouldReclip(covariant _ProviderStackClipper oldClipper) =>
      oldClipper.bounds != bounds ||
      oldClipper.foregroundBounds != foregroundBounds;
}
