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
  Future<void>? _initialization;
  Future<void>? _pendingLogout;
  String _baseUrl = '';
  String? _username;
  late String _audioProfile;
  bool _isAuthenticated = false;
  bool _isLoading = true;
  bool _isDisposed = false;
  late int _localCacheSizeBytes;
  bool _isDarkTheme = true;
  bool _shouldShowMultiDeviceButton = true;
  LogLevelPreference _logLevel = LogLevelPreference.info;

  UserData? get userDataController => _userDataController;
  String get baseUrl => _baseUrl;
  String? get username => _username;
  String get audioProfile => _audioProfile;
  bool get isLoading => _isLoading;
  bool get isAuthenticated => _isAuthenticated;
  int get localCacheSizeBytes => _localCacheSizeBytes;
  bool get isDarkTheme => _isDarkTheme;
  bool get shouldShowMultiDeviceButton => _shouldShowMultiDeviceButton;
  LogLevelPreference get logLevel => _logLevel;
  PreferredProvider? get preferredProvider =>
      PreferredProviderController.instance.preferredProvider;

  Future<void> initialize() => _initialization ??= _initialize();

  Future<void> _initialize() async {
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

  Future<void> logout() =>
      _pendingLogout ??= _logout().whenComplete(() => _pendingLogout = null);

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
        shouldShowMultiDeviceButton: _shouldShowMultiDeviceButton,
      ),
    );
  }

  Future<void> saveSessionSettings(SessionSettingsUpdate settings) async {
    await coreBridge.saveSessionSettings(settings);
    _localCacheSizeBytes = settings.localCacheSizeBytes;
    _isDarkTheme = settings.isDarkTheme;
    _logLevel = settings.logLevel;
    _shouldShowMultiDeviceButton = settings.shouldShowMultiDeviceButton;
    notifyListeners();
  }

  // Do not redirect until native logout has removed the persisted credentials.
  Future<void> _logout() async {
    await coreBridge.logout();
    if (_isDisposed) return;
    _isAuthenticated = false;
    _username = null;
    _disposeUserDataController();
    PreferredProviderController.instance.reset();
    ProviderManifestRegistry.clear();
    MediaItemImage.configureProxyBaseUrl('');
    await coreEvents.reset();
    if (!_isDisposed) notifyListeners();
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
    _shouldShowMultiDeviceButton = settings.shouldShowMultiDeviceButton;
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

  // Rust owns the ordered data load; icons and playback remain independent.
  Future<void> _loadSessionData() async {
    final controller = _userDataController;
    if (controller == null || _isDisposed) return;
    final library = _runStartupTask(
      'library',
      controller.loadLibraryData,
      _userDataLoadTimeout,
    );
    await Future.wait(<Future<void>>[
      library.then(
        (_) => _runStartupTask(
          'provider icons',
          _loadProviderIcons,
          _startupTimeout,
        ),
      ),
      _runStartupTask('playback events', coreEvents.start, _startupTimeout),
    ]);
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

    final controller = _userDataController;
    final manifests = await coreBridge.getProviderManifests();
    if (_isDisposed ||
        !_isAuthenticated ||
        !identical(controller, _userDataController)) {
      return;
    }
    ProviderManifestRegistry.registerAll(manifests);
    await Future.wait(
      manifests.where((manifest) => manifest.hasDefaultIcon).map((
        manifest,
      ) async {
        try {
          final iconData = await coreBridge.getProviderIconData(
            manifest.domain,
          );
          if (iconData != null &&
              !_isDisposed &&
              _isAuthenticated &&
              identical(controller, _userDataController)) {
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
