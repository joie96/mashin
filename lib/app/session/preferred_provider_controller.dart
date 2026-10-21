import 'package:flutter/foundation.dart';

import '../../features/core/core_models.dart';

/// Projects Rust-owned settings; writes become visible only after persistence.
class PreferredProviderController extends ChangeNotifier {
  static final PreferredProviderController instance =
      PreferredProviderController();

  PreferredProvider? _preferredProvider;
  Future<void> Function(PreferredProvider?)? _persist;
  int _sessionGeneration = 0;

  // Session snapshot and persistence ownership.
  void initialize({
    required PreferredProvider? preferredProvider,
    required Future<void> Function(PreferredProvider?) persist,
  }) {
    _sessionGeneration++;
    _preferredProvider = preferredProvider;
    _persist = persist;
    notifyListeners();
  }

  void reset() {
    _sessionGeneration++;
    _preferredProvider = null;
    _persist = null;
    notifyListeners();
  }

  // Preference commands and current snapshot.
  Future<void> setPreferredProvider(
    PreferredProvider? preferredProvider,
  ) async {
    final persist = _persist;
    if (persist == null) {
      throw StateError('Session settings have not been initialized');
    }
    final generation = _sessionGeneration;
    await persist(preferredProvider);
    if (_sessionGeneration != generation) return;
    _preferredProvider = preferredProvider;
    notifyListeners();
  }

  PreferredProvider? get preferredProvider => _preferredProvider;
}
