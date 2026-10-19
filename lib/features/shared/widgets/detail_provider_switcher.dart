import 'package:flutter/material.dart';

import '../../core/core_models.dart';
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
  static const Size _buttonSize = Size.square(40);
  static const double _selectedIconSize = 24;
  static const double _unselectedIconSize = 16;
  static const double _hoverScale = 1.5;
  static const Duration _hoverDuration = Duration(milliseconds: 140);

  bool _isSelecting = false;
  String? _hoveredProvider;

  // Hover changes the icon only, keeping the header's layout and hit areas stable.
  void _setHoveredProvider(String? provider) {
    if (_hoveredProvider == provider) return;
    setState(() => _hoveredProvider = provider);
  }

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
        child: MediaProviderIcon(
          providerLabel: provider,
          iconSvg: ProviderManifestRegistry.iconSvgForProvider(provider),
          size: _selectedIconSize,
        ),
      );
    }
    return Wrap(
      spacing: 4,
      runSpacing: 4,
      children: mappings
          .map((mapping) {
            final bool isSelected =
                mapping.providerInstance == widget.selection.selectedProvider;
            final bool isHovered =
                _hoveredProvider == mapping.providerInstance &&
                !isSelected &&
                mapping.available &&
                !widget.isLoading &&
                !_isSelecting;
            final String label =
                ProviderManifestRegistry.getByProvider(
                  mapping.providerDomain,
                )?.name ??
                mapping.providerDomain;
            return MouseRegion(
              onEnter: (_) => _setHoveredProvider(mapping.providerInstance),
              onExit: (_) => _setHoveredProvider(null),
              child: Semantics(
                selected: isSelected,
                child: IconButton(
                  key: ValueKey<String>(mapping.providerInstance),
                  tooltip: label,
                  onPressed:
                      widget.isLoading ||
                          _isSelecting ||
                          !mapping.available ||
                          isSelected
                      ? null
                      : () => _select(mapping),
                  style:
                      IconButton.styleFrom(
                        fixedSize: _buttonSize,
                        padding: const EdgeInsets.all(8),
                        backgroundColor: Colors.transparent,
                        disabledBackgroundColor: Colors.transparent,
                        side: BorderSide.none,
                        shape: const CircleBorder(),
                      ).copyWith(
                        overlayColor: const WidgetStatePropertyAll<Color>(
                          Colors.transparent,
                        ),
                      ),
                  icon: AnimatedScale(
                    scale: isHovered ? _hoverScale : 1,
                    duration: _hoverDuration,
                    curve: Curves.easeOutCubic,
                    child: Opacity(
                      opacity: mapping.available ? 1 : 0.4,
                      child: MediaProviderIcon(
                        providerLabel: label,
                        iconSvg: ProviderManifestRegistry.iconSvgForProvider(
                          mapping.providerDomain,
                        ),
                        size: isSelected
                            ? _selectedIconSize
                            : _unselectedIconSize,
                      ),
                    ),
                  ),
                ),
              ),
            );
          })
          .toList(growable: false),
    );
  }
}
