import 'dart:async';

import 'package:flutter/foundation.dart';

import '../../data/music_assistant/ma_auth_store.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_event_hub.dart';
import '../../features/playback/playback_coordinator.dart';
import '../../features/playback/playback_models.dart';
import '../../features/playback/playback_player.dart';
import '../../features/playback/sendspin/sendspin_player.dart';
import '../../features/playback/sendspin/sendspin_rust_backend.dart';
import '../../features/playback/sendspin/sendspin_rust_bridge_ffi.dart';
import '../../features/shared/userdata_controller.dart';
import '../config/app_config.dart';

class AppSessionController extends ChangeNotifier {
  AppSessionController()
    : playbackCoordinator = PlaybackCoordinator(
        players: <PlaybackPlayer>[
          SendspinPlayer(
            transportBackend: SendspinRustBackend(
              bridge: FfiSendspinRustBridge(),
            ),
            clientResolver: () => _instance?._client,
            eventHubResolver: () => _instance?._eventHub,
          ),
        ],
        initialOutputMode: PlaybackOutputMode.localSendspin,
      ) {
    _instance = this;
  }

  static AppSessionController? _instance;

  final AuthStore _authStore = AuthStore();
  final PlaybackCoordinator playbackCoordinator;

  MusicAssistantClient? _client;
  MusicAssistantEventHub? _eventHub;
  UserDataController? _userDataController;
  String _baseUrl = AppConfig.defaultMusicAssistantUrl;
  String? _username;
  String _preferredCodec = AppConfig.sendspinPreferredCodec;
  bool _isLoading = true;
  bool _isDisposed = false;

  MusicAssistantClient? get client => _client;
  UserDataController? get userDataController => _userDataController;
  String get baseUrl => _baseUrl;
  String? get username => _username;
  String get preferredCodec => _preferredCodec;
  bool get isLoading => _isLoading;
  bool get isAuthenticated => _client != null;

  Future<void> initialize() async {
    try {
      _preferredCodec = await AppConfig.getSendspinPreferredCodec();
      await _restoreSession();
      await playbackCoordinator.initialize();
    } catch (error) {
      debugPrint('Startup bootstrap failed: $error');
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
    final String sendspinUrl = await _authStore.readSendspinUrl(
      baseUrl: baseUrl,
    );
    final MusicAssistantClient loginClient = MusicAssistantClient(
      baseUrl: baseUrl,
    );

    try {
      final String token = await loginClient.login(
        username: username,
        password: password,
      );
      final MusicAssistantClient authenticatedClient = MusicAssistantClient(
        baseUrl: baseUrl,
        authToken: token,
      );
      final MusicAssistantEventHub authenticatedEventHub =
          MusicAssistantEventHub(baseUrl: baseUrl, authToken: token);

      if (!await authenticatedClient.hasValidSession()) {
        authenticatedClient.dispose();
        await authenticatedEventHub.dispose();
        return 'Anmeldung fehlgeschlagen. Bitte Eingaben pruefen.';
      }

      await _authStore.write(
        AuthSession(
          baseUrl: baseUrl,
          sendspinUrl: sendspinUrl,
          username: username,
          token: token,
        ),
      );

      if (_isDisposed) {
        authenticatedClient.dispose();
        await authenticatedEventHub.dispose();
        return null;
      }

      await _replaceAuthenticatedSession(
        client: authenticatedClient,
        eventHub: authenticatedEventHub,
        username: username,
      );
      notifyListeners();
      return null;
    } on UnauthorizedException {
      return 'Benutzername oder Passwort ist ungueltig.';
    } on RequestException catch (error) {
      return error.message;
    } catch (_) {
      return 'Verbindung fehlgeschlagen. Bitte Server-URL pruefen.';
    } finally {
      loginClient.dispose();
    }
  }

  Future<void> setPreferredCodec(String codec) async {
    final SendspinPlayer? player =
        playbackCoordinator.activePlayer is SendspinPlayer
        ? playbackCoordinator.activePlayer as SendspinPlayer
        : null;
    if (player == null || codec == _preferredCodec) {
      return;
    }

    await player.setPreferredAudioCodec(codec);
    _preferredCodec = codec;
    notifyListeners();
  }

  Future<void> _restoreSession() async {
    final AuthSession? stored = await _authStore.read();
    _baseUrl = stored?.baseUrl ?? _baseUrl;
    _username = stored?.username;

    await _authStore.writeSendspinUrl(
      baseUrl: _baseUrl,
      sendspinUrl: stored?.sendspinUrl,
    );

    if (stored == null) {
      return;
    }

    final MusicAssistantClient candidateClient = MusicAssistantClient(
      baseUrl: stored.baseUrl,
      authToken: stored.token,
    );
    if (!await candidateClient.hasValidSession()) {
      candidateClient.dispose();
      await _authStore.clear();
      return;
    }

    if (_isDisposed) {
      candidateClient.dispose();
      return;
    }

    await _replaceAuthenticatedSession(
      client: candidateClient,
      eventHub: MusicAssistantEventHub(
        baseUrl: stored.baseUrl,
        authToken: stored.token,
      ),
      username: stored.username,
    );
  }

  Future<void> _replaceAuthenticatedSession({
    required MusicAssistantClient client,
    required MusicAssistantEventHub eventHub,
    required String? username,
  }) async {
    _disposeUserDataController();
    _client?.dispose();
    unawaited(_eventHub?.dispose());

    _client = client;
    _eventHub = eventHub;
    _username = username;
    final UserDataController controller = UserDataController(
      client: client,
      configuredUsername: username,
    );
    controller.addListener(_onUserDataChanged);
    _userDataController = controller;
    await controller.loadPreferences();
  }

  void _onUserDataChanged() {
    if (!_isDisposed) {
      notifyListeners();
    }
  }

  void _disposeUserDataController() {
    final UserDataController? controller = _userDataController;
    if (controller == null) {
      return;
    }
    controller.removeListener(_onUserDataChanged);
    controller.dispose();
    _userDataController = null;
  }

  @override
  void dispose() {
    _isDisposed = true;
    _disposeUserDataController();
    unawaited(playbackCoordinator.shutdown());
    playbackCoordinator.dispose();
    unawaited(_eventHub?.dispose());
    _client?.dispose();
    if (identical(_instance, this)) {
      _instance = null;
    }
    super.dispose();
  }
}
