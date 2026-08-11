import 'package:flutter/material.dart';

import '../config/app_config.dart';
import '../../features/auth/login_screen.dart';
import '../../data/music_assistant/ma_auth_store.dart';
import '../../data/music_assistant/music_assistant_client.dart';
import '../navigation/app_router.dart';
import '../navigation/app_section.dart';
import '../theme/app_theme.dart';

class AppShell extends StatefulWidget {
  const AppShell({super.key});

  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell> {
  static const double _desktopBreakpoint = 960;

  AppSection _currentSection = AppSection.home;
  bool _isDarkTheme = true;
  final MaAuthStore _authStore = MaAuthStore();

  MusicAssistantClient? _musicAssistantClient;
  bool _isBootstrapping = true;
  String _baseUrl = AppConfig.defaultMusicAssistantUrl;
  String? _username;

  @override
  void initState() {
    super.initState();
    _restoreSession();
  }

  @override
  void dispose() {
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
        final isDesktop = constraints.maxWidth >= _desktopBreakpoint;
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

    return Row(
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
              const _DesktopPlayerBar(),
            ],
          ),
        ),
      ],
    );
  }

  Widget _buildMobileLayout(BuildContext context) {
    final colors = context.mashinColors;

    return Column(
      children: <Widget>[
        Expanded(
          child: Container(
            color: colors.bgPrimary,
            child: AppRouter.pageFor(_currentSection, _musicAssistantClient!),
          ),
        ),
        const _MobilePlayerBar(),
        _MobileBottomNav(
          currentSection: _currentSection,
          onSelected: _select,
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
  const _DesktopPlayerBar();

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Container(
      height: 88,
      decoration: BoxDecoration(
        color: colors.bgSecondary,
        border: Border(top: BorderSide(color: colors.border)),
      ),
      child: Row(
        children: <Widget>[
          const SizedBox(width: 18),
          Container(
            width: 54,
            height: 54,
            decoration: BoxDecoration(
              color: colors.bgTertiary,
              borderRadius: BorderRadius.circular(8),
            ),
            child: Icon(Icons.music_note_rounded, color: colors.textSecondary),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Text('Kein Track aktiv', style: TextStyle(color: colors.textPrimary, fontWeight: FontWeight.w600)),
                Text('Waehle einen Titel zum Starten', style: TextStyle(color: colors.textTertiary)),
              ],
            ),
          ),
          IconButton(onPressed: () {}, icon: const Icon(Icons.skip_previous_rounded)),
          IconButton(onPressed: () {}, icon: const Icon(Icons.play_arrow_rounded)),
          IconButton(onPressed: () {}, icon: const Icon(Icons.skip_next_rounded)),
          const SizedBox(width: 16),
        ],
      ),
    );
  }
}

class _MobilePlayerBar extends StatelessWidget {
  const _MobilePlayerBar();

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
          IconButton(onPressed: () {}, icon: const Icon(Icons.play_arrow_rounded)),
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
