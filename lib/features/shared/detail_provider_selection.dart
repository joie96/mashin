import '../../app/session/preferred_provider_controller.dart';
import '../core/core_models.dart';

/// Selects section queries without replacing the page's original media item.
class DetailProviderSelection {
  DetailProviderSelection({required this.itemId, required this.provider});

  final String itemId;
  final String provider;
  List<ProviderMapping> _mappings = const <ProviderMapping>[];
  ProviderMapping? _selected;

  // Section query identity and header options.
  String get selectedItemId => _selected?.itemId ?? itemId;
  String get selectedProvider => _selected?.providerInstance ?? provider;
  List<ProviderMapping> get mappings {
    final int selectedIndex = _mappings.indexWhere(
      (mapping) => mapping == _selected,
    );
    final int startIndex = selectedIndex < 0 ? 0 : selectedIndex;
    return List<ProviderMapping>.unmodifiable(<ProviderMapping>[
      ..._mappings.skip(startIndex),
      ..._mappings.take(startIndex),
    ]);
  }

  ProviderMapping? get nextProvider => mappings
      .where(
        (mapping) =>
            mapping.available && mapping.providerInstance != selectedProvider,
      )
      .firstOrNull;

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
    final preferred = PreferredProviderController.instance.preferredProvider;
    _selected = available
        .where(
          (mapping) => mapping.providerInstance == preferred?.providerInstance,
        )
        .firstOrNull;
    _selected ??= available
        .where((mapping) => mapping.providerDomain == preferred?.providerDomain)
        .firstOrNull;
    _selected ??= available.first;
  }

  Future<void> select(ProviderMapping mapping) async {
    if (!_mappings.contains(mapping) || !mapping.available) {
      throw ArgumentError('Provider is not available for this item');
    }
    await PreferredProviderController.instance.setPreferredProvider(
      PreferredProvider(
        providerInstance: mapping.providerInstance,
        providerDomain: mapping.providerDomain,
      ),
    );
    _selected = mapping;
  }
}
