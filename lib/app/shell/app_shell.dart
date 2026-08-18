import 'dart:async';

import 'package:flutter/material.dart' hide RepeatMode;

import '../config/app_config.dart';
import '../../features/auth/login_screen.dart';
import '../../features/shared/selection/selection_session_controller.dart';
import '../../features/shared/widgets/mobile_selection_overlay.dart';
import '../../data/music_assistant/ma_auth_store.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_event_hub.dart';
import '../../data/music_assistant/ma_models.dart';
import '../../features/playback/playback_coordinator.dart';
import '../../features/playback/playback_models.dart';
import '../../features/playback/playback_player.dart';
import '../../features/playback/sendspin/sendspin_player.dart';
import '../../features/playback/sendspin/sendspin_rust_backend.dart';
import '../../features/playback/sendspin/sendspin_rust_bridge_ffi.dart';
import '../navigation/app_router.dart';
import '../navigation/app_section.dart';
import '../theme/app_theme.dart';

class AppShell extends StatefulWidget {
  const AppShell({super.key});

  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell> {
  static const double _mobileSelectionOverlayBottomInset = 84;
  static const Duration _busyUiFallbackAfter = Duration(seconds: 3);

  AppSection _currentSection = AppSection.home;
  bool _isDarkTheme = true;
  final AuthStore _authStore = AuthStore();
  final SelectionSessionController _selectionSessionController = SelectionSessionController.instance;
  late final PlaybackCoordinator _playbackCoordinator;

  MusicAssistantClient? _musicAssistantClient;
  MusicAssistantEventHub? _musicAssistantEventHub;
  PlaybackState _playbackState = const PlaybackState();
  PlaybackStateType _lastStablePlaybackStateType = PlaybackStateType.uninitialized;
  DateTime? _busySince;
  bool _isBootstrapping = true;
  String _baseUrl = AppConfig.defaultMusicAssistantUrl;
  String? _username;

  // Getters for current Track and Queue state
  QueueItem? get _currentQueueItem {
    final items = _playbackCoordinator.queueItems;
    final index = _playbackCoordinator.currentQueueIndex;
    if (index == null || index < 0 || index >= items.length) {
      return null;
    }
    return items[index];
  }

  String get _currentTrackTitle {
    final item = _currentQueueItem;
    if (item == null) {
      return 'Kein Track aktiv';
    }

    final mediaName = item.mediaItem?.name.trim() ?? '';
    final queueName = item.name.trim();
    if (mediaName.isNotEmpty) {
      return mediaName;
    }
    if (queueName.isNotEmpty) {
      return queueName;
    }
    return 'Unbekannter Titel';
  }

  String get _currentTrackSubtitle {
    final item = _currentQueueItem;
    if (item == null) {
      return 'Waehle einen Titel zum Starten';
    }

    final artists = item.mediaItem?.artists
            .map((artist) => artist.name.trim())
            .where((name) => name.isNotEmpty)
            .toList() ??
        const <String>[];
    if (artists.isNotEmpty) {
      return artists.join(', ');
    }
    return 'Unbekannte:r Interpret:in';
  }

  double get _currentTrackMaxSeconds {
    final durationFromStateSeconds = ((_playbackState.durationMillis ?? 0) / 1000).toDouble();
    if (durationFromStateSeconds > 0) {
      return durationFromStateSeconds;
    }

    final seekMaxSeconds = ((_playbackState.seekMaxMillis ?? 0) / 1000).toDouble();
    if (seekMaxSeconds > 0) {
      return seekMaxSeconds;
    }

    final queueDuration = _currentQueueItem?.duration;
    if (queueDuration != null && queueDuration > 0) {
      return queueDuration.toDouble();
    }

    return 0;
  }

  String? get _currentTrackCoverUrl {
    final url = _currentQueueItem?.mediaItem?.imageUri?.trim() ?? '';
    return url.isEmpty ? null : url;
  }

  double get _currentTrackPositionSeconds {
    final positionFromStateSeconds = ((_playbackState.positionMillis ?? 0) / 1000).toDouble();
    if (positionFromStateSeconds > 0) {
      return positionFromStateSeconds;
    }

    return 0;
  }

  PlaybackStateType get _effectivePlaybackStateType {
    final raw = _playbackState.playbackState;
    final isBusy = raw == PlaybackStateType.buffering || raw == PlaybackStateType.seeking;

    if (!isBusy) {
      return raw;
    }

    final busySince = _busySince;
    if (busySince == null) {
      return raw;
    }

    final isStaleBusy = DateTime.now().difference(busySince) > _busyUiFallbackAfter;
    if (isStaleBusy) {
      return _lastStablePlaybackStateType;
    }

    return raw;
  }

  // Initialization and Lifecycle
  @override
  void initState() {
    super.initState();

    _playbackCoordinator = PlaybackCoordinator(
      players: <PlaybackPlayer>[
        SendspinPlayer(
          transportBackend: SendspinRustBackend(
            bridge: FfiSendspinRustBridge(),
          ),
          clientResolver: () => _musicAssistantClient,
          eventHubResolver: () => _musicAssistantEventHub,
        ),
      ],
      initialOutputMode: PlaybackOutputMode.localSendspin,
    );

    _playbackCoordinator.addListener(_onPlaybackChanged);
    unawaited(_bootstrapStartup());
  }

  @override
  void dispose() {
    _playbackCoordinator.removeListener(_onPlaybackChanged);
    unawaited(_playbackCoordinator.shutdown());
    _playbackCoordinator.dispose();
    unawaited(_musicAssistantEventHub?.dispose());
    _musicAssistantClient?.dispose();
    super.dispose();
  }

  // Building the UI
  @override
  Widget build(BuildContext context) {
    if (_isBootstrapping) {
      return Scaffold(
        backgroundColor: context.mashinColors.bgPrimary,
        body: const Center(child: CircularProgressIndicator()),
      );
    }

    if (_musicAssistantClient == null) {
      return Scaffold(
        backgroundColor: context.mashinColors.bgPrimary,
        body: SafeArea(
          child: LoginScreen(
            initialBaseUrl: _baseUrl,
            initialUsername: _username,
            onSubmit: _handleLogin,
          ),
        ),
      );
    }

    return LayoutBuilder(
      builder: (BuildContext context, BoxConstraints constraints) {
        final isDesktop = !AppConfig.isMobile();
        return Scaffold(
          backgroundColor: context.mashinColors.bgPrimary,
          body: SafeArea(
            top: false,
            bottom: false,
            child: isDesktop ? _buildDesktopLayout(context) : _buildMobileLayout(context),
          ),
        );
      },
    );
  }

  Widget _buildDesktopLayout(BuildContext context) {
    final colors = context.mashinColors;

    return Column(
      children: <Widget>[
        Expanded(
          child: Row(
            children: <Widget>[
              Container(
                width: 250,
                color: colors.bgSecondary,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: <Widget>[
                    const SizedBox(height: 44),
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 22),
                      child: Text(
                        'Mashin',
                        style: Theme.of(context).textTheme.titleLarge?.copyWith(
                          fontWeight: FontWeight.w700,
                          color: colors.textPrimary,
                        ),
                      ),
                    ),
                    const SizedBox(height: 22),
                    _DesktopNavItem(
                      label: 'Home',
                      icon: Icons.home_outlined,
                      selected: _currentSection == AppSection.home,
                      onTap: () => _select(AppSection.home),
                    ),
                    _DesktopNavItem(
                      label: 'Entdecken',
                      icon: Icons.explore_outlined,
                      selected: _currentSection == AppSection.explore,
                      onTap: () => _select(AppSection.explore),
                    ),
                    _DesktopNavItem(
                      label: 'Favoriten',
                      icon: Icons.favorite_outlined,
                      selected: _currentSection == AppSection.favorites,
                      onTap: () => _select(AppSection.favorites),
                    ),
                    const Divider(height: 24),
                    _DesktopNavItem(
                      label: 'Playlists',
                      icon: Icons.queue_music_outlined,
                      selected: _currentSection == AppSection.playlists,
                      onTap: () => _select(AppSection.playlists),
                    ),
                    _DesktopNavItem(
                      label: 'Suche',
                      icon: Icons.search_outlined,
                      selected: _currentSection == AppSection.search,
                      onTap: () => _select(AppSection.search),
                    ),
                  ],
                ),
              ),
              VerticalDivider(width: 1, color: colors.border),
              Expanded(
                child: Column(
                  children: <Widget>[
                    _DesktopTopBar(
                      title: _titleForSection(_currentSection),
                      onToggleTheme: _toggleTheme,
                      isDarkTheme: _isDarkTheme,
                    ),
                    Expanded(
                      child: Container(
                        color: colors.bgPrimary,
                        child: AppRouter.pageFor(_currentSection, _musicAssistantClient!, _playbackCoordinator),
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
        _DesktopPlayerBar(
          playStateType: _effectivePlaybackStateType,
          playbackState: _playbackState,
          dontStopTheMusicEnabled: _playbackCoordinator.dontStopTheMusicEnabled ?? false,
          trackTitle: _currentTrackTitle,
          trackSubtitle: _currentTrackSubtitle,
          trackCoverUrl: _currentTrackCoverUrl,
          maxSeconds: _currentTrackMaxSeconds,
          currentSeconds: _currentTrackPositionSeconds,
          onTogglePlayPause: _togglePlayPause,
          onPrevious: _playPrevious,
          onNext: _playNext,
          onToggleShuffle: _toggleShuffle,
          onToggleRepeatMode: _toggleRepeatMode,
          onToggleDontStopTheMusic: _toggleDontStopTheMusic,
          onToggleMute: _toggleMute,
          onVolumeChanged: _setVolume,
        ),
      ],
    );
  }

  Widget _buildMobileLayout(BuildContext context) {
    final colors = context.mashinColors;

    return Stack(
      children: <Widget>[
        Column(
          children: <Widget>[
            Expanded(
              child: Container(
                color: colors.bgPrimary,
                child: AppRouter.pageFor(_currentSection, _musicAssistantClient!, _playbackCoordinator),
              ),
            ),
            _MobilePlayerBar(
              playbackState: _effectivePlaybackStateType,
              trackTitle: _currentTrackTitle,
              trackCoverUrl: _currentTrackCoverUrl,
              onTogglePlayPause: _togglePlayPause,
            ),
            _MobileBottomNav(
              currentSection: _currentSection,
              onSelected: _select,
            ),
          ],
        ),
        AnimatedBuilder(
          animation: _selectionSessionController,
          builder: (BuildContext context, Widget? child) {
            if (!_selectionSessionController.isVisible) {
              return const SizedBox.shrink();
            }

            return Positioned(
              right: 8,
              bottom: _mobileSelectionOverlayBottomInset,
              child: MobileSelectionOverlay(
                allSelected: _selectionSessionController.allSelected,
                onToggleAll: _selectionSessionController.onToggleAll,
                onMenuTap: _selectionSessionController.onMenuTap,
                onClose: _selectionSessionController.onClose,
              ),
            );
          },
        ),
      ],
    );
  }

  void _select(AppSection section) {
    if (_currentSection == section) {
      return;
    }

    setState(() {
      _currentSection = section;
    });
  }

  void _toggleTheme() {
    final mode = _isDarkTheme ? ThemeMode.light : ThemeMode.dark;
    final root = context.findAncestorStateOfType<_MashinAppState>();
    root?.setThemeMode(mode);

    setState(() {
      _isDarkTheme = !_isDarkTheme;
    });
  }

  String _titleForSection(AppSection section) {
    switch (section) {
      case AppSection.home:
        return 'Home';
      case AppSection.explore:
        return 'Entdecken';
      case AppSection.favorites:
        return 'Favoriten';
      case AppSection.playlists:
        return 'Playlists';
      case AppSection.search:
        return 'Suche';
    }
  }

  // Session & Login Management
  Future<void> _restoreSession() async {
    final stored = await _authStore.read();

    final candidateBaseUrl = stored?.baseUrl ?? _baseUrl;
    final candidateSendspinUrl = stored?.sendspinUrl;
    final candidateToken = stored?.token;
    final candidateUsername = stored?.username;

    _baseUrl = candidateBaseUrl;
    _username = candidateUsername;

    await _authStore.writeSendspinUrl(
      baseUrl: _baseUrl,
      sendspinUrl: candidateSendspinUrl,
    );

    if (candidateToken == null || candidateToken.isEmpty) {
      unawaited(_musicAssistantEventHub?.dispose());
      if (!mounted) {
        return;
      }

      setState(() {
        _musicAssistantEventHub = null;
        _isBootstrapping = false;
      });
      return;
    }

    final candidateClient = MusicAssistantClient(
      baseUrl: candidateBaseUrl,
      authToken: candidateToken,
    );

    final isValid = await candidateClient.hasValidSession();
    if (!mounted) {
      candidateClient.dispose();
      return;
    }

    if (isValid) {
      final eventHub = MusicAssistantEventHub(
        baseUrl: candidateBaseUrl,
        authToken: candidateToken,
      );
      setState(() {
        _musicAssistantClient = candidateClient;
        _musicAssistantEventHub = eventHub;
        _isBootstrapping = false;
      });
      return;
    }

    candidateClient.dispose();
    await _authStore.clear();
    unawaited(_musicAssistantEventHub?.dispose());

    setState(() {
      _musicAssistantClient = null;
      _musicAssistantEventHub = null;
      _isBootstrapping = false;
    });
  }

  Future<String?> _handleLogin({
    required String baseUrl,
    required String username,
    required String password,
  }) async {
    _baseUrl = baseUrl;
    final sendspinUrl = await _authStore.readSendspinUrl(baseUrl: baseUrl);
    _username = username;

    final loginClient = MusicAssistantClient(baseUrl: baseUrl);

    try {
      final token = await loginClient.login(username: username, password: password);

      final authenticatedClient = MusicAssistantClient(
        baseUrl: baseUrl,
        authToken: token,
      );

      final authenticatedEventHub = MusicAssistantEventHub(
        baseUrl: baseUrl,
        authToken: token,
      );

      final valid = await authenticatedClient.hasValidSession();
      if (!valid) {
        authenticatedClient.dispose();
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

      if (!mounted) {
        authenticatedClient.dispose();
        await authenticatedEventHub.dispose();
        return null;
      }

      _musicAssistantClient?.dispose();
      unawaited(_musicAssistantEventHub?.dispose());

      setState(() {
        _musicAssistantClient = authenticatedClient;
        _musicAssistantEventHub = authenticatedEventHub;
        _currentSection = AppSection.home;
      });

      return null;
    } on UnauthorizedException {
      return 'Benutzername oder Passwort ist ungueltig.';
    } on RequestException catch (ex) {
      return ex.message;
    } catch (_) {
      return 'Verbindung fehlgeschlagen. Bitte Server-URL pruefen.';
    } finally {
      loginClient.dispose();
    }
  }

  // Bootstrap and Playback Initialization
  Future<void> _initializePlaybackCoordinator() async {
    try {
      await _playbackCoordinator.initialize();
      if (!mounted) {
        return;
      }

      setState(() {
        _playbackState = _playbackCoordinator.state;
        _lastStablePlaybackStateType = _playbackState.playbackState;
      });
    } catch (error) {
      if (!mounted) {
        return;
      }

      setState(() {
        _playbackState = const PlaybackState(
          playbackState: PlaybackStateType.error,
        );
      });

      debugPrint('Sendspin startup connection failed: $error');
    }
  }

  Future<void> _bootstrapStartup() async {
    try {
      await _restoreSession();
      await _initializePlaybackCoordinator();
    } catch (error) {
      debugPrint('Startup bootstrap failed: $error');
    }
  }

  Future<void> _togglePlayPause() async {
    try {
      await _playbackCoordinator.togglePlayPause();
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  // Playback Control Handlers
  Future<void> _playPrevious() async {
    try {
      await _playbackCoordinator.previous();
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  Future<void> _playNext() async {
    try {
      await _playbackCoordinator.next();
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  Future<void> _toggleShuffle() async {
    try {
      await _playbackCoordinator.toggleShuffle(_playbackState.shuffleEnabled);
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  Future<void> _toggleRepeatMode() async {
    try {
      await _playbackCoordinator.toggleRepeatMode(_playbackState.repeatMode);
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  Future<void> _toggleDontStopTheMusic() async {
    try {
      final enabled = _playbackCoordinator.dontStopTheMusicEnabled ?? false;
      await _playbackCoordinator.setDontStopTheMusic(!enabled);
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  Future<void> _toggleMute() async {
    try {
      await _playbackCoordinator.setMuted(!_playbackState.muted);
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  Future<void> _setVolume(double next) async {
    final int safeVolume = next.round().clamp(0, 100);
    try {
      await _playbackCoordinator.setVolume(safeVolume);
    } catch (error) {
      debugPrint('Set volume failed: $error');
    }
  }

  void _onPlaybackChanged() {
    if (!mounted) {
      return;
    }

    final nextPlaybackState = _playbackCoordinator.state;
    final nextType = nextPlaybackState.playbackState;
    final isBusy = nextType == PlaybackStateType.buffering || nextType == PlaybackStateType.seeking;

    setState(() {
      _playbackState = nextPlaybackState;
      if (isBusy) {
        _busySince ??= DateTime.now();
      } else {
        _busySince = null;
        _lastStablePlaybackStateType = nextType;
      }
    });
  }
}

// Desktop UI Components
class _DesktopTopBar extends StatelessWidget {
  final String title;
  final VoidCallback onToggleTheme;
  final bool isDarkTheme;

  const _DesktopTopBar({
    required this.title,
    required this.onToggleTheme,
    required this.isDarkTheme,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Container(
      height: 84,
      decoration: BoxDecoration(
        color: colors.bgPrimary,
        border: Border(bottom: BorderSide(color: colors.border)),
      ),
      child: Row(
        children: <Widget>[
          const SizedBox(width: 22),
          Icon(Icons.chevron_left_outlined, color: colors.textSecondary),
          const SizedBox(width: 18),
          Expanded(
            child: TextField(
              decoration: InputDecoration(
                hintText: 'Kuenstler:innen, Songs, Alben ...',
                hintStyle: TextStyle(color: colors.textTertiary),
                filled: true,
                fillColor: colors.bgSecondary,
                contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 0),
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(20),
                  borderSide: BorderSide.none,
                ),
              ),
            ),
          ),
          const SizedBox(width: 18),
          Text(
            title,
            style: Theme.of(context).textTheme.titleMedium?.copyWith(
              color: colors.textSecondary,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(width: 14),
          IconButton(
            tooltip: 'Theme wechseln',
            onPressed: onToggleTheme,
            icon: Icon(isDarkTheme ? Icons.light_mode_outlined : Icons.dark_mode_outlined),
          ),
          const SizedBox(width: 12),
          CircleAvatar(
            radius: 18,
            backgroundColor: colors.bgTertiary,
            child: Icon(Icons.person_outlined, color: colors.textSecondary),
          ),
          const SizedBox(width: 24),
        ],
      ),
    );
  }
}

class _DesktopNavItem extends StatelessWidget {
  final String label;
  final IconData icon;
  final bool selected;
  final VoidCallback onTap;

  const _DesktopNavItem({
    required this.label,
    required this.icon,
    required this.selected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 2),
      child: Material(
        color: selected ? colors.hover : Colors.transparent,
        borderRadius: BorderRadius.circular(10),
        child: InkWell(
          borderRadius: BorderRadius.circular(10),
          onTap: onTap,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 12),
            child: Row(
              children: <Widget>[
                Icon(icon, size: 20, color: selected ? colors.accent : colors.textSecondary),
                const SizedBox(width: 10),
                Text(
                  label,
                  style: TextStyle(
                    color: selected ? colors.textPrimary : colors.textSecondary,
                    fontWeight: selected ? FontWeight.w600 : FontWeight.w500,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _DesktopPlayerBar extends StatelessWidget {
  final PlaybackStateType playStateType;
  final PlaybackState playbackState;
  final bool dontStopTheMusicEnabled;
  final String trackTitle;
  final String trackSubtitle;
  final String? trackCoverUrl;
  final double maxSeconds;
  final double currentSeconds;
  final Future<void> Function() onTogglePlayPause;
  final Future<void> Function() onPrevious;
  final Future<void> Function() onNext;
  final Future<void> Function() onToggleShuffle;
  final Future<void> Function() onToggleRepeatMode;
  final Future<void> Function() onToggleDontStopTheMusic;
  final Future<void> Function() onToggleMute;
  final ValueChanged<double> onVolumeChanged;

  const _DesktopPlayerBar({
    required this.playStateType,
    required this.playbackState,
    required this.dontStopTheMusicEnabled,
    required this.trackTitle,
    required this.trackSubtitle,
    required this.trackCoverUrl,
    required this.maxSeconds,
    required this.currentSeconds,
    required this.onTogglePlayPause,
    required this.onPrevious,
    required this.onNext,
    required this.onToggleShuffle,
    required this.onToggleRepeatMode,
    required this.onToggleDontStopTheMusic,
    required this.onToggleMute,
    required this.onVolumeChanged,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool hasPlaybackSession = playStateType != PlaybackStateType.uninitialized;
    final bool isPlaying = playStateType == PlaybackStateType.playing;
    final bool isBusy = playStateType == PlaybackStateType.buffering ||
      playStateType == PlaybackStateType.seeking;
    final bool isShuffleActive = playbackState.shuffleEnabled == true;
    final RepeatMode repeatMode = playbackState.repeatMode ?? RepeatMode.off;
    final bool isRepeatOne = repeatMode == RepeatMode.one;
    final IconData repeatIcon = isRepeatOne ? Icons.repeat_one_outlined : Icons.repeat_outlined;
    final Color repeatColor = repeatMode == RepeatMode.off ? colors.textSecondary : colors.accent;
    final double clampedMaxSeconds = maxSeconds.clamp(0, 36000).toDouble();
    final double clampedCurrentSeconds = currentSeconds
      .clamp(0, clampedMaxSeconds > 0 ? clampedMaxSeconds : 1)
      .toDouble();

    return Container(
      height: 90,
      decoration: BoxDecoration(
        color: colors.bgSecondary,
        border: Border(top: BorderSide(color: colors.border)),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 20),
        child: Row(
          children: <Widget>[
            SizedBox(
              width: 300,
              child: Row(
                children: <Widget>[
                  Container(
                    width: 60,
                    height: 60,
                    decoration: BoxDecoration(
                      color: colors.bgTertiary,
                      borderRadius: BorderRadius.circular(4),
                    ),
                    child: ClipRRect(
                      borderRadius: BorderRadius.circular(4),
                      child: trackCoverUrl != null
                          ? Image.network(
                              trackCoverUrl!,
                              fit: BoxFit.cover,
                              errorBuilder: (_, _, _) {
                                return Icon(Icons.music_note_outlined, color: colors.textSecondary);
                              },
                            )
                          : Icon(Icons.music_note_outlined, color: colors.textSecondary),
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: <Widget>[
                        Text(
                          trackTitle,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(color: colors.textPrimary, fontWeight: FontWeight.w600),
                        ),
                        Text(
                          trackSubtitle,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(color: colors.textSecondary, fontSize: 12),
                        ),
                      ],
                    ),
                  ),
                  _PlayerBarIconButton(
                    icon: Icons.favorite_border_outlined,
                    onTap: () {},
                    color: colors.textSecondary,
                  ),
                  _PlayerBarIconButton(
                    icon: Icons.add_outlined,
                    onTap: () {},
                    color: colors.textSecondary,
                  ),
                ],
              ),
            ),
            Expanded(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.start,
                children: <Widget>[
                  const SizedBox(height: 12),
                  Row(
                    mainAxisSize: MainAxisSize.min,
                    mainAxisAlignment: MainAxisAlignment.center,
                    crossAxisAlignment: CrossAxisAlignment.center,
                    children: <Widget>[
                      _TransportControlButton(
                        icon: Icons.shuffle_outlined,
                        size: 32,
                        iconSize: 18,
                        onTap: hasPlaybackSession ? onToggleShuffle : null,
                        color: isShuffleActive ? colors.accent : colors.textSecondary,
                      ),
                      const SizedBox(width: 12),
                      _TransportControlButton(
                        icon: Icons.skip_previous_sharp,
                        size: 32,
                        iconSize: 26,
                        onTap: hasPlaybackSession ? onPrevious : null,
                        color: colors.textPrimary,
                      ),
                      const SizedBox(width: 12),
                      _HoverScaleControlButton(
                        width: 40,
                        height: 40,
                        onPressed: isBusy
                            ? null
                            : () {
                                onTogglePlayPause();
                              },
                        style: ElevatedButton.styleFrom(
                          padding: EdgeInsets.zero,
                          shape: const CircleBorder(),
                          backgroundColor: colors.textPrimary,
                          foregroundColor: colors.bgPrimary,
                          disabledBackgroundColor: colors.textPrimary.withValues(alpha: 0.45),
                          disabledForegroundColor: colors.bgPrimary,
                          elevation: 0,
                        ),
                        child: isBusy
                            ? SizedBox(
                                width: 16,
                                height: 16,
                                child: CircularProgressIndicator(
                                  strokeWidth: 2,
                                  valueColor: AlwaysStoppedAnimation<Color>(colors.bgPrimary),
                                ),
                              )
                            : Icon(isPlaying ? Icons.pause_sharp : Icons.play_arrow_sharp, size: 30),
                      ),
                      const SizedBox(width: 12),
                      _TransportControlButton(
                        icon: Icons.skip_next_sharp,
                        size: 32,
                        iconSize: 26,
                        onTap: hasPlaybackSession ? onNext : null,
                        color: colors.textPrimary,
                      ),
                      const SizedBox(width: 12),
                      _TransportControlButton(
                        icon: repeatIcon,
                        size: 32,
                        iconSize: 18,
                        onTap: hasPlaybackSession ? onToggleRepeatMode : null,
                        color: repeatColor,
                      ),
                    ],
                  ),
                  const SizedBox(height: 4),
                  ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 500),
                    child: Row(
                      children: <Widget>[
                        Text(
                          _formatSeconds(clampedCurrentSeconds),
                          style: TextStyle(color: colors.textTertiary, fontSize: 11),
                        ),
                        const SizedBox(width: 10),
                        Expanded(
                          child: Slider(
                            value: clampedCurrentSeconds,
                            min: 0,
                            max: clampedMaxSeconds > 0 ? clampedMaxSeconds : 1,
                            padding: EdgeInsets.zero,
                            onChanged: null,
                            activeColor: colors.textSecondary,
                            inactiveColor: colors.bgTertiary,
                          ),
                        ),
                        const SizedBox(width: 10),
                        Text(
                          _formatSeconds(clampedMaxSeconds),
                          style: TextStyle(color: colors.textTertiary, fontSize: 11),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
            SizedBox(
              width: 300,
              child: Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: <Widget>[
                  _PlayerBarIconButton(icon: Icons.queue_music_outlined, onTap: () {}, color: colors.textSecondary),
                  _PlayerBarIconButton(
                    icon: Icons.cell_tower_outlined,
                    onTap: hasPlaybackSession ? onToggleDontStopTheMusic : null,
                    color: dontStopTheMusicEnabled ? colors.accent : colors.textSecondary,
                  ),
                  _PlayerBarIconButton(icon: Icons.devices_outlined, onTap: () {}, color: colors.textSecondary),
                  _PlayerBarIconButton(
                    icon: playbackState.muted ? Icons.volume_off_outlined : Icons.volume_up_outlined,
                    onTap: onToggleMute,
                    color: playbackState.muted ? colors.accent : colors.textSecondary,
                  ),
                  SizedBox(
                    width: 100,
                    child: SliderTheme(
                      data: SliderTheme.of(context).copyWith(
                        trackHeight: 2,
                        thumbShape: const RoundSliderThumbShape(
                          enabledThumbRadius: 6,
                          disabledThumbRadius: 6,
                        ),
                        overlayShape: const RoundSliderOverlayShape(
                          overlayRadius: 10,
                        ),
                      ),
                      child: Slider(
                        value: playbackState.volume.toDouble().clamp(0, 100),
                        min: 0,
                        max: 100,
                        activeColor: colors.textSecondary,
                        inactiveColor: colors.bgTertiary,
                        onChanged: onVolumeChanged,
                      ),
                    ),
                  ),
                  _PlayerBarIconButton(icon: Icons.tune_outlined, onTap: () {}, color: colors.textSecondary),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _TransportControlButton extends StatelessWidget {
  final IconData icon;
  final VoidCallback? onTap;
  final Color color;
  final double size;
  final double iconSize;

  const _TransportControlButton({
    required this.icon,
    required this.onTap,
    required this.color,
    required this.size,
    required this.iconSize,
  });

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: size,
      height: size,
      child: IconButton(
        onPressed: onTap,
        padding: EdgeInsets.zero,
        constraints: const BoxConstraints(minWidth: 0, minHeight: 0),
        splashRadius: size / 2,
        icon: Icon(
          icon,
          size: iconSize,
          color: onTap == null ? color.withValues(alpha: 0.45) : color,
        ),
      ),
    );
  }
}

class _HoverScaleControlButton extends StatefulWidget {
  final double width;
  final double height;
  final VoidCallback? onPressed;
  final ButtonStyle style;
  final Widget child;

  const _HoverScaleControlButton({
    required this.width,
    required this.height,
    required this.onPressed,
    required this.style,
    required this.child,
  });

  @override
  State<_HoverScaleControlButton> createState() => _HoverScaleControlButtonState();
}

class _HoverScaleControlButtonState extends State<_HoverScaleControlButton> {
  bool _isHovered = false;

  void _setHovered(bool value) {
    if (_isHovered == value || widget.onPressed == null) {
      return;
    }

    setState(() {
      _isHovered = value;
    });
  }

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      onEnter: (_) => _setHovered(true),
      onExit: (_) => _setHovered(false),
      child: AnimatedScale(
        scale: _isHovered ? 1.08 : 1,
        duration: const Duration(milliseconds: 140),
        curve: Curves.easeOutCubic,
        child: SizedBox(
          width: widget.width,
          height: widget.height,
          child: ElevatedButton(
            onPressed: widget.onPressed,
            style: widget.style,
            child: widget.child,
          ),
        ),
      ),
    );
  }
}

class _PlayerBarIconButton extends StatelessWidget {
  final IconData icon;
  final VoidCallback? onTap;
  final Color color;

  const _PlayerBarIconButton({
    required this.icon,
    required this.onTap,
    required this.color,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return IconButton(
      onPressed: onTap,
      splashRadius: 16,
      visualDensity: VisualDensity.compact,
      icon: Icon(icon, size: 20, color: onTap == null ? color.withValues(alpha: 0.45) : color),
      style: IconButton.styleFrom(
        backgroundColor: Colors.transparent,
        foregroundColor: colors.textSecondary,
      ),
    );
  }
}

String _formatSeconds(double seconds) {
  if (!seconds.isFinite || seconds <= 0) {
    return '0:00';
  }

  final int totalSeconds = seconds.round();
  final int minutes = totalSeconds ~/ 60;
  final int remainingSeconds = totalSeconds % 60;
  return '$minutes:${remainingSeconds.toString().padLeft(2, '0')}';
}

// Mobile UI Components
class _MobilePlayerBar extends StatelessWidget {
  final PlaybackStateType playbackState;
  final String trackTitle;
  final String? trackCoverUrl;
  final Future<void> Function() onTogglePlayPause;

  const _MobilePlayerBar({
    required this.playbackState,
    required this.trackTitle,
    required this.trackCoverUrl,
    required this.onTogglePlayPause,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool isPlaying = playbackState == PlaybackStateType.playing;
    final bool isBusy = playbackState == PlaybackStateType.buffering ||
        playbackState == PlaybackStateType.seeking;

    return Container(
      height: 68,
      margin: const EdgeInsets.fromLTRB(10, 0, 10, 8),
      decoration: BoxDecoration(
        color: colors.bgSecondary,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: colors.border),
      ),
      child: Row(
        children: <Widget>[
          const SizedBox(width: 10),
          Container(
            width: 46,
            height: 46,
            decoration: BoxDecoration(
              color: colors.bgTertiary,
              borderRadius: BorderRadius.circular(8),
            ),
            child: ClipRRect(
              borderRadius: BorderRadius.circular(8),
              child: trackCoverUrl != null
                  ? Image.network(
                      trackCoverUrl!,
                      fit: BoxFit.cover,
                      errorBuilder: (_, _, _) => Icon(Icons.music_note_outlined, color: colors.textSecondary),
                    )
                  : Icon(Icons.music_note_outlined, color: colors.textSecondary),
            ),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              trackTitle,
              style: TextStyle(color: colors.textPrimary, fontWeight: FontWeight.w600),
              overflow: TextOverflow.ellipsis,
            ),
          ),
          IconButton(
            onPressed: isBusy
                ? null
                : () {
                    onTogglePlayPause();
                  },
            icon: isBusy
                ? SizedBox(
                    width: 18,
                    height: 18,
                    child: CircularProgressIndicator(
                      strokeWidth: 2,
                      valueColor: AlwaysStoppedAnimation<Color>(colors.textSecondary),
                    ),
                  )
                : Icon(isPlaying ? Icons.pause_outlined : Icons.play_arrow_outlined),
          ),
          const SizedBox(width: 4),
        ],
      ),
    );
  }
}

class _MobileBottomNav extends StatelessWidget {
  final AppSection currentSection;
  final ValueChanged<AppSection> onSelected;

  const _MobileBottomNav({
    required this.currentSection,
    required this.onSelected,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Container(
      height: 80,
      padding: const EdgeInsets.fromLTRB(10, 8, 10, 16),
      decoration: BoxDecoration(
        color: colors.bgSecondary,
        border: Border(top: BorderSide(color: colors.border)),
      ),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceAround,
        children: <Widget>[
          _MobileNavButton(
            icon: Icons.home_outlined,
            selected: currentSection == AppSection.home,
            onTap: () => onSelected(AppSection.home),
          ),
          _MobileNavButton(
            icon: Icons.explore_outlined,
            selected: currentSection == AppSection.explore,
            onTap: () => onSelected(AppSection.explore),
          ),
          _MobileNavButton(
            icon: Icons.favorite_outlined,
            selected: currentSection == AppSection.favorites,
            onTap: () => onSelected(AppSection.favorites),
          ),
          _MobileNavButton(
            icon: Icons.queue_music_outlined,
            selected: currentSection == AppSection.playlists,
            onTap: () => onSelected(AppSection.playlists),
          ),
          _MobileNavButton(
            icon: Icons.search_outlined,
            selected: currentSection == AppSection.search,
            onTap: () => onSelected(AppSection.search),
          ),
        ],
      ),
    );
  }
}

class _MobileNavButton extends StatelessWidget {
  final IconData icon;
  final bool selected;
  final VoidCallback onTap;

  const _MobileNavButton({
    required this.icon,
    required this.selected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return InkResponse(
      radius: 24,
      onTap: onTap,
      child: Container(
        width: 42,
        height: 42,
        decoration: BoxDecoration(
          color: selected ? colors.hover : Colors.transparent,
          borderRadius: BorderRadius.circular(21),
        ),
        child: Icon(
          icon,
          color: selected ? colors.accent : colors.textSecondary,
        ),
      ),
    );
  }
}

class MashinApp extends StatefulWidget {
  const MashinApp({super.key});

  @override
  State<MashinApp> createState() => _MashinAppState();
}

class _MashinAppState extends State<MashinApp> {
  ThemeMode _themeMode = ThemeMode.dark;

  void setThemeMode(ThemeMode mode) {
    if (_themeMode == mode) {
      return;
    }

    setState(() {
      _themeMode = mode;
    });
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Mashin',
      debugShowCheckedModeBanner: false,
      themeMode: _themeMode,
      theme: AppTheme.light(),
      darkTheme: AppTheme.dark(),
      home: const AppShell(),
    );
  }
}
