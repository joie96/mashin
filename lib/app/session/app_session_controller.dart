import 'dart:async';

import 'package:flutter/foundation.dart';
import '../../features/core/core_events.dart';
import '../../features/core/core_bridge.dart';
import '../../features/core/core_models.dart';
import '../../features/core/core_userdata.dart';
import '../../features/shared/widgets/media_image.dart';
import 'preferred_provider_controller.dart';

class AppSessionController extends ChangeNotifier {
  static const Duration _startupTimeout = Duration(seconds: 5);
  static const Duration _sessionStartupTimeout = Duration(seconds: 20);

  AppSessionController({MashinCoreBridge? coreBridge})
    : coreBridge = coreBridge ?? MashinCoreBridge() {
    coreEvents = MashinCoreBridgeEvents(coreBridge: this.coreBridge);
    _instance = this;
    PreferredProviderController.instance.addListener(notifyListeners);
    _providerSubscription = this.coreBridge.catalogUpdates.listen((update) {
      if (!_isDisposed &&
          _isAuthenticated &&
          update.kind == CatalogQueryKind.providerManifests) {
        ProviderManifestRegistry.replaceAll(update.providerManifests);
      }
    });
    _cacheSubscription = this.coreBridge.events.listen((event) {
      if (!_isDisposed &&
          _isAuthenticated &&
          event['type'] == 'cache_cleared' &&
          event['cache'] == 'database') {
        unawaited(
          _runStartupTask(
            'provider icons',
            _loadProviderIcons,
            _startupTimeout,
          ),
        );
      }
    });
  }

  static AppSessionController? _instance;

  final MashinCoreBridge coreBridge;
  late final MashinCoreBridgeEvents coreEvents;
  late final StreamSubscription<CatalogUpdate> _providerSubscription;
  late final StreamSubscription<Map<String, dynamic>> _cacheSubscription;

  UserData? _userDataController;
  Future<void>? _initialization;
  Future<void>? _pendingLogout;
  String _baseUrl = '';
  String? _username;
  late String _audioProfile;
  bool _isAuthenticated = false;
  bool _isLoading = true;
  bool _isDisposed = false;
  ThemePreference _themePreference = ThemePreference.dark;
  bool _shouldShowMultiDeviceButton = true;
  LogLevelPreference _logLevel = LogLevelPreference.info;

  UserData? get userDataController => _userDataController;
  String get baseUrl => _baseUrl;
  String? get username => _username;
  String get audioProfile => _audioProfile;
  bool get isLoading => _isLoading;
  bool get isAuthenticated => _isAuthenticated;
  ThemePreference get themePreference => _themePreference;
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
      await _startSessionDataLoad();
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
      unawaited(_startSessionDataLoad());
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

  Future<void> setThemePreference(ThemePreference themePreference) async {
    if (themePreference == _themePreference) {
      return;
    }

    await saveSessionSettings(
      SessionSettingsUpdate(
        themePreference: themePreference,
        logLevel: _logLevel,
        shouldShowMultiDeviceButton: _shouldShowMultiDeviceButton,
      ),
    );
  }

  Future<void> saveSessionSettings(SessionSettingsUpdate settings) async {
    await coreBridge.saveSessionSettings(settings);
    _themePreference = settings.themePreference;
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
    MediaImage.configureArtworkLoader(null);
    MediaImage.configureImageLoader(null);
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
    _themePreference = settings.themePreference;
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
    MediaImage.configureArtworkLoader(
      coreBridge.getMediaArtwork,
      catalogUpdates: coreBridge.catalogUpdates,
      catalogReadChanges: coreBridge.catalogReadChanges,
    );
    MediaImage.configureImageLoader(
      (request, refresh) => coreBridge.readImage(request, refresh: refresh),
      invalidations: coreBridge.events
          .where(
            (event) =>
                event['type'] == 'cache_cleared' && event['cache'] == 'images',
          )
          .map<void>((_) {}),
      sourceChanges: coreBridge.events
          .where((event) => event['type'] == 'image_source_changed')
          .map((event) => event['proxy_id'] as String),
    );
  }

  // Rust owns parallel data loading; icons, snapshots and playback remain independent.
  Future<void> _startSessionDataLoad() async {
    final controller = _userDataController;
    if (controller == null || _isDisposed) return;
    final sessionData = _runStartupTask('session data start', () async {
      await coreBridge.startSessionDataLoad();
    }, _startupTimeout);
    await Future.wait(<Future<void>>[
      _runStartupTask(
        'user data snapshot',
        controller.refreshLibrarySnapshot,
        _startupTimeout,
      ),
      _runStartupTask('provider icons', _loadProviderIcons, _startupTimeout),
      sessionData,
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
    ProviderManifestRegistry.configureIconLoader(
      coreBridge.getProviderIconData,
    );
    final manifests = await coreBridge.getProviderManifests();
    if (_isDisposed ||
        !_isAuthenticated ||
        !identical(controller, _userDataController)) {
      return;
    }
    ProviderManifestRegistry.replaceAll(manifests);
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
    unawaited(_providerSubscription.cancel());
    unawaited(_cacheSubscription.cancel());
    PreferredProviderController.instance.removeListener(notifyListeners);
    _disposeUserDataController();
    coreEvents.dispose();
    unawaited(_disposeCoreBridge());
    if (identical(_instance, this)) {
      _instance = null;
      MediaImage.configureArtworkLoader(null);
      MediaImage.configureImageLoader(null);
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
