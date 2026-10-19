import 'package:shared_preferences/shared_preferences.dart';

import '../core/core_models.dart';

/// Selects section queries without replacing the page's original media item.
class DetailProviderSelection {
  static const String preferenceKey = 'details.preferred_provider';

  DetailProviderSelection({required this.itemId, required this.provider});

  final String itemId;
  final String provider;
  List<ProviderMapping> _mappings = const <ProviderMapping>[];
  ProviderMapping? _selected;

  // Section query identity and header options.
  String get selectedItemId => _selected?.itemId ?? itemId;
  String get selectedProvider => _selected?.providerInstance ?? provider;
  List<ProviderMapping> get mappings =>
      List<ProviderMapping>.unmodifiable(<ProviderMapping>[
        ?_selected,
        ..._mappings.where((mapping) => mapping != _selected),
      ]);

  // Loading and persisting the cross-page preference.
  Future<void> initialize(MediaItem item) async {
    final Set<String> instances = <String>{};
    _mappings = item.providerMappings
        .where(
          (mapping) =>
              mapping.providerInstance.isNotEmpty &&
              mapping.providerInstance != 'library' &&
              mapping.providerDomain != 'library' &&
              mapping.itemId.isNotEmpty &&
              instances.add(mapping.providerInstance),
        )
        .toList(growable: false);
    _selected = null;
    final List<ProviderMapping> available = _mappings
        .where((mapping) => mapping.available)
        .toList(growable: false);
    if (available.isEmpty) return;
    if (provider != 'library') {
      _selected = available
          .where(
            (mapping) =>
                mapping.providerInstance == provider ||
                mapping.providerDomain == provider,
          )
          .firstOrNull;
      return;
    }
    final SharedPreferences preferences = await SharedPreferences.getInstance();
    final List<String>? preferred = preferences.getStringList(preferenceKey);
    _selected = available
        .where((mapping) => mapping.providerInstance == preferred?.firstOrNull)
        .firstOrNull;
    _selected ??= available
        .where((mapping) => mapping.providerDomain == preferred?.lastOrNull)
        .firstOrNull;
    _selected ??= available.first;
  }

  Future<void> select(ProviderMapping mapping) async {
    if (!_mappings.contains(mapping) || !mapping.available) {
      throw ArgumentError('Provider is not available for this item');
    }
    final SharedPreferences preferences = await SharedPreferences.getInstance();
    if (!await preferences.setStringList(preferenceKey, <String>[
      mapping.providerInstance,
      mapping.providerDomain,
    ])) {
      throw StateError('Preferred provider could not be saved');
    }
    _selected = mapping;
  }
}
