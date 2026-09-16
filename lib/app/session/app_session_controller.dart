import 'dart:async';

import 'package:flutter/foundation.dart';

import '../../features/core/core_events.dart';
import '../../features/core/core_bridge.dart';
import '../../features/core/core_models.dart';
import '../../features/core/core_userdata.dart';
import '../logging/app_logger.dart';

class AppSessionController extends ChangeNotifier {
  final AppLogger _logger = AppLogger('AppSessionController');

  AppSessionController() {
    coreBridge = MashinCoreBridge();
    coreEvents = MashinCoreBridgeEvents(coreBridge: coreBridge);
    _instance = this;
  }

  static AppSessionController? _instance;

  late final MashinCoreBridge coreBridge;
  late final MashinCoreBridgeEvents coreEvents;

  UserData? _userDataController;
  late String _baseUrl;
  String? _username;
  late String _preferredCodec;
  bool _isAuthenticated = false;
  bool _isLoading = true;
  bool _isDisposed = false;

  UserData? get userDataController => _userDataController;
  String get baseUrl => _baseUrl;
  String? get username => _username;
  String get preferredCodec => _preferredCodec;
  bool get isLoading => _isLoading;
  bool get isAuthenticated => _isAuthenticated;

  Future<void> initialize() async {
    try {
      await _restoreSession();
      await _refreshSessionSettings();
      await _loadProviderIcons();
      await _initializeUserData();
      await coreEvents.start();
    } catch (error, stackTrace) {
      _logger.error(
        'Startup bootstrap failed: $error',
        error: error,
        stackTrace: stackTrace,
      );
    } finally {
      if (!_isDisposed) {
        _isLoading = false;
        notifyListeners();
      }
    }
  }

  Future<String?> login({
    required String baseUrl,
    required String username,
    required String password,
  }) async {
    _baseUrl = baseUrl;
    _username = username;

    try {
      await coreBridge.login(
        musicAssistantUrl: baseUrl,
        username: username,
        password: password,
      );
      if (_isDisposed) {
        return null;
      }

      await _refreshSessionSettings();
      _isAuthenticated = true;
      await _loadProviderIcons();
      await _initializeUserData();
      notifyListeners();
      return null;
    } catch (_) {
      return 'Verbindung fehlgeschlagen. Bitte Server-URL pruefen.';
    }
  }

  Future<void> setPreferredCodec(String codec) async {
    if (codec == _preferredCodec) {
      return;
    }

    await coreBridge.requestCodec(codec);
    _preferredCodec = codec;
    notifyListeners();
  }

  Future<void> _restoreSession() async {
    _isAuthenticated = await coreBridge.restorePersistedSession();
  }

  Future<void> _refreshSessionSettings() async {
    final settings = await coreBridge.getSessionSettings();
    _baseUrl = settings.musicAssistantUrl;
    MediaItemImage.configureProxyBaseUrl(_baseUrl);
    _username = settings.username;
    _preferredCodec = settings.preferredCodec;
  }

  Future<void> _initializeUserData() async {
    _disposeUserDataController();
    if (!_isAuthenticated) {
      return;
    }
    final controller = UserData(coreBridge: coreBridge);
    _userDataController = controller;
    await controller.loadPreferences();
  }

  Future<void> _loadProviderIcons() async {
    if (!_isAuthenticated) {
      return;
    }

    final manifests = await coreBridge.getProviderManifests();
    ProviderManifestRegistry.registerAll(manifests);
    await Future.wait(
      manifests.where((manifest) => manifest.hasDefaultIcon).map((
        manifest,
      ) async {
        try {
          final iconData = await coreBridge.getProviderIconData(
            manifest.domain,
          );
          if (iconData != null) {
            ProviderManifestRegistry.registerIconData(
              manifest.domain,
              iconData,
            );
          }
        } catch (_) {}
      }),
    );
  }

  void _disposeUserDataController() {
    final UserData? controller = _userDataController;
    if (controller == null) {
      return;
    }
    controller.dispose();
    _userDataController = null;
  }

  @override
  void dispose() {
    _isDisposed = true;
    _disposeUserDataController();
    unawaited(coreBridge.shutdown());
    coreEvents.dispose();
    if (identical(_instance, this)) {
      _instance = null;
    }
    super.dispose();
  }
}
