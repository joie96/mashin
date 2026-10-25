import 'dart:async';

import 'package:flutter/foundation.dart';
import '../../features/core/core_events.dart';
import '../../features/core/core_bridge.dart';
import '../../features/core/core_models.dart';
import '../../features/core/core_userdata.dart';
import 'preferred_provider_controller.dart';

class AppSessionController extends ChangeNotifier {
  static const Duration _startupTimeout = Duration(seconds: 5);
  static const Duration _sessionStartupTimeout = Duration(seconds: 20);
  static const Duration _userDataLoadTimeout = Duration(minutes: 2);

  AppSessionController({MashinCoreBridge? coreBridge})
    : coreBridge = coreBridge ?? MashinCoreBridge() {
    coreEvents = MashinCoreBridgeEvents(coreBridge: this.coreBridge);
    _instance = this;
    PreferredProviderController.instance.addListener(notifyListeners);
  }

  static AppSessionController? _instance;

  final MashinCoreBridge coreBridge;
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

  UserData? get userDataController => _userDataController;
  String get baseUrl => _baseUrl;
  String? get username => _username;
  String get audioProfile => _audioProfile;
  bool get isLoading => _isLoading;
  bool get isAuthenticated => _isAuthenticated;
  int get localCacheSizeBytes => _localCacheSizeBytes;
  bool get isDarkTheme => _isDarkTheme;
  LogLevelPreference get logLevel => _logLevel;
  PreferredProvider? get preferredProvider =>
      PreferredProviderController.instance.preferredProvider;

  Future<void> initialize() async {
    try {
      await _restoreSession();
      if (_isDisposed) return;
      await _refreshSessionSettings();
      if (_isDisposed) return;
      _initializeUserDataController();
      _isLoading = false;
      notifyListeners();
      await _loadSessionData();
    } catch (error, stackTrace) {
      if (_isLoading) {
        _isAuthenticated = false;
      }
      coreBridge.log(
        LogLevelPreference.error,
        'AppSessionController',
        'Startup bootstrap failed: $error | $stackTrace',
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
      if (_isDisposed) return null;
      _isAuthenticated = true;
      _initializeUserDataController();
      notifyListeners();
      unawaited(_loadSessionData());
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
    PreferredProviderController.instance.initialize(
      preferredProvider: settings.preferredProvider,
      persist: coreBridge.setPreferredProvider,
    );
  }

  // Session projections exist before background loading so views can subscribe.
  void _initializeUserDataController() {
    _disposeUserDataController();
    if (!_isAuthenticated) {
      return;
    }
    final controller = UserData(coreBridge: coreBridge);
    _userDataController = controller;
  }

  // Independent startup tasks cannot prevent other sections from becoming ready.
  Future<void> _loadSessionData() async {
    final controller = _userDataController;
    if (controller == null || _isDisposed) return;
    await Future.wait(<Future<void>>[
      _runStartupTask('library', () async {
        await controller.loadLibraryData(waitFor: _preloadRecommendations());
      }, _userDataLoadTimeout),
      _runStartupTask('provider icons', _loadProviderIcons, _startupTimeout),
      _runStartupTask('playback events', coreEvents.start, _startupTimeout),
    ]);
  }

  Future<void> _preloadRecommendations() async {
    try {
      await coreBridge.getRecommendations();
    } catch (error) {
      coreBridge.log(
        LogLevelPreference.warn,
        'AppSessionController',
        'Recommendations preload failed: $error',
      );
    }
  }

  Future<void> _runStartupTask(
    String name,
    Future<void> Function() operation,
    Duration timeout,
  ) async {
    try {
      await operation().timeout(timeout);
    } catch (error, stackTrace) {
      coreBridge.log(
        LogLevelPreference.error,
        'AppSessionController',
        'Loading $name failed: $error | $stackTrace',
      );
    }
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
    PreferredProviderController.instance.removeListener(notifyListeners);
    _disposeUserDataController();
    coreEvents.dispose();
    unawaited(_disposeCoreBridge());
    if (identical(_instance, this)) {
      _instance = null;
      PreferredProviderController.instance.reset();
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
