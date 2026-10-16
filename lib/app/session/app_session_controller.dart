import 'dart:async';

import 'package:flutter/foundation.dart';
import '../../features/core/core_events.dart';
import '../../features/core/core_bridge.dart';
import '../../features/core/core_models.dart';
import '../../features/core/core_userdata.dart';

class AppSessionController extends ChangeNotifier {
  static const Duration _startupTimeout = Duration(seconds: 5);
  static const Duration _sessionStartupTimeout = Duration(seconds: 20);
  static const Duration _userDataMigrationTimeout = Duration(minutes: 2);

  AppSessionController() {
    coreBridge = MashinCoreBridge();
    coreEvents = MashinCoreBridgeEvents(coreBridge: coreBridge);
    _instance = this;
  }

  static AppSessionController? _instance;

  late final MashinCoreBridge coreBridge;
  late final MashinCoreBridgeEvents coreEvents;

  UserData? _userDataController;
  String _baseUrl = '';
  String? _username;
  late String _audioProfile;
  bool _isAuthenticated = false;
  bool _isLoading = true;
  bool _isDisposed = false;
  late int _localCacheSizeBytes;
  bool _isDarkTheme = true;
  LogLevelPreference _logLevel = LogLevelPreference.info;
  final Completer<void> _initialSettingsReady = Completer<void>();

  UserData? get userDataController => _userDataController;
  String get baseUrl => _baseUrl;
  String? get username => _username;
  String get audioProfile => _audioProfile;
  bool get isLoading => _isLoading;
  bool get isAuthenticated => _isAuthenticated;
  int get localCacheSizeBytes => _localCacheSizeBytes;
  bool get isDarkTheme => _isDarkTheme;
  LogLevelPreference get logLevel => _logLevel;
  Future<void> get initialSettingsReady => _initialSettingsReady.future;

  Future<void> initialize() async {
    try {
      await _restoreSession();
      await _refreshSessionSettings();
      _initialSettingsReady.complete();
      await _initializeUserData().timeout(_userDataMigrationTimeout);
      await _loadProviderIcons().timeout(_startupTimeout);
      await coreEvents.start().timeout(_startupTimeout);
    } catch (error, stackTrace) {
      if (!_initialSettingsReady.isCompleted) {
        _isAuthenticated = false;
      }
      coreBridge.log(
        LogLevelPreference.error,
        'AppSessionController',
        'Startup bootstrap failed: $error | $stackTrace',
      );
    } finally {
      if (!_initialSettingsReady.isCompleted) {
        _initialSettingsReady.complete();
      }
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
      await _initializeUserData();
      await _loadProviderIcons();
      notifyListeners();
      return null;
    } catch (_) {
      return 'Verbindung fehlgeschlagen. Bitte Server-URL prüfen.';
    }
  }

  Future<void> setAudioProfile(String audioProfile) async {
    if (audioProfile == _audioProfile) {
      return;
    }

    await coreBridge.setAudioProfile(audioProfile);
    _audioProfile = audioProfile;
    notifyListeners();
  }

  Future<void> setDarkTheme(bool isDarkTheme) async {
    if (isDarkTheme == _isDarkTheme) {
      return;
    }

    await saveSessionSettings(
      SessionSettingsUpdate(
        localCacheSizeBytes: _localCacheSizeBytes,
        isDarkTheme: isDarkTheme,
        logLevel: _logLevel,
      ),
    );
  }

  Future<void> saveSessionSettings(SessionSettingsUpdate settings) async {
    await coreBridge.saveSessionSettings(settings);
    _localCacheSizeBytes = settings.localCacheSizeBytes;
    _isDarkTheme = settings.isDarkTheme;
    _logLevel = settings.logLevel;
    notifyListeners();
  }

  Future<void> _restoreSession() async {
    _isAuthenticated = await coreBridge.restorePersistedSession().timeout(
      _sessionStartupTimeout,
    );
  }

  Future<void> _refreshSessionSettings() async {
    final settings = await coreBridge.getSessionSettings().timeout(
      _startupTimeout,
    );
    _baseUrl = settings.musicAssistantUrl;
    MediaItemImage.configureProxyBaseUrl(_baseUrl);
    _username = settings.username;
    _audioProfile = settings.audioProfile;
    _localCacheSizeBytes = settings.localCacheSizeBytes;
    _isDarkTheme = settings.isDarkTheme;
    _logLevel = settings.logLevel;
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
    coreEvents.dispose();
    unawaited(_disposeCoreBridge());
    if (identical(_instance, this)) {
      _instance = null;
    }
    super.dispose();
  }

  // Android's service owns playback; closing its UI releases only the client.
  Future<void> _disposeCoreBridge() async {
    try {
      if (!coreBridge.isServiceOwnedPlayback) {
        await coreBridge.shutdown();
      }
    } catch (error) {
      coreBridge.log(
        LogLevelPreference.error,
        'AppSessionController',
        'Core shutdown failed: $error',
      );
    } finally {
      await coreBridge.dispose();
    }
  }
}
