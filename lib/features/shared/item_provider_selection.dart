import 'package:flutter/foundation.dart';

import '../../app/session/preferred_provider_controller.dart';
import '../core/core_models.dart';

/// Owns transient mapping order for one loaded object, not a persisted item ID.
class ItemProviderSelection extends ChangeNotifier {
  ItemProviderSelection._(this.item)
    : _mappings = item.providerMappings
          .where(
            (mapping) =>
                mapping.providerInstance.isNotEmpty &&
                mapping.providerInstance != 'library' &&
                mapping.providerDomain != 'library' &&
                mapping.itemId.isNotEmpty,
          )
          .toList();

  static final Expando<ItemProviderSelection> _selections =
      Expando<ItemProviderSelection>();

  factory ItemProviderSelection.forItem(MediaItem item) =>
      _selections[item] ??= ItemProviderSelection._(item);

  final MediaItem item;
  List<ProviderMapping> _mappings;
  Future<void>? _initialization;

  // Mapping order is shared by icon controls and actions for this exact object.
  List<ProviderMapping> get mappings =>
      List<ProviderMapping>.unmodifiable(_mappings);
  ProviderMapping? get selected =>
      _mappings.where((mapping) => mapping.available).firstOrNull;
  bool get canCycle =>
      _mappings.where((mapping) => mapping.available).length > 1;

  ({String itemId, String provider}) get selectedIdentity {
    final mapping = _initialization == null ? null : selected;
    return (
      itemId: mapping?.itemId ?? item.itemId,
      provider: mapping?.providerInstance ?? item.provider,
    );
  }

  String? get selectedUri {
    if (_initialization == null || selected == null) return item.uri;
    final identity = selectedIdentity;
    return '${identity.provider}://${mediaTypeToJson(item.mediaType)}/${identity.itemId}';
  }

  // Preferences are read once per loaded object and are never written here.
  Future<void> initialize() => _initialization ??= _initialize();

  Future<void> cycle() async {
    await initialize();
    final List<ProviderMapping> available = _mappings
        .where((mapping) => mapping.available)
        .toList();
    if (available.length < 2) return;
    _rotateTo(available[1]);
    notifyListeners();
  }

  Future<void> _initialize() async {
    if (_mappings.isEmpty) return;
    final available = _mappings.where((mapping) => mapping.available);
    if (available.length <= 1) {
      if (available.firstOrNull case final ProviderMapping mapping) {
        _rotateTo(mapping);
      }
      return;
    }
    final preferred = PreferredProviderController.instance.preferredProvider;
    final mapping =
        available
            .where(
              (mapping) =>
                  mapping.providerInstance == preferred?.providerInstance,
            )
            .firstOrNull ??
        available
            .where(
              (mapping) => mapping.providerDomain == preferred?.providerDomain,
            )
            .firstOrNull ??
        available.firstOrNull;
    if (mapping != null) _rotateTo(mapping);
    notifyListeners();
  }

  void _rotateTo(ProviderMapping mapping) {
    final int index = _mappings.indexOf(mapping);
    _mappings = <ProviderMapping>[
      ..._mappings.skip(index),
      ..._mappings.take(index),
    ];
  }
}
