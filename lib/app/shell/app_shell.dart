import 'dart:async';

import 'package:flutter/material.dart';

import '../config/app_config.dart';
import '../../features/auth/login_screen.dart';
import '../../features/shared/selection/selection_session_controller.dart';
import '../../features/shared/widgets/mobile_selection_overlay.dart';
import '../../data/music_assistant/ma_auth_store.dart';
import '../../data/music_assistant/music_assistant_client.dart';
import '../../features/playback/playback_backend.dart';
import '../../features/playback/playback_coordinator.dart';
import '../../features/playback/playback_models.dart';
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

  AppSection _currentSection = AppSection.home;
  bool _isDarkTheme = true;
  final MaAuthStore _authStore = MaAuthStore();
  final SelectionSessionController _selectionSessionController = SelectionSessionController.instance;
  late final PlaybackCoordinator _playbackCoordinator;

  MusicAssistantClient? _musicAssistantClient;
  PlaybackState _playbackState = const PlaybackState();
  bool _isBootstrapping = true;
  String _baseUrl = AppConfig.defaultMusicAssistantUrl;
  String? _username;

  @override
  void initState() {
    super.initState();

    _playbackCoordinator = PlaybackCoordinator(
      backends: <PlaybackBackend>[
        SendspinRustBackend(
          bridge: FfiSendspinRustBridge(),
          musicAssistantBaseUrlResolver: () => _baseUrl,
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
    _musicAssistantClient?.dispose();
    super.dispose();
  }

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
                      icon: Icons.home_rounded,
                      selected: _currentSection == AppSection.home,
                      onTap: () => _select(AppSection.home),
                    ),
                    _DesktopNavItem(
                      label: 'Entdecken',
                      icon: Icons.explore_rounded,
                      selected: _currentSection == AppSection.explore,
                      onTap: () => _select(AppSection.explore),
                    ),
                    _DesktopNavItem(
                      label: 'Favoriten',
                      icon: Icons.favorite_rounded,
                      selected: _currentSection == AppSection.favorites,
                      onTap: () => _select(AppSection.favorites),
                    ),
                    const Divider(height: 24),
                    _DesktopNavItem(
                      label: 'Playlists',
                      icon: Icons.queue_music_rounded,
                      selected: _currentSection == AppSection.playlists,
                      onTap: () => _select(AppSection.playlists),
                    ),
                    _DesktopNavItem(
                      label: 'Suche',
                      icon: Icons.search_rounded,
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
                        child: AppRouter.pageFor(_currentSection, _musicAssistantClient!),
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
        _DesktopPlayerBar(
          isPlaying: _playbackState.playbackState == PlaybackStateType.playing,
          playbackState: _playbackState,
          onTogglePlayPause: _togglePlayPause,
          onPrevious: _playPrevious,
          onNext: _playNext,
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
                child: AppRouter.pageFor(_currentSection, _musicAssistantClient!),
              ),
            ),
            _MobilePlayerBar(
              isPlaying: _playbackState.playbackState == PlaybackStateType.playing,
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

  Future<void> _restoreSession() async {
    final stored = await _authStore.read();

    final candidateBaseUrl = stored?.baseUrl ?? _baseUrl;
    final candidateToken = stored?.token;
    final candidateUsername = stored?.username;

    _baseUrl = candidateBaseUrl;
    _username = candidateUsername;

    if (candidateToken == null || candidateToken.isEmpty) {
      if (!mounted) {
        return;
      }

      setState(() {
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
      setState(() {
        _musicAssistantClient = candidateClient;
        _isBootstrapping = false;
      });
      return;
    }

    candidateClient.dispose();
    await _authStore.clear();

    setState(() {
      _musicAssistantClient = null;
      _isBootstrapping = false;
    });
  }

  Future<String?> _handleLogin({
    required String baseUrl,
    required String username,
    required String password,
  }) async {
    _baseUrl = baseUrl;
    _username = username;

    final loginClient = MusicAssistantClient(baseUrl: baseUrl);

    try {
      final token = await loginClient.login(username: username, password: password);

      final authenticatedClient = MusicAssistantClient(
        baseUrl: baseUrl,
        authToken: token,
      );

      final valid = await authenticatedClient.hasValidSession();
      if (!valid) {
        authenticatedClient.dispose();
        return 'Anmeldung fehlgeschlagen. Bitte Eingaben pruefen.';
      }

      await _authStore.write(
        MaAuthSession(
          baseUrl: baseUrl,
          username: username,
          token: token,
        ),
      );

      if (!mounted) {
        authenticatedClient.dispose();
        return null;
      }

      _musicAssistantClient?.dispose();

      setState(() {
        _musicAssistantClient = authenticatedClient;
        _currentSection = AppSection.home;
      });

      return null;
    } on MaUnauthorizedException {
      return 'Benutzername oder Passwort ist ungueltig.';
    } on MaRequestException catch (ex) {
      return ex.message;
    } catch (_) {
      return 'Verbindung fehlgeschlagen. Bitte Server-URL pruefen.';
    } finally {
      loginClient.dispose();
    }
  }

  Future<void> _initializePlaybackCoordinator() async {
    try {
      await _playbackCoordinator.initialize();
      if (!mounted) {
        return;
      }

      setState(() {
        _playbackState = _playbackCoordinator.state;
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
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  void _onPlaybackChanged() {
    if (!mounted) {
      return;
    }

    setState(() {
      _playbackState = _playbackCoordinator.state;
    });
  }
}

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
          Icon(Icons.chevron_left_rounded, color: colors.textSecondary),
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
            icon: Icon(isDarkTheme ? Icons.light_mode_rounded : Icons.dark_mode_rounded),
          ),
          const SizedBox(width: 12),
          CircleAvatar(
            radius: 18,
            backgroundColor: colors.bgTertiary,
            child: Icon(Icons.person_rounded, color: colors.textSecondary),
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
  final bool isPlaying;
  final PlaybackState playbackState;
  final Future<void> Function() onTogglePlayPause;
  final Future<void> Function() onPrevious;
  final Future<void> Function() onNext;
  final Future<void> Function() onToggleMute;
  final ValueChanged<double> onVolumeChanged;

  const _DesktopPlayerBar({
    required this.isPlaying,
    required this.playbackState,
    required this.onTogglePlayPause,
    required this.onPrevious,
    required this.onNext,
    required this.onToggleMute,
    required this.onVolumeChanged,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool hasPlaybackSession = playbackState.playbackState != PlaybackStateType.uninitialized;
    final bool isShuffleActive = playbackState.shuffleEnabled == true;
    final String repeatMode = playbackState.repeatMode?.trim().toLowerCase() ?? '';
    final bool isRepeatActive = repeatMode.isNotEmpty && repeatMode != 'off' && repeatMode != 'none';
    final double maxSeconds = ((playbackState.seekMaxMillis ?? 0) / 1000).clamp(0, 36000).toDouble();

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
                    child: Icon(Icons.music_note_rounded, color: colors.textSecondary),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: <Widget>[
                        Text(
                          'Kein Track aktiv',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(color: colors.textPrimary, fontWeight: FontWeight.w600),
                        ),
                        Text(
                          'Waehle einen Titel zum Starten',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(color: colors.textSecondary, fontSize: 12),
                        ),
                      ],
                    ),
                  ),
                  _PlayerBarIconButton(
                    icon: Icons.favorite_border_rounded,
                    onTap: () {},
                    color: colors.textSecondary,
                  ),
                  _PlayerBarIconButton(
                    icon: Icons.add_rounded,
                    onTap: () {},
                    color: colors.textSecondary,
                  ),
                ],
              ),
            ),
            Expanded(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: <Widget>[
                  Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: <Widget>[
                      _PlayerBarIconButton(
                        icon: Icons.shuffle_rounded,
                        onTap: () {},
                        color: isShuffleActive ? colors.accent : colors.textSecondary,
                      ),
                      _PlayerBarIconButton(
                        icon: Icons.skip_previous_rounded,
                        onTap: hasPlaybackSession ? onPrevious : null,
                        color: colors.textPrimary,
                      ),
                      const SizedBox(width: 6),
                      SizedBox(
                        width: 36,
                        height: 36,
                        child: ElevatedButton(
                          onPressed: hasPlaybackSession
                              ? () {
                                  onTogglePlayPause();
                                }
                              : null,
                          style: ElevatedButton.styleFrom(
                            padding: EdgeInsets.zero,
                            shape: const CircleBorder(),
                            backgroundColor: colors.textPrimary,
                            foregroundColor: colors.bgPrimary,
                            disabledBackgroundColor: colors.textPrimary.withValues(alpha: 0.45),
                            disabledForegroundColor: colors.bgPrimary,
                            elevation: 0,
                          ),
                          child: Icon(isPlaying ? Icons.pause_rounded : Icons.play_arrow_rounded, size: 20),
                        ),
                      ),
                      const SizedBox(width: 6),
                      _PlayerBarIconButton(
                        icon: Icons.skip_next_rounded,
                        onTap: hasPlaybackSession ? onNext : null,
                        color: colors.textPrimary,
                      ),
                      _PlayerBarIconButton(
                        icon: isRepeatActive ? Icons.repeat_on_rounded : Icons.repeat_rounded,
                        onTap: () {},
                        color: isRepeatActive ? colors.accent : colors.textSecondary,
                      ),
                    ],
                  ),
                  const SizedBox(height: 4),
                  ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 600),
                    child: Row(
                      children: <Widget>[
                        Text(
                          '0:00',
                          style: TextStyle(color: colors.textTertiary, fontSize: 11),
                        ),
                        const SizedBox(width: 10),
                        Expanded(
                          child: Slider(
                            value: 0,
                            min: 0,
                            max: maxSeconds > 0 ? maxSeconds : 1,
                            onChanged: null,
                            activeColor: colors.textSecondary,
                            inactiveColor: colors.bgTertiary,
                          ),
                        ),
                        const SizedBox(width: 10),
                        Text(
                          _formatSeconds(maxSeconds),
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
                  _PlayerBarIconButton(icon: Icons.queue_music_rounded, onTap: () {}, color: colors.textSecondary),
                  _PlayerBarIconButton(icon: Icons.auto_awesome_rounded, onTap: () {}, color: colors.textSecondary),
                  _PlayerBarIconButton(icon: Icons.devices_rounded, onTap: () {}, color: colors.textSecondary),
                  _PlayerBarIconButton(
                    icon: playbackState.muted ? Icons.volume_off_rounded : Icons.volume_up_rounded,
                    onTap: onToggleMute,
                    color: playbackState.muted ? colors.accent : colors.textSecondary,
                  ),
                  SizedBox(
                    width: 90,
                    child: Slider(
                      value: playbackState.volume.toDouble().clamp(0, 100),
                      min: 0,
                      max: 100,
                      activeColor: colors.textSecondary,
                      inactiveColor: colors.bgTertiary,
                      onChanged: onVolumeChanged,
                    ),
                  ),
                  _PlayerBarIconButton(icon: Icons.tune_rounded, onTap: () {}, color: colors.textSecondary),
                ],
              ),
            ),
          ],
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
      icon: Icon(icon, size: 18, color: onTap == null ? color.withValues(alpha: 0.45) : color),
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

class _MobilePlayerBar extends StatelessWidget {
  final bool isPlaying;
  final Future<void> Function() onTogglePlayPause;

  const _MobilePlayerBar({
    required this.isPlaying,
    required this.onTogglePlayPause,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

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
            child: Icon(Icons.music_note_rounded, color: colors.textSecondary),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              'Kein Track aktiv',
              style: TextStyle(color: colors.textPrimary, fontWeight: FontWeight.w600),
              overflow: TextOverflow.ellipsis,
            ),
          ),
          IconButton(
            onPressed: () {
              onTogglePlayPause();
            },
            icon: Icon(isPlaying ? Icons.pause_rounded : Icons.play_arrow_rounded),
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
            icon: Icons.home_rounded,
            selected: currentSection == AppSection.home,
            onTap: () => onSelected(AppSection.home),
          ),
          _MobileNavButton(
            icon: Icons.explore_rounded,
            selected: currentSection == AppSection.explore,
            onTap: () => onSelected(AppSection.explore),
          ),
          _MobileNavButton(
            icon: Icons.favorite_rounded,
            selected: currentSection == AppSection.favorites,
            onTap: () => onSelected(AppSection.favorites),
          ),
          _MobileNavButton(
            icon: Icons.queue_music_rounded,
            selected: currentSection == AppSection.playlists,
            onTap: () => onSelected(AppSection.playlists),
          ),
          _MobileNavButton(
            icon: Icons.search_rounded,
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
