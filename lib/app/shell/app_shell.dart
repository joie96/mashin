import 'dart:async';

import 'package:flutter/material.dart' hide RepeatMode;

import '../layout/layout_utils.dart';
import '../../features/shared/selection/selection_controller.dart';
import '../../features/shared/selection/selection_overlay.dart';
import '../../features/shared/context_menu/context_menu_controller.dart';
import '../../features/shared/widgets/media_link_text.dart';
import '../../features/core/core_bridge.dart';
import '../../features/core/core_events.dart';
import '../../features/core/core_models.dart';
import '../../features/playerbar/playerbar_overlay.dart';
import '../../features/playerbar/playback_seek_slider.dart';
import '../../features/queue/queue_overlay.dart';
import '../../features/playlist/playlist_name_dialog.dart';
import '../../features/core/core_userdata.dart';
import '../../features/shared/widgets/media_image.dart';
import 'package:go_router/go_router.dart';

import '../navigation/app_router.dart';
import '../session/app_session_controller.dart';
import '../theme/app_theme.dart';

class AppShell extends StatefulWidget {
  const AppShell({
    super.key,
    required this.session,
    required this.navigatorKey,
    required this.navigationObserver,
    required this.child,
  });

  final AppSessionController session;
  final GlobalKey<NavigatorState> navigatorKey;
  final ShellNavigationObserver navigationObserver;
  final Widget child;

  @override
  State<AppShell> createState() => _AppShellState();
}

class ShellBackScope extends StatelessWidget {
  const ShellBackScope({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    final _ShellBackState? backState = context
        .dependOnInheritedWidgetOfExactType<_ShellBackState>();
    return PopScope<void>(
      canPop: !(backState?.hasOverlayBackAction ?? false),
      onPopInvokedWithResult: (bool didPop, void _) {
        if (!didPop) {
          backState?.onBack();
        }
      },
      child: child,
    );
  }
}

class _ShellBackState extends InheritedWidget {
  const _ShellBackState({
    required this.hasOverlayBackAction,
    required this.onBack,
    required super.child,
  });

  final bool hasOverlayBackAction;
  final VoidCallback onBack;

  @override
  bool updateShouldNotify(_ShellBackState oldWidget) =>
      hasOverlayBackAction != oldWidget.hasOverlayBackAction;
}

class _AppShellState extends State<AppShell> {
  static const double _mobileSelectionOverlayBottomInset = 84;
  final SelectionController _selectionSessionController =
      SelectionController.instance;
  bool _isQueueOverlayOpen = false;
  bool _isMobilePlayerOverlayOpen = false;
  bool _isAudioOptionsOpen = false;
  double _mobilePlayerOverlayOpenProgress = 0;
  double _mobileQueueOverlayOpenProgress = 0;
  bool _isNavigationRefreshScheduled = false;
  final TextEditingController _desktopSearchController =
      TextEditingController();
  UserData? _observedUserData;

  MashinCoreBridgeEvents get _coreEvents => widget.session.coreEvents;
  MashinCoreBridgeApi get _coreBridge => widget.session.coreBridge;
  UserData? get _userDataController => widget.session.userDataController;
  String get _audioProfile => widget.session.audioProfile;
  bool get _canGoBack => widget.navigatorKey.currentState?.canPop() ?? false;

  List<Playlist> get _sidebarPlaylists =>
      _userDataController?.playlists ?? const <Playlist>[];

  bool get _isLoadingSidebarPlaylists =>
      _userDataController?.isLoadingLibraryData ?? false;

  /// Current playback and queue state
  QueueItem? get _currentQueueItem {
    final items = _coreEvents.mediaSessionState.queue.items;
    final index = _coreEvents.mediaSessionState.queue.currentIndex;
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

    final mediaName = item.mediaItem.name.trim();
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
      return 'Wähle einen Titel zum Starten';
    }

    final Track? track = item.mediaItem is Track
        ? item.mediaItem as Track
        : null;
    final artists =
        track?.artists
            .map((artist) => artist.name.trim())
            .where((name) => name.isNotEmpty)
            .toList() ??
        const <String>[];
    if (artists.isNotEmpty) {
      return artists.join(', ');
    }
    return 'Unbekannte:r Interpret:in';
  }

  Artist? get _currentTrackPrimaryArtist => _currentTrack?.artists.firstOrNull;

  Track? get _currentTrack {
    final MediaItem? mediaItem = _currentQueueItem?.mediaItem;
    return mediaItem is Track ? mediaItem : null;
  }

  double get _currentTrackMaxSeconds {
    final durationFromStateSeconds =
        ((_coreEvents.mediaSessionState.playerState.durationMillis ?? 0) / 1000)
            .toDouble();
    if (durationFromStateSeconds > 0) {
      return durationFromStateSeconds;
    }

    final queueDuration = _currentQueueItem?.duration;
    if (queueDuration != null && queueDuration > 0) {
      return queueDuration.toDouble();
    }

    return 0;
  }

  String? get _currentTrackCoverUrl {
    final url = _currentQueueItem?.imageUriForSize(80)?.trim() ?? '';
    return url.isEmpty ? null : url;
  }

  String? get _currentQueueCoverUrl {
    final url = _currentQueueItem?.imageUriForSize(512)?.trim() ?? '';
    return url.isEmpty ? null : url;
  }

  int get _queueTrackCount => _coreEvents.mediaSessionState.queue.items.length;

  String get _queueTrackCountText => '$_queueTrackCount Tracks';

  String get _queueTotalDurationText {
    int totalSeconds = 0;
    for (final item in _coreEvents.mediaSessionState.queue.items) {
      final int duration = item.duration ?? 0;
      if (duration > 0) {
        totalSeconds += duration;
      }
    }

    final int hours = totalSeconds ~/ 3600;
    final int minutes = (totalSeconds % 3600) ~/ 60;
    final int seconds = totalSeconds % 60;

    if (hours > 0) {
      return '$hours:${minutes.toString().padLeft(2, '0')}:${seconds.toString().padLeft(2, '0')}';
    }
    return '$minutes:${seconds.toString().padLeft(2, '0')}';
  }

  double get _currentTrackPositionSeconds {
    final positionFromStateSeconds =
        ((_coreEvents.mediaSessionState.playerState.positionMillis ?? 0) / 1000)
            .toDouble();
    if (positionFromStateSeconds > 0) {
      return positionFromStateSeconds;
    }

    return 0;
  }

  /// Initialization and lifecycle
  @override
  void initState() {
    super.initState();
    widget.session.addListener(_onSessionChanged);
    widget.navigationObserver.addListener(_onNavigationChanged);
    _observeUserData();
  }

  @override
  void didUpdateWidget(covariant AppShell oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.session != widget.session) {
      oldWidget.session.removeListener(_onSessionChanged);
      widget.session.addListener(_onSessionChanged);
    }
    _observeUserData();
  }

  @override
  void dispose() {
    _desktopSearchController.dispose();
    _observedUserData?.removeListener(_onUserDataChanged);
    widget.session.removeListener(_onSessionChanged);
    widget.navigationObserver.removeListener(_onNavigationChanged);
    super.dispose();
  }

  /// Shell layout
  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (BuildContext context, BoxConstraints constraints) {
        final isDesktop = !isCompactLayout(context);
        final bool hasMobileOverlayBackAction =
            _isQueueOverlayOpen ||
            _mobileQueueOverlayOpenProgress > 0 ||
            _isMobilePlayerOverlayOpen ||
            _mobilePlayerOverlayOpenProgress > 0 ||
            _isAudioOptionsOpen ||
            _selectionSessionController.isVisible;
        return _ShellBackState(
          hasOverlayBackAction: !isDesktop && hasMobileOverlayBackAction,
          onBack: _handleMobileBack,
          child: PopScope<void>(
            canPop: isDesktop || !(hasMobileOverlayBackAction || _canGoBack),
            onPopInvokedWithResult: (bool didPop, void _) {
              if (!didPop && !isDesktop) {
                _handleMobileBack();
              }
            },
            child: Scaffold(
              backgroundColor: context.mashinColors.bgPrimary,
              body: SafeArea(
                top: true,
                bottom: false,
                child: isDesktop
                    ? _buildDesktopLayout(context)
                    : _buildMobileLayout(context),
              ),
            ),
          ),
        );
      },
    );
  }

  Widget _buildDesktopLayout(BuildContext context) {
    final colors = context.mashinColors;
    final AppSection? currentSection = AppRoutes.sectionForLocation(
      GoRouterState.of(context).uri.path,
    );
    final bool isShowingDetail = currentSection == null;

    return Stack(
      children: <Widget>[
        Column(
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
                            style: Theme.of(context).textTheme.titleLarge
                                ?.copyWith(
                                  fontWeight: FontWeight.w700,
                                  color: colors.textPrimary,
                                ),
                          ),
                        ),
                        const SizedBox(height: 22),
                        _DesktopNavItem(
                          label: 'Home',
                          icon: Icons.home_outlined,
                          selected:
                              !isShowingDetail &&
                              currentSection == AppSection.home,
                          onTap: () => _select(AppSection.home),
                        ),
                        _DesktopNavItem(
                          label: 'Entdecken',
                          icon: Icons.explore_outlined,
                          selected:
                              !isShowingDetail &&
                              currentSection == AppSection.explore,
                          onTap: () => _select(AppSection.explore),
                        ),
                        _DesktopNavItem(
                          label: 'Favoriten',
                          icon: Icons.favorite_outlined,
                          selected:
                              !isShowingDetail &&
                              currentSection == AppSection.favorites,
                          onTap: () => _select(AppSection.favorites),
                        ),
                        const SizedBox(height: 14),
                        Divider(height: 1, color: colors.border),
                        Padding(
                          padding: const EdgeInsets.fromLTRB(22, 14, 8, 10),
                          child: Row(
                            children: <Widget>[
                              Expanded(
                                child: Text(
                                  'Playlists',
                                  style: Theme.of(context).textTheme.labelLarge
                                      ?.copyWith(
                                        color: colors.textSecondary,
                                        fontWeight: FontWeight.w600,
                                      ),
                                ),
                              ),
                              if (_isLoadingSidebarPlaylists)
                                SizedBox(
                                  width: 14,
                                  height: 14,
                                  child: CircularProgressIndicator(
                                    strokeWidth: 2,
                                    color: colors.textSecondary,
                                  ),
                                ),
                              if (_isLoadingSidebarPlaylists)
                                const SizedBox(width: 4),
                              Transform.translate(
                                offset: const Offset(0, 0),
                                child: SizedBox(
                                  width: 24,
                                  height: 24,
                                  child: IconButton(
                                    tooltip: 'Wiedergabeliste erstellen',
                                    onPressed: _createPlaylist,
                                    padding: EdgeInsets.zero,
                                    icon: const Icon(Icons.add, size: 15),
                                    constraints: const BoxConstraints.tightFor(
                                      width: 24,
                                      height: 24,
                                    ),
                                  ),
                                ),
                              ),
                            ],
                          ),
                        ),
                        Expanded(
                          child: ListView.builder(
                            padding: const EdgeInsets.symmetric(
                              horizontal: 10,
                              vertical: 0,
                            ),
                            itemCount: _sidebarPlaylists.length,
                            itemBuilder: (BuildContext context, int index) {
                              final Playlist playlist =
                                  _sidebarPlaylists[index];

                              return _DesktopSidebarPlaylistItem(
                                playlist: playlist,
                                tracks:
                                    _userDataController?.tracksForPlaylist(
                                      playlist.itemId,
                                    ) ??
                                    const <Track>[],
                                onTap: () => _openPlaylist(playlist),
                                onPlay: () => _playSidebarPlaylist(playlist),
                                onContextMenu: (Offset position) =>
                                    _showSidebarPlaylistMenu(
                                      playlist,
                                      position,
                                    ),
                              );
                            },
                          ),
                        ),
                      ],
                    ),
                  ),
                  VerticalDivider(width: 1, color: colors.border),
                  Expanded(
                    child: Column(
                      children: <Widget>[
                        _DesktopTopBar(
                          canGoBack: _canGoBack,
                          onBack: _onBack,
                          onToggleTheme: _toggleTheme,
                          isDarkTheme: widget.session.isDarkTheme,
                          searchController: _desktopSearchController,
                          onSearchChanged: _onDesktopSearchChanged,
                          onSearchSubmitted: _submitDesktopSearch,
                          onClearSearch: _clearDesktopSearch,
                        ),
                        Expanded(
                          child: Container(
                            color: colors.bgPrimary,
                            child: widget.child,
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
            _MediaSessionScope(
              mediaSessionChanges: _coreEvents,
              child: _QueueUiScope(
                mediaSessionChanges: _coreEvents,
                builder: (BuildContext context) => _DesktopCurrentTrackInfo(
                  trackTitle: _currentTrackTitle,
                  trackSubtitle: _currentTrackSubtitle,
                  currentTrackArtist: _currentTrackPrimaryArtist,
                  trackCoverUrl: _currentTrackCoverUrl,
                  currentTrack: _currentTrack,
                  userDataController: _userDataController,
                  onOpenArtist: _openArtist,
                ),
              ),
              builder: (BuildContext context, Widget? currentTrackInfo) =>
                  _DesktopPlayerBar(
                    currentTrackInfo:
                        currentTrackInfo ?? const SizedBox.shrink(),
                    playStateType:
                        _coreEvents.mediaSessionState.playerState.playbackState,
                    playbackState: _coreEvents.mediaSessionState.playerState,
                    playbackQueue: _coreEvents.mediaSessionState.queue,
                    dontStopTheMusicEnabled:
                        _coreEvents.mediaSessionState.queue.autoplayEnabled,
                    maxSeconds: _currentTrackMaxSeconds,
                    currentSeconds: _currentTrackPositionSeconds,
                    isQueueOpen: _isQueueOverlayOpen,
                    onTogglePlayPause: _togglePlayPause,
                    onPrevious: _playPrevious,
                    onNext: _playNext,
                    onToggleShuffle: _toggleShuffle,
                    onToggleRepeatMode: _toggleRepeatMode,
                    onToggleDontStopTheMusic: _toggleDontStopTheMusic,
                    onToggleMute: _toggleMute,
                    onSeek: _seek,
                    onVolumeChanged: _setVolume,
                    onOpenQueue: _openQueueOverlay,
                    onOpenAudioOptions: _showAudioOptions,
                  ),
            ),
          ],
        ),
        _QueueUiScope(
          mediaSessionChanges: _coreEvents,
          builder: (BuildContext context) => QueueOverlay(
            isOpen: _isQueueOverlayOpen,
            isDesktop: true,
            desktopBottomInset: _DesktopPlayerBar.barHeight,
            trackTitle: _currentTrackTitle,
            trackSubtitle: _currentTrackSubtitle,
            trackCoverUrl: _currentQueueCoverUrl,
            queueTrackCountText: _queueTrackCountText,
            queueTotalDurationText: _queueTotalDurationText,
            dontStopTheMusicEnabled:
                _coreEvents.mediaSessionState.queue.autoplayEnabled,
            queueItems: _coreEvents.mediaSessionState.queue.items,
            currentQueueIndex: _coreEvents.mediaSessionState.queue.currentIndex,
            coreBridge: _coreBridge,
            coreEvents: _coreEvents,
            userDataController: _userDataController,
            onClose: _closeQueueOverlay,
            onToggleDontStopTheMusic: _toggleDontStopTheMusic,
            onOpenArtist: _openArtistFromQueue,
            onOpenAlbum: _openAlbumFromQueue,
          ),
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
              child: Container(color: colors.bgPrimary, child: widget.child),
            ),
            _MediaSessionScope(
              mediaSessionChanges: _coreEvents,
              child: _QueueUiScope(
                mediaSessionChanges: _coreEvents,
                builder: (BuildContext context) => _MobileCurrentTrackInfo(
                  trackTitle: _currentTrackTitle,
                  trackSubtitle: _currentTrackSubtitle,
                  trackCoverUrl: _currentTrackCoverUrl,
                ),
              ),
              builder: (BuildContext context, Widget? currentTrackInfo) =>
                  _MobilePlayerBar(
                    currentTrackInfo:
                        currentTrackInfo ?? const SizedBox.shrink(),
                    playbackState:
                        _coreEvents.mediaSessionState.playerState.playbackState,
                    currentSeconds: _currentTrackPositionSeconds,
                    maxSeconds: _currentTrackMaxSeconds,
                    trackTransitionKey: _currentQueueItem?.queueItemId ?? '',
                    onTogglePlayPause: _togglePlayPause,
                    onPrevious: _playPrevious,
                    onNext: _playNext,
                    onOpenPlayerOverlay: _openMobilePlayerOverlay,
                    onPlayerOverlayDragUpdate: _updateMobilePlayerOverlayDrag,
                    onPlayerOverlayDragEnd: _endMobilePlayerOverlayDrag,
                  ),
            ),
            _MobileBottomNav(
              selectedSection: AppRoutes.sectionForLocation(
                GoRouterState.of(context).uri.path,
              ),
              onSelected: _select,
            ),
          ],
        ),
        _QueueUiScope(
          mediaSessionChanges: _coreEvents,
          builder: (BuildContext context) => _MediaSessionScope(
            mediaSessionChanges: _coreEvents,
            builder: (BuildContext context, Widget? child) => PlayerBarOverlay(
              isOpen: _isMobilePlayerOverlayOpen,
              openProgress: _mobilePlayerOverlayOpenProgress,
              trackTitle: _currentTrackTitle,
              trackSubtitle: _currentTrackSubtitle,
              trackCoverUrl: _currentQueueCoverUrl,
              currentTrack: _currentTrack,
              playbackStateType:
                  _coreEvents.mediaSessionState.playerState.playbackState,
              playbackState: _coreEvents.mediaSessionState.playerState,
              playbackQueue: _coreEvents.mediaSessionState.queue,
              maxSeconds: _currentTrackMaxSeconds,
              currentSeconds: _currentTrackPositionSeconds,
              dontStopTheMusicEnabled:
                  _coreEvents.mediaSessionState.queue.autoplayEnabled,
              userDataController: _userDataController,
              onClose: _closeMobilePlayerOverlay,
              onOpenQueue: _openQueueFromMobilePlayerOverlay,
              onQueueDragUpdate: _updateMobileQueueOverlayDrag,
              onQueueDragEnd: _endMobileQueueOverlayDrag,
              onTogglePlayPause: _togglePlayPause,
              onPrevious: _playPrevious,
              onNext: _playNext,
              onToggleShuffle: _toggleShuffle,
              onToggleRepeatMode: _toggleRepeatMode,
              onToggleDontStopTheMusic: _toggleDontStopTheMusic,
              onToggleMute: _toggleMute,
              onSeek: _seek,
              onOpenAudioOptions: _showAudioOptions,
              onOpenArtist: _openArtistFromMobilePlayerOverlay,
              onOpenAlbum: _openAlbumFromMobilePlayerOverlay,
            ),
          ),
        ),
        _QueueUiScope(
          mediaSessionChanges: _coreEvents,
          builder: (BuildContext context) => QueueOverlay(
            isOpen: _isQueueOverlayOpen,
            isDesktop: false,
            mobileOpenProgress: _mobileQueueOverlayOpenProgress,
            trackTitle: _currentTrackTitle,
            trackSubtitle: _currentTrackSubtitle,
            trackCoverUrl: _currentTrackCoverUrl,
            queueTrackCountText: _queueTrackCountText,
            queueTotalDurationText: _queueTotalDurationText,
            dontStopTheMusicEnabled:
                _coreEvents.mediaSessionState.queue.autoplayEnabled,
            queueItems: _coreEvents.mediaSessionState.queue.items,
            currentQueueIndex: _coreEvents.mediaSessionState.queue.currentIndex,
            coreBridge: _coreBridge,
            coreEvents: _coreEvents,
            userDataController: _userDataController,
            onClose: _closeQueueOverlay,
            onToggleDontStopTheMusic: _toggleDontStopTheMusic,
            onOpenArtist: _openArtistFromQueue,
            onOpenAlbum: _openAlbumFromQueue,
          ),
        ),
        AnimatedBuilder(
          animation: _selectionSessionController,
          builder: (BuildContext context, Widget? child) {
            if (!_selectionSessionController.isVisible) {
              return const SizedBox.shrink();
            }

            return Positioned(
              right: 8,
              bottom: _mobileSelectionOverlayBottomInset + 72,
              child: SelectionActionsOverlay(
                allSelected: _selectionSessionController.allSelected,
                selectedCount: _selectionSessionController.selectedCount,
                itemTypeLabel: _selectionSessionController.itemTypeLabel,
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

  void _openQueueOverlay() {
    setState(() {
      _isQueueOverlayOpen = !_isQueueOverlayOpen;
      if (_isQueueOverlayOpen) {
        _isMobilePlayerOverlayOpen = false;
      }
    });
  }

  void _openMobilePlayerOverlay() {
    if (_isMobilePlayerOverlayOpen) {
      return;
    }
    setState(() {
      _isQueueOverlayOpen = false;
      _mobilePlayerOverlayOpenProgress = 0;
      _mobileQueueOverlayOpenProgress = 0;
      _isMobilePlayerOverlayOpen = true;
    });
  }

  void _updateMobilePlayerOverlayDrag(
    double upwardDelta,
    double viewportHeight,
  ) {
    if (_isMobilePlayerOverlayOpen || viewportHeight <= 0) {
      return;
    }

    setState(() {
      _isQueueOverlayOpen = false;
      _mobileQueueOverlayOpenProgress = 0;
      _mobilePlayerOverlayOpenProgress =
          (_mobilePlayerOverlayOpenProgress + (upwardDelta / viewportHeight))
              .clamp(0, 1)
              .toDouble();
    });
  }

  void _endMobilePlayerOverlayDrag(double primaryVelocity) {
    if (_isMobilePlayerOverlayOpen && _mobilePlayerOverlayOpenProgress == 0) {
      return;
    }

    final bool shouldOpen =
        _mobilePlayerOverlayOpenProgress >= 0.22 || primaryVelocity < -450;
    setState(() {
      _mobilePlayerOverlayOpenProgress = 0;
      if (shouldOpen) {
        _isQueueOverlayOpen = false;
        _isMobilePlayerOverlayOpen = true;
      }
    });
  }

  void _closeMobilePlayerOverlay() {
    if (!_isMobilePlayerOverlayOpen &&
        _mobilePlayerOverlayOpenProgress == 0 &&
        _mobileQueueOverlayOpenProgress == 0) {
      return;
    }
    setState(() {
      _isMobilePlayerOverlayOpen = false;
      _mobilePlayerOverlayOpenProgress = 0;
      _mobileQueueOverlayOpenProgress = 0;
    });
  }

  Future<void> _openArtistFromMobilePlayerOverlay(Artist artist) async {
    _closeMobilePlayerOverlay();
    await Future<void>.delayed(QueueOverlay.animationDuration);
    if (!mounted) {
      return;
    }
    _openArtist(artist);
  }

  Future<void> _openAlbumFromMobilePlayerOverlay(Album album) async {
    _closeMobilePlayerOverlay();
    await Future<void>.delayed(QueueOverlay.animationDuration);
    if (!mounted) {
      return;
    }
    _openAlbum(album);
  }

  void _openQueueFromMobilePlayerOverlay() {
    setState(() {
      _isMobilePlayerOverlayOpen = true;
      _mobilePlayerOverlayOpenProgress = 0;
      _mobileQueueOverlayOpenProgress = 0;
      _isQueueOverlayOpen = true;
    });
  }

  void _updateMobileQueueOverlayDrag(
    double upwardDelta,
    double viewportHeight,
  ) {
    if (!_isMobilePlayerOverlayOpen ||
        _isQueueOverlayOpen ||
        viewportHeight <= 0) {
      return;
    }

    setState(() {
      _mobileQueueOverlayOpenProgress =
          (_mobileQueueOverlayOpenProgress + (upwardDelta / viewportHeight))
              .clamp(0, 1)
              .toDouble();
    });
  }

  void _endMobileQueueOverlayDrag(double primaryVelocity) {
    if (_isQueueOverlayOpen && _mobileQueueOverlayOpenProgress == 0) {
      return;
    }

    final bool shouldOpen =
        _mobileQueueOverlayOpenProgress >= 0.22 || primaryVelocity < -450;
    setState(() {
      _mobileQueueOverlayOpenProgress = 0;
      if (shouldOpen) {
        _isMobilePlayerOverlayOpen = true;
        _isQueueOverlayOpen = true;
      }
    });
  }

  void _closeQueueOverlay() {
    if (!_isQueueOverlayOpen && _mobileQueueOverlayOpenProgress == 0) {
      return;
    }
    setState(() {
      _isQueueOverlayOpen = false;
      _mobileQueueOverlayOpenProgress = 0;
    });
  }

  Future<void> _openArtistFromQueue(Artist artist) async {
    _closeQueueOverlay();
    _closeMobilePlayerOverlay();
    await Future<void>.delayed(QueueOverlay.animationDuration);
    if (!mounted) {
      return;
    }
    _openArtist(artist);
  }

  Future<void> _openAlbumFromQueue(Album album) async {
    _closeQueueOverlay();
    _closeMobilePlayerOverlay();
    await Future<void>.delayed(QueueOverlay.animationDuration);
    if (!mounted) {
      return;
    }
    _openAlbum(album);
  }

  void _openArtist(Artist artist) {
    context.push(AppRoutes.artist(artist));
  }

  void _openAlbum(Album album) {
    context.push(AppRoutes.album(album));
  }

  void _openPlaylist(Playlist playlist) {
    context.push(AppRoutes.playlist(playlist));
  }

  void _select(AppSection section) {
    final String destination = AppRoutes.locationFor(section);
    if (GoRouterState.of(context).uri.path == destination) {
      return;
    }

    context.push(destination);
  }

  void _handleMobileBack() {
    if (_isAudioOptionsOpen) {
      Navigator.of(context, rootNavigator: true).maybePop();
      return;
    }

    if (_selectionSessionController.isVisible) {
      _selectionSessionController.onClose();
      return;
    }

    if (_isQueueOverlayOpen || _mobileQueueOverlayOpenProgress > 0) {
      _closeQueueOverlay();
      return;
    }

    if (_isMobilePlayerOverlayOpen || _mobilePlayerOverlayOpenProgress > 0) {
      _closeMobilePlayerOverlay();
      return;
    }

    _popNavigationRoute();
  }

  void _popNavigationRoute() {
    unawaited(widget.navigatorKey.currentState?.maybePop() ?? Future.value());
  }

  /// Desktop search and playlist actions

  void _onDesktopSearchChanged(String value) {
    // Rebuild keeps clear button visibility in sync while typing.
    setState(() {});
  }

  void _submitDesktopSearch(String value) {
    final trimmed = value.trim();
    context.go(
      Uri(
        path: AppRoutes.search,
        queryParameters: <String, String>{if (trimmed.isNotEmpty) 'q': trimmed},
      ).toString(),
    );
  }

  void _clearDesktopSearch() {
    _desktopSearchController.clear();
    context.go(AppRoutes.search);
  }

  Future<void> _createPlaylist() async {
    final UserData? controller = _userDataController;
    if (controller == null || !mounted) {
      return;
    }

    final String? name = await showPlaylistNameDialog(context: context);
    if (name != null) {
      await controller.createPrivatePlaylist(name);
    }
  }

  Future<void> _renamePlaylist(Playlist playlist) async {
    final UserData? controller = _userDataController;
    if (controller == null || !mounted || !playlist.isEditable) {
      return;
    }

    final String? name = await showPlaylistNameDialog(
      context: context,
      playlist: playlist,
    );
    if (name != null) {
      await controller.renamePlaylist(playlist, name);
    }
  }

  Future<void> _showSidebarPlaylistMenu(
    Playlist playlist,
    Offset position,
  ) async {
    if (!playlist.isEditable) {
      return;
    }
    await const AppContextMenuController().showContextMenu(
      context: context,
      globalPosition: position,
      actions: <ContextMenuAction>[
        ContextMenuAction(
          label: 'Abspielen',
          icon: Icons.play_arrow,
          onSelected: () => _playSidebarPlaylist(playlist),
          isEnabled: _playlistTracks(playlist).isNotEmpty,
        ),
        ContextMenuAction(
          label: 'Zufällig abspielen',
          icon: Icons.shuffle_outlined,
          onSelected: () => _shuffleSidebarPlaylist(playlist),
          isEnabled: _playlistTracks(playlist).isNotEmpty,
        ),
        ContextMenuAction(
          label: 'Als Nächstes spielen',
          icon: Icons.keyboard_return,
          iconQuarterTurns: 2,
          onSelected: () => _playSidebarPlaylistNext(playlist),
          isEnabled: _playlistTracks(playlist).isNotEmpty,
        ),
        ContextMenuAction(
          label: 'Als Letztes spielen',
          icon: Icons.last_page,
          onSelected: () => _playSidebarPlaylistLast(playlist),
          isEnabled: _playlistTracks(playlist).isNotEmpty,
        ),
        const ContextMenuAction.separator(),
        ContextMenuAction(
          label: 'Umbenennen',
          icon: Icons.edit_outlined,
          onSelected: () => _renamePlaylist(playlist),
          isEnabled: playlist.isEditable,
        ),
        ContextMenuAction(
          label: 'Cover aktualisieren',
          icon: Icons.image_outlined,
          onSelected: () =>
              _userDataController!.refreshPrivatePlaylistCover(playlist),
          isEnabled: playlist.isEditable,
        ),
        ContextMenuAction(
          label: 'Löschen',
          icon: Icons.delete_outline,
          onSelected: () => _deleteSidebarPlaylist(playlist),
          isEnabled: playlist.isEditable,
          isDestructive: true,
        ),
      ],
    );
  }

  List<Track> _playlistTracks(Playlist playlist) {
    return _userDataController?.tracksForPlaylist(playlist.itemId) ??
        const <Track>[];
  }

  Future<void> _playSidebarPlaylist(Playlist playlist) async {
    final List<Track> tracks = _playlistTracks(playlist);
    if (tracks.isNotEmpty) await _coreBridge.queuePlayMedia(tracks);
  }

  Future<void> _shuffleSidebarPlaylist(Playlist playlist) async {
    final List<Track> tracks = _playlistTracks(playlist);
    if (tracks.isNotEmpty) {
      await _coreBridge.queuePlayMedia(tracks, shuffle: true);
    }
  }

  Future<void> _playSidebarPlaylistNext(Playlist playlist) async {
    final List<Track> tracks = _playlistTracks(playlist);
    if (tracks.isNotEmpty) {
      await _coreBridge.queuePlayMedia(tracks, option: QueueOption.next);
    }
  }

  Future<void> _playSidebarPlaylistLast(Playlist playlist) async {
    final List<Track> tracks = _playlistTracks(playlist);
    if (tracks.isNotEmpty) {
      await _coreBridge.queuePlayMedia(tracks, option: QueueOption.add);
    }
  }

  Future<void> _deleteSidebarPlaylist(Playlist playlist) async {
    final UserData? controller = _userDataController;
    if (controller == null || !playlist.isEditable || !mounted) return;

    final bool confirmed = await showDeletePlaylistDialog(
      context: context,
      playlistName: playlist.name,
    );
    if (confirmed) await controller.removePlaylist(playlist);
  }

  void _toggleTheme() {
    unawaited(widget.session.setDarkTheme(!widget.session.isDarkTheme));
  }

  void _onBack() {
    _popNavigationRoute();
  }

  /// Playback command handlers

  Future<void> _togglePlayPause() async {
    try {
      if (_coreEvents.mediaSessionState.playerState.playbackState ==
          PlayState.playing) {
        await _coreBridge.transportPause();
      } else {
        await _coreBridge.transportPlay();
      }
    } catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Wiedergabe fehlgeschlagen: $error')),
        );
      }
    }
  }

  Future<void> _playPrevious() async {
    try {
      await _coreBridge.transportPrevious();
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  Future<void> _playNext() async {
    try {
      await _coreBridge.transportNext();
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  Future<void> _toggleShuffle() async {
    try {
      await _coreBridge.transportShuffle(
        !_coreEvents.mediaSessionState.queue.shuffleEnabled,
      );
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  Future<void> _toggleRepeatMode() async {
    try {
      final current = _coreEvents.mediaSessionState.queue.repeatMode;
      final next = switch (current) {
        RepeatMode.off => RepeatMode.all,
        RepeatMode.all => RepeatMode.one,
        RepeatMode.one => RepeatMode.off,
      };
      await _coreBridge.transportRepeatMode(next);
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  Future<void> _toggleDontStopTheMusic() async {
    try {
      final enabled = _coreEvents.mediaSessionState.queue.autoplayEnabled;
      await _coreBridge.setAutoplay(!enabled);
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  Future<void> _toggleMute() async {
    try {
      await _coreBridge.setMuted(
        !_coreEvents.mediaSessionState.playerState.muted,
      );
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  Future<void> _setVolume(double next) async {
    final int safeVolume = next.round().clamp(0, 100);
    try {
      await _coreBridge.setVolume(safeVolume);
    } catch (error, stackTrace) {
      _coreBridge.log(
        LogLevelPreference.error,
        'AppShell',
        'Set volume failed: $error | $stackTrace',
      );
    }
  }

  Future<void> _showAudioOptions(BuildContext anchorContext) async {
    if (isCompactLayout(context)) {
      setState(() => _isAudioOptionsOpen = true);
    }
    try {
      await const AppContextMenuController().showCustomContextMenu(
        context: context,
        anchorContext: anchorContext,
        width: 280,
        child: _AudioOptionsMenu(
          selectedAudioProfile: _audioProfile,
          onSelected: _setAudioProfile,
        ),
      );
    } finally {
      if (mounted && _isAudioOptionsOpen) {
        setState(() => _isAudioOptionsOpen = false);
      }
    }
  }

  Future<void> _setAudioProfile(String audioProfile) async {
    Navigator.of(context, rootNavigator: true).pop();
    if (audioProfile == _audioProfile) {
      return;
    }

    try {
      await widget.session.setAudioProfile(audioProfile);
    } catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Profilwechsel fehlgeschlagen: $error')),
        );
      }
    }
  }

  Future<void> _seek(double seconds) async {
    try {
      await _coreBridge.transportSeek(seconds);
    } catch (_) {
      // Ignore playback command failures in shell-level controls.
    }
  }

  void _onSessionChanged() {
    if (!mounted) {
      return;
    }
    _observeUserData();
    setState(() {});
  }

  void _observeUserData() {
    final UserData? controller = _userDataController;
    if (identical(controller, _observedUserData)) return;
    _observedUserData?.removeListener(_onUserDataChanged);
    _observedUserData = controller;
    controller?.addListener(_onUserDataChanged);
  }

  void _onUserDataChanged() {
    if (!mounted) return;
    setState(() {});
  }

  void _onNavigationChanged() {
    if (!mounted) {
      return;
    }

    if (_selectionSessionController.isVisible) {
      _selectionSessionController.onClose();
    }

    if (_isNavigationRefreshScheduled) {
      return;
    }

    _isNavigationRefreshScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _isNavigationRefreshScheduled = false;
      if (mounted) {
        setState(() {});
      }
    });
  }
}

/// Media-session render-state scopes.
class _MediaSessionScope extends StatelessWidget {
  const _MediaSessionScope({
    required this.mediaSessionChanges,
    required this.builder,
    this.child,
  });

  final Listenable mediaSessionChanges;
  final TransitionBuilder builder;
  final Widget? child;

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: mediaSessionChanges,
      builder: builder,
      child: child,
    );
  }
}

class _QueueUiScope extends StatelessWidget {
  const _QueueUiScope({
    required this.mediaSessionChanges,
    required this.builder,
  });

  final Listenable mediaSessionChanges;
  final WidgetBuilder builder;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: mediaSessionChanges,
      builder: (BuildContext context, Widget? child) => builder(context),
    );
  }
}

/// Desktop UI components
enum _DesktopUserMenuAction { settings }

class _DesktopTopBar extends StatelessWidget {
  final bool canGoBack;
  final VoidCallback onBack;
  final VoidCallback onToggleTheme;
  final bool isDarkTheme;
  final TextEditingController searchController;
  final ValueChanged<String> onSearchChanged;
  final ValueChanged<String> onSearchSubmitted;
  final VoidCallback onClearSearch;

  const _DesktopTopBar({
    required this.canGoBack,
    required this.onBack,
    required this.onToggleTheme,
    required this.isDarkTheme,
    required this.searchController,
    required this.onSearchChanged,
    required this.onSearchSubmitted,
    required this.onClearSearch,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Container(
      height: 62,
      decoration: BoxDecoration(
        color: colors.bgPrimary,
        border: Border(bottom: BorderSide(color: colors.border)),
      ),
      child: Row(
        children: <Widget>[
          const SizedBox(width: 22),
          Material(
            color: Colors.transparent,
            borderRadius: BorderRadius.circular(10),
            child: InkWell(
              borderRadius: BorderRadius.circular(10),
              hoverColor: colors.hover,
              splashColor: Colors.transparent,
              highlightColor: Colors.transparent,
              onTap: canGoBack ? onBack : null,
              child: Padding(
                padding: const EdgeInsets.all(8),
                child: Icon(
                  Icons.chevron_left_outlined,
                  color: canGoBack ? colors.textSecondary : colors.textTertiary,
                ),
              ),
            ),
          ),
          const SizedBox(width: 18),
          Expanded(
            child: SizedBox(
              height: 36,
              child: TextField(
                controller: searchController,
                textInputAction: TextInputAction.search,
                style: const TextStyle(fontSize: 14),
                decoration: InputDecoration(
                  isDense: true,
                  hintText: 'Künstler:innen, Songs, Alben ...',
                  hintStyle: TextStyle(
                    color: colors.textTertiary,
                    fontSize: 14,
                  ),
                  prefixIcon: const Icon(Icons.search_outlined, size: 18),
                  prefixIconConstraints: const BoxConstraints(
                    minWidth: 38,
                    minHeight: 36,
                  ),
                  suffixIcon: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: <Widget>[
                      if (searchController.text.trim().isNotEmpty)
                        IconButton(
                          tooltip: 'Leeren',
                          onPressed: onClearSearch,
                          padding: EdgeInsets.zero,
                          constraints: const BoxConstraints(
                            minWidth: 36,
                            minHeight: 36,
                          ),
                          icon: const Icon(Icons.close_outlined, size: 16),
                        ),
                      IconButton(
                        tooltip: 'Suchen',
                        onPressed: () =>
                            onSearchSubmitted(searchController.text),
                        padding: EdgeInsets.zero,
                        constraints: const BoxConstraints(
                          minWidth: 36,
                          minHeight: 36,
                        ),
                        icon: const Icon(Icons.search_outlined, size: 16),
                      ),
                      const SizedBox(width: 6),
                    ],
                  ),
                  filled: true,
                  fillColor: colors.bgSecondary,
                  contentPadding: const EdgeInsets.symmetric(
                    horizontal: 12,
                    vertical: 0,
                  ),
                  border: OutlineInputBorder(
                    borderRadius: BorderRadius.circular(18),
                    borderSide: BorderSide.none,
                  ),
                ),
                onChanged: onSearchChanged,
                onSubmitted: onSearchSubmitted,
              ),
            ),
          ),
          const SizedBox(width: 14),
          IconButton(
            tooltip: 'Theme wechseln',
            onPressed: onToggleTheme,
            padding: EdgeInsets.zero,
            constraints: const BoxConstraints(minWidth: 32, minHeight: 32),
            visualDensity: VisualDensity.compact,
            icon: Icon(
              isDarkTheme
                  ? Icons.light_mode_outlined
                  : Icons.dark_mode_outlined,
              size: 18,
            ),
          ),
          const SizedBox(width: 12),
          PopupMenuButton<_DesktopUserMenuAction>(
            tooltip: 'Benutzermenü',
            icon: CircleAvatar(
              radius: 16,
              backgroundColor: colors.bgTertiary,
              child: Icon(
                Icons.person_outlined,
                size: 18,
                color: colors.textSecondary,
              ),
            ),
            onSelected: (_DesktopUserMenuAction action) {
              if (action == _DesktopUserMenuAction.settings) {
                context.push(AppRoutes.settings);
              }
            },
            itemBuilder: (BuildContext context) =>
                const <PopupMenuEntry<_DesktopUserMenuAction>>[
                  PopupMenuItem<_DesktopUserMenuAction>(
                    value: _DesktopUserMenuAction.settings,
                    child: ListTile(
                      leading: Icon(Icons.settings_outlined),
                      title: Text('Einstellungen'),
                    ),
                  ),
                ],
          ),
          const SizedBox(width: 24),
        ],
      ),
    );
  }
}

class _DesktopNavItem extends StatefulWidget {
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
  State<_DesktopNavItem> createState() => _DesktopNavItemState();
}

class _DesktopNavItemState extends State<_DesktopNavItem> {
  bool _isHovered = false;

  void _setHovered(bool value) {
    if (_isHovered == value) {
      return;
    }
    setState(() {
      _isHovered = value;
    });
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 2),
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => _setHovered(true),
        onExit: (_) => _setHovered(false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onTap,
          child: Container(
            decoration: BoxDecoration(
              color: _isHovered ? colors.hover : Colors.transparent,
              borderRadius: BorderRadius.circular(10),
            ),
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 12),
            child: Row(
              children: <Widget>[
                Icon(
                  widget.icon,
                  size: 20,
                  color: widget.selected ? colors.accent : colors.textSecondary,
                ),
                const SizedBox(width: 10),
                Text(
                  widget.label,
                  style: TextStyle(
                    color: widget.selected
                        ? colors.accent
                        : colors.textSecondary,
                    fontWeight: widget.selected
                        ? FontWeight.w600
                        : FontWeight.w500,
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

class _DesktopSidebarPlaylistItem extends StatefulWidget {
  const _DesktopSidebarPlaylistItem({
    required this.playlist,
    required this.tracks,
    required this.onTap,
    required this.onPlay,
    required this.onContextMenu,
  });

  final Playlist playlist;
  final List<Track> tracks;
  final VoidCallback onTap;
  final VoidCallback onPlay;
  final ValueChanged<Offset> onContextMenu;

  @override
  State<_DesktopSidebarPlaylistItem> createState() =>
      _DesktopSidebarPlaylistItemState();
}

class _DesktopSidebarPlaylistItemState
    extends State<_DesktopSidebarPlaylistItem> {
  bool _isHovered = false;
  bool _isCoverHovered = false;
  bool _isPlayHovered = false;

  void _setHovered(bool value) {
    if (_isHovered == value) {
      return;
    }
    setState(() {
      _isHovered = value;
    });
  }

  void _setPlayHovered(bool value) {
    if (_isPlayHovered == value) {
      return;
    }
    setState(() {
      _isPlayHovered = value;
    });
  }

  void _setCoverHovered(bool value) {
    if (_isCoverHovered == value && (value || !_isPlayHovered)) {
      return;
    }
    setState(() {
      _isCoverHovered = value;
      if (!value) {
        _isPlayHovered = false;
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final Playlist playlist = widget.playlist;
    final List<Track> tracks = widget.tracks;
    final String title = playlist.name.trim().isEmpty
        ? 'Unknown Playlist'
        : playlist.name.trim();
    final String? coverUrl = playlist.imageUriForSize(40)?.trim();
    final int totalSeconds = tracks.fold<int>(
      0,
      (int sum, Track track) => sum + track.duration,
    );
    final int hours = totalSeconds ~/ 3600;
    final int minutes = (totalSeconds % 3600) ~/ 60;
    final String totalDurationText = hours > 0
        ? '$hours Std. $minutes Min.'
        : '$minutes Min.';
    final String tracksCountText = tracks.length == 1
        ? '1 Titel'
        : '${tracks.length} Titel';
    final String metadataText = '$tracksCountText • $totalDurationText';

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => _setHovered(true),
        onExit: (_) => _setHovered(false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onTap,
          onSecondaryTapUp: (TapUpDetails details) =>
              widget.onContextMenu(details.globalPosition),
          child: Container(
            decoration: BoxDecoration(
              color: _isHovered ? colors.hover : Colors.transparent,
              borderRadius: BorderRadius.circular(10),
            ),
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
            child: Row(
              children: <Widget>[
                MouseRegion(
                  onEnter: (_) => _setCoverHovered(true),
                  onExit: (_) => _setCoverHovered(false),
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(6),
                    child: SizedBox(
                      width: 40,
                      height: 40,
                      child: Stack(
                        fit: StackFit.expand,
                        children: <Widget>[
                          coverUrl == null || coverUrl.isEmpty
                              ? ColoredBox(color: colors.bgTertiary)
                              : MediaImage(
                                  source: coverUrl,
                                  fit: BoxFit.cover,
                                  fallback: ColoredBox(
                                    color: colors.bgTertiary,
                                  ),
                                ),
                          if (tracks.isNotEmpty)
                            Positioned.fill(
                              child: IgnorePointer(
                                ignoring: !_isCoverHovered,
                                child: AnimatedOpacity(
                                  opacity: _isCoverHovered ? 1 : 0,
                                  duration: const Duration(milliseconds: 140),
                                  child: ColoredBox(
                                    color: Colors.black.withValues(
                                      alpha: colors.coverHoverOverlayOpacity,
                                    ),
                                    child: Center(
                                      child: MouseRegion(
                                        onEnter: (_) => _setPlayHovered(true),
                                        onExit: (_) => _setPlayHovered(false),
                                        child: AnimatedScale(
                                          scale: _isPlayHovered ? 1.08 : 1,
                                          duration: const Duration(
                                            milliseconds: 140,
                                          ),
                                          curve: Curves.easeOutCubic,
                                          child: GestureDetector(
                                            onTap: widget.onPlay,
                                            child: Container(
                                              width: 28,
                                              height: 28,
                                              decoration: BoxDecoration(
                                                color: colors
                                                    .overlayPlayButtonBackground,
                                                borderRadius:
                                                    BorderRadius.circular(14),
                                              ),
                                              child: Icon(
                                                Icons.play_arrow_sharp,
                                                size: 18,
                                                color: colors
                                                    .overlayPlayButtonForeground,
                                              ),
                                            ),
                                          ),
                                        ),
                                      ),
                                    ),
                                  ),
                                ),
                              ),
                            ),
                        ],
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: <Widget>[
                      Text(
                        title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          color: colors.textPrimary,
                          fontSize: 13,
                        ),
                      ),
                      const SizedBox(height: 4),
                      Text(
                        metadataText,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          color: colors.textTertiary,
                          fontSize: 11,
                        ),
                      ),
                    ],
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
  static const double barHeight = 90;

  final Widget currentTrackInfo;
  final PlayState playStateType;
  final PlayerState playbackState;
  final Queue playbackQueue;
  final bool dontStopTheMusicEnabled;
  final double maxSeconds;
  final double currentSeconds;
  final bool isQueueOpen;
  final Future<void> Function() onTogglePlayPause;
  final Future<void> Function() onPrevious;
  final Future<void> Function() onNext;
  final Future<void> Function() onToggleShuffle;
  final Future<void> Function() onToggleRepeatMode;
  final Future<void> Function() onToggleDontStopTheMusic;
  final Future<void> Function() onToggleMute;
  final Future<void> Function(double) onSeek;
  final ValueChanged<double> onVolumeChanged;
  final VoidCallback onOpenQueue;
  final ValueChanged<BuildContext> onOpenAudioOptions;

  const _DesktopPlayerBar({
    required this.currentTrackInfo,
    required this.playStateType,
    required this.playbackState,
    required this.playbackQueue,
    required this.dontStopTheMusicEnabled,
    required this.maxSeconds,
    required this.currentSeconds,
    required this.isQueueOpen,
    required this.onTogglePlayPause,
    required this.onPrevious,
    required this.onNext,
    required this.onToggleShuffle,
    required this.onToggleRepeatMode,
    required this.onToggleDontStopTheMusic,
    required this.onToggleMute,
    required this.onSeek,
    required this.onVolumeChanged,
    required this.onOpenQueue,
    required this.onOpenAudioOptions,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool hasPlaybackSession = playStateType != PlayState.uninitialized;
    final bool isPlaying = playStateType == PlayState.playing;
    final bool isBusy =
        playStateType == PlayState.buffering ||
        playStateType == PlayState.seeking;
    final bool isShuffleActive = playbackQueue.shuffleEnabled;
    final RepeatMode repeatMode = playbackQueue.repeatMode;
    final bool isRepeatOne = repeatMode == RepeatMode.one;
    final IconData repeatIcon = isRepeatOne
        ? Icons.repeat_one_outlined
        : Icons.repeat_outlined;
    final Color repeatColor = repeatMode == RepeatMode.off
        ? colors.textSecondary
        : colors.accent;
    final double clampedMaxSeconds = maxSeconds.clamp(0, 36000).toDouble();
    final double clampedCurrentSeconds = currentSeconds
        .clamp(0, clampedMaxSeconds > 0 ? clampedMaxSeconds : 1)
        .toDouble();

    return Container(
      height: barHeight,
      decoration: BoxDecoration(
        color: colors.bgSecondary,
        border: Border(top: BorderSide(color: colors.border)),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 20),
        child: Row(
          children: <Widget>[
            currentTrackInfo,
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
                        color: isShuffleActive
                            ? colors.accent
                            : colors.textSecondary,
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
                          disabledBackgroundColor: colors.textPrimary
                              .withValues(alpha: 0.45),
                          disabledForegroundColor: colors.bgPrimary,
                          elevation: 0,
                        ),
                        child: isBusy
                            ? SizedBox(
                                width: 16,
                                height: 16,
                                child: CircularProgressIndicator(
                                  strokeWidth: 2,
                                  valueColor: AlwaysStoppedAnimation<Color>(
                                    colors.bgPrimary,
                                  ),
                                ),
                              )
                            : Icon(
                                isPlaying
                                    ? Icons.pause_sharp
                                    : Icons.play_arrow_sharp,
                                size: 30,
                              ),
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
                          style: TextStyle(
                            color: colors.textTertiary,
                            fontSize: 11,
                          ),
                        ),
                        const SizedBox(width: 10),
                        Expanded(
                          child: SliderTheme(
                            data: SliderTheme.of(context).copyWith(
                              trackHeight: 3,
                              thumbShape: const RoundSliderThumbShape(
                                enabledThumbRadius: 7,
                                disabledThumbRadius: 7,
                              ),
                              overlayShape: const RoundSliderOverlayShape(
                                overlayRadius: 10,
                              ),
                              thumbColor: colors.textSecondary,
                              activeTrackColor: colors.textSecondary,
                              secondaryActiveTrackColor: colors.textTertiary,
                              inactiveTrackColor: colors.bgTertiary,
                            ),
                            child: PlaybackSeekSlider(
                              value: clampedCurrentSeconds,
                              bufferedValue:
                                  clampedMaxSeconds > 0 &&
                                      playbackState.bufferedPositionMillis !=
                                          null
                                  ? playbackState.bufferedPositionMillis! / 1000
                                  : null,
                              min: 0,
                              max: clampedMaxSeconds > 0
                                  ? clampedMaxSeconds
                                  : 1,
                              enabled: hasPlaybackSession,
                              onSeek: onSeek,
                            ),
                          ),
                        ),
                        const SizedBox(width: 10),
                        Text(
                          _formatSeconds(clampedMaxSeconds),
                          style: TextStyle(
                            color: colors.textTertiary,
                            fontSize: 11,
                          ),
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
                  _PlayerBarIconButton(
                    icon: Icons.queue_music_outlined,
                    onTap: onOpenQueue,
                    color: isQueueOpen ? colors.accent : colors.textSecondary,
                  ),
                  _PlayerBarIconButton(
                    icon: Icons.cell_tower_outlined,
                    onTap: hasPlaybackSession ? onToggleDontStopTheMusic : null,
                    color: dontStopTheMusicEnabled
                        ? colors.accent
                        : colors.textSecondary,
                  ),
                  _PlayerBarIconButton(
                    icon: Icons.devices_outlined,
                    onTap: () {},
                    color: colors.textSecondary,
                  ),
                  _PlayerBarIconButton(
                    icon: playbackState.muted
                        ? Icons.volume_off_outlined
                        : Icons.volume_up_outlined,
                    onTap: onToggleMute,
                    color: playbackState.muted
                        ? colors.accent
                        : colors.textSecondary,
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
                  Builder(
                    builder: (BuildContext buttonContext) =>
                        _PlayerBarIconButton(
                          icon: Icons.tune_outlined,
                          onTap: () => onOpenAudioOptions(buttonContext),
                          color: colors.textSecondary,
                        ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _AudioOptionsMenu extends StatelessWidget {
  const _AudioOptionsMenu({
    required this.selectedAudioProfile,
    required this.onSelected,
  });

  final String selectedAudioProfile;
  final ValueChanged<String> onSelected;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Material(
      color: colors.bgSecondary,
      elevation: 12,
      borderRadius: BorderRadius.circular(10),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 8),
        child: RadioGroup<String>(
          groupValue: selectedAudioProfile,
          onChanged: (String? value) {
            if (value != null) {
              onSelected(value);
            }
          },
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: <Widget>[
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 6, 16, 8),
                child: Text(
                  'Klangqualität',
                  style: TextStyle(
                    color: colors.textPrimary,
                    fontSize: 15,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              const _AudioProfileOption(
                value: 'opus_96k',
                title: 'Low (Opus 96 kbit/s)',
              ),
              const _AudioProfileOption(
                value: 'opus_128k',
                title: 'Medium (Opus 128 kbit/s)',
              ),
              const _AudioProfileOption(
                value: 'opus_192k',
                title: 'Default (Opus 192 kbit/s)',
              ),
              const _AudioProfileOption(
                value: 'opus_256k',
                title: 'High (Opus 256 kbit/s)',
              ),
              const _AudioProfileOption(value: 'flac', title: 'Studio (Flac)'),
            ],
          ),
        ),
      ),
    );
  }
}

class _AudioProfileOption extends StatelessWidget {
  const _AudioProfileOption({required this.value, required this.title});

  final String value;
  final String title;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return RadioListTile<String>(
      value: value,
      dense: true,
      title: Text(title, style: TextStyle(color: colors.textPrimary)),
      contentPadding: const EdgeInsets.symmetric(horizontal: 8),
    );
  }
}

class _DesktopCurrentTrackInfo extends StatelessWidget {
  const _DesktopCurrentTrackInfo({
    required this.trackTitle,
    required this.trackSubtitle,
    required this.currentTrackArtist,
    required this.trackCoverUrl,
    required this.currentTrack,
    required this.userDataController,
    required this.onOpenArtist,
  });

  final String trackTitle;
  final String trackSubtitle;
  final Artist? currentTrackArtist;
  final String? trackCoverUrl;
  final Track? currentTrack;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;

  Future<void> _showAddToPlaylistMenu(BuildContext context, Track track) {
    final UserData? controller = userDataController;
    if (controller == null) {
      return Future<void>.value();
    }
    return const AppContextMenuController().showContextMenu(
      context: context,
      actions: controller.playlists
          .map(
            (Playlist playlist) => ContextMenuAction(
              label: playlist.name,
              icon: Icons.playlist_play_outlined,
              onSelected: () =>
                  controller.addPlaylistTracks(playlist.itemId, <Track>[track]),
            ),
          )
          .toList(growable: false),
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return SizedBox(
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
                  ? MediaImage(
                      source: trackCoverUrl!,
                      fit: BoxFit.cover,
                      fallback: Icon(
                        Icons.music_note_outlined,
                        color: colors.textSecondary,
                      ),
                    )
                  : Icon(
                      Icons.music_note_outlined,
                      color: colors.textSecondary,
                    ),
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
                  style: TextStyle(
                    color: colors.textPrimary,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                MediaLinkText(
                  items: currentTrackArtist == null
                      ? const <MediaItem>[]
                      : <MediaItem>[currentTrackArtist!],
                  fallbackText: trackSubtitle,
                  onItemTap: onOpenArtist == null
                      ? null
                      : (MediaItem item) => onOpenArtist!(item as Artist),
                  style: TextStyle(color: colors.textSecondary, fontSize: 12),
                ),
              ],
            ),
          ),
          Builder(
            builder: (BuildContext buttonContext) {
              final Track? track = currentTrack;
              final bool isFavorite =
                  track != null &&
                  (userDataController?.isFavorite(track) ?? track.favorite);
              return _PlayerBarIconButton(
                icon: isFavorite
                    ? Icons.favorite_outlined
                    : Icons.favorite_border_outlined,
                onTap: track == null || userDataController == null
                    ? null
                    : () => userDataController!.setFavorite(<Track>[
                        track,
                      ], !isFavorite),
                color: colors.textSecondary,
              );
            },
          ),
          Builder(
            builder: (BuildContext buttonContext) {
              final Track? track = currentTrack;
              final bool hasPlaylists =
                  userDataController?.playlists.isNotEmpty ?? false;
              return _PlayerBarIconButton(
                icon: Icons.add_outlined,
                onTap: track == null || !hasPlaylists
                    ? null
                    : () => _showAddToPlaylistMenu(buttonContext, track),
                color: colors.textSecondary,
              );
            },
          ),
        ],
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
  State<_HoverScaleControlButton> createState() =>
      _HoverScaleControlButtonState();
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
      icon: Icon(
        icon,
        size: 20,
        color: onTap == null ? color.withValues(alpha: 0.45) : color,
      ),
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

/// Mobile UI components
class _MobilePlayerBar extends StatefulWidget {
  final Widget currentTrackInfo;
  final PlayState playbackState;
  final double currentSeconds;
  final double maxSeconds;
  final String trackTransitionKey;
  final Future<void> Function() onTogglePlayPause;
  final Future<void> Function() onPrevious;
  final Future<void> Function() onNext;
  final VoidCallback onOpenPlayerOverlay;
  final void Function(double upwardDelta, double viewportHeight)
  onPlayerOverlayDragUpdate;
  final ValueChanged<double> onPlayerOverlayDragEnd;

  const _MobilePlayerBar({
    required this.currentTrackInfo,
    required this.playbackState,
    this.currentSeconds = 0,
    this.maxSeconds = 0,
    required this.trackTransitionKey,
    required this.onTogglePlayPause,
    required this.onPrevious,
    required this.onNext,
    required this.onOpenPlayerOverlay,
    required this.onPlayerOverlayDragUpdate,
    required this.onPlayerOverlayDragEnd,
  });

  @override
  State<_MobilePlayerBar> createState() => _MobilePlayerBarState();
}

class _MobilePlayerBarState extends State<_MobilePlayerBar> {
  bool _isChangingTrack = false;
  bool _isNextTrack = true;
  bool _hasPendingSwipeTransition = false;
  bool _usesSlideTransition = false;

  @override
  void didUpdateWidget(covariant _MobilePlayerBar oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.trackTransitionKey == widget.trackTransitionKey) {
      return;
    }

    // Only a locally initiated swipe should move the incoming track sideways.
    _usesSlideTransition = _hasPendingSwipeTransition;
    _hasPendingSwipeTransition = false;
  }

  // Serializes swipe commands until the transport command completes.
  Future<void> _changeTrack({
    required bool isNextTrack,
    required Future<void> Function() change,
  }) async {
    if (_isChangingTrack) {
      return;
    }

    setState(() {
      _isChangingTrack = true;
      _isNextTrack = isNextTrack;
      _hasPendingSwipeTransition = true;
    });
    try {
      await change();
    } finally {
      if (mounted) {
        setState(() => _isChangingTrack = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool isPlaying = widget.playbackState == PlayState.playing;
    final bool isBusy =
        widget.playbackState == PlayState.buffering ||
        widget.playbackState == PlayState.seeking;
    final double progress = widget.maxSeconds > 0
        ? (widget.currentSeconds / widget.maxSeconds).clamp(0.0, 1.0)
        : 0.0;

    return GestureDetector(
      onTap: widget.onOpenPlayerOverlay,
      onVerticalDragUpdate: (DragUpdateDetails details) {
        if (details.delta.dy < 0) {
          widget.onPlayerOverlayDragUpdate(
            -details.delta.dy * 0.92,
            MediaQuery.sizeOf(context).height,
          );
        }
      },
      onVerticalDragEnd: (DragEndDetails details) {
        widget.onPlayerOverlayDragEnd(details.primaryVelocity ?? 0);
      },
      behavior: HitTestBehavior.opaque,
      child: Container(
        height: 64,
        margin: const EdgeInsets.fromLTRB(10, 6, 10, 4),
        clipBehavior: Clip.antiAlias,
        decoration: BoxDecoration(
          color: colors.bgSecondary,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: colors.border),
        ),
        child: Stack(
          children: <Widget>[
            Row(
              children: <Widget>[
                Expanded(
                  child: Dismissible(
                    key: const ValueKey<String>('mobile-player-track-swipe'),
                    direction: _isChangingTrack
                        ? DismissDirection.none
                        : DismissDirection.horizontal,
                    resizeDuration: null,
                    movementDuration: const Duration(milliseconds: 220),
                    confirmDismiss: (DismissDirection direction) async {
                      final bool isNextTrack =
                          direction == DismissDirection.endToStart;
                      await _changeTrack(
                        isNextTrack: isNextTrack,
                        change: isNextTrack ? widget.onNext : widget.onPrevious,
                      );
                      return false;
                    },
                    child: Stack(
                      children: <Widget>[
                        Positioned.fill(
                          left: 64,
                          child: Align(
                            alignment: Alignment.centerLeft,
                            child: FractionallySizedBox(
                              widthFactor: progress,
                              heightFactor: 1,
                              child: ColoredBox(color: colors.selection),
                            ),
                          ),
                        ),
                        AnimatedSwitcher(
                          duration: const Duration(milliseconds: 220),
                          // The incoming track travels opposite to the swipe direction.
                          transitionBuilder:
                              (Widget child, Animation<double> animation) {
                                if (!_usesSlideTransition) {
                                  return FadeTransition(
                                    opacity: animation,
                                    child: child,
                                  );
                                }
                                final bool isIncoming =
                                    child.key ==
                                    ValueKey<String>(widget.trackTransitionKey);
                                final double direction = _isNextTrack ? 1 : -1;
                                return SlideTransition(
                                  position: Tween<Offset>(
                                    begin: Offset(
                                      isIncoming ? direction : -direction,
                                      0,
                                    ),
                                    end: Offset.zero,
                                  ).animate(animation),
                                  child: child,
                                );
                              },
                          child: KeyedSubtree(
                            key: ValueKey<String>(widget.trackTransitionKey),
                            child: widget.currentTrackInfo,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
                IconButton(
                  onPressed: isBusy
                      ? null
                      : () {
                          widget.onTogglePlayPause();
                        },
                  icon: isBusy
                      ? SizedBox(
                          width: 18,
                          height: 18,
                          child: CircularProgressIndicator(
                            strokeWidth: 2,
                            valueColor: AlwaysStoppedAnimation<Color>(
                              colors.bgPrimary,
                            ),
                          ),
                        )
                      : Icon(
                          isPlaying
                              ? Icons.pause_sharp
                              : Icons.play_arrow_sharp,
                        ),
                  style: IconButton.styleFrom(
                    shape: const CircleBorder(),
                    backgroundColor: colors.textPrimary,
                    foregroundColor: colors.bgPrimary,
                    disabledBackgroundColor: colors.textPrimary.withValues(
                      alpha: 0.45,
                    ),
                    disabledForegroundColor: colors.bgPrimary,
                  ),
                ),
                const SizedBox(width: 8),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class _MobileCurrentTrackInfo extends StatelessWidget {
  const _MobileCurrentTrackInfo({
    required this.trackTitle,
    required this.trackSubtitle,
    required this.trackCoverUrl,
  });

  final String trackTitle;
  final String trackSubtitle;
  final String? trackCoverUrl;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Row(
      children: <Widget>[
        Container(
          width: 64,
          height: 64,
          color: colors.bgTertiary,
          child: ClipRRect(
            borderRadius: const BorderRadius.horizontal(
              left: Radius.circular(11),
            ),
            child: trackCoverUrl != null
                ? MediaImage(
                    source: trackCoverUrl!,
                    fit: BoxFit.cover,
                    fallback: Icon(
                      Icons.music_note_outlined,
                      color: colors.textSecondary,
                    ),
                  )
                : Icon(Icons.music_note_outlined, color: colors.textSecondary),
          ),
        ),
        const SizedBox(width: 10),
        Expanded(
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Text(
                trackTitle,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: colors.textPrimary, fontSize: 13),
              ),
              Text(
                trackSubtitle,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: colors.textSecondary, fontSize: 11),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

class _MobileBottomNav extends StatelessWidget {
  final AppSection? selectedSection;
  final ValueChanged<AppSection> onSelected;

  const _MobileBottomNav({
    required this.selectedSection,
    required this.onSelected,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Container(
      height: 72,
      padding: const EdgeInsets.fromLTRB(12, 10, 12, 14),
      decoration: BoxDecoration(
        color: colors.bgSecondary,
        border: Border(top: BorderSide(color: colors.border)),
      ),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceAround,
        children: <Widget>[
          _MobileNavButton(
            icon: Icons.home_outlined,
            selectedIcon: Icons.home,
            selected: selectedSection == AppSection.home,
            onTap: () => onSelected(AppSection.home),
          ),
          _MobileNavButton(
            icon: Icons.explore_outlined,
            selectedIcon: Icons.explore,
            selected: selectedSection == AppSection.explore,
            onTap: () => onSelected(AppSection.explore),
          ),
          _MobileNavButton(
            icon: Icons.favorite_border_outlined,
            selectedIcon: Icons.favorite,
            selected: selectedSection == AppSection.favorites,
            onTap: () => onSelected(AppSection.favorites),
          ),
          _MobileNavButton(
            icon: Icons.queue_music_outlined,
            selectedIcon: Icons.queue_music,
            selected: selectedSection == AppSection.playlists,
            onTap: () => onSelected(AppSection.playlists),
          ),
          _MobileNavButton(
            icon: Icons.search_outlined,
            selectedIcon: Icons.search,
            selected: selectedSection == AppSection.search,
            onTap: () => onSelected(AppSection.search),
          ),
          _MobileNavButton(
            icon: Icons.settings_outlined,
            selectedIcon: Icons.settings,
            selected: selectedSection == AppSection.settings,
            onTap: () => onSelected(AppSection.settings),
          ),
        ],
      ),
    );
  }
}

class _MobileNavButton extends StatelessWidget {
  final IconData icon;
  final IconData selectedIcon;
  final bool selected;
  final VoidCallback onTap;

  const _MobileNavButton({
    required this.icon,
    required this.selectedIcon,
    required this.selected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return SizedBox(
      width: 48,
      height: 42,
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          borderRadius: BorderRadius.circular(21),
          hoverColor: Colors.transparent,
          splashColor: Colors.transparent,
          highlightColor: Colors.transparent,
          onTap: onTap,
          child: Center(
            child: AnimatedContainer(
              duration: const Duration(milliseconds: 220),
              curve: Curves.easeOutCubic,
              width: selected ? 56 : 42,
              height: 34,
              decoration: BoxDecoration(
                color: selected ? colors.hover : Colors.transparent,
                borderRadius: BorderRadius.circular(17),
              ),
              alignment: Alignment.center,
              child: AnimatedSwitcher(
                duration: const Duration(milliseconds: 180),
                switchInCurve: Curves.easeOutCubic,
                switchOutCurve: Curves.easeInCubic,
                transitionBuilder: (Widget child, Animation<double> animation) {
                  return FadeTransition(
                    opacity: animation,
                    child: ScaleTransition(scale: animation, child: child),
                  );
                },
                child: Icon(
                  selected ? selectedIcon : icon,
                  key: ValueKey<bool>(selected),
                  color: selected ? colors.accent : colors.textSecondary,
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// App root

class MashinApp extends StatefulWidget {
  const MashinApp({super.key, required this.session});

  final AppSessionController session;

  @override
  State<MashinApp> createState() => _MashinAppState();
}

class _MashinAppState extends State<MashinApp> {
  late ThemeMode _themeMode;
  late final AppSessionController _session;
  late final AppRouter _appRouter;

  @override
  void initState() {
    super.initState();
    _session = widget.session;
    _themeMode = _session.isDarkTheme ? ThemeMode.dark : ThemeMode.light;
    _appRouter = AppRouter(_session);
    _session.addListener(_onSessionChanged);
  }

  @override
  void dispose() {
    _session.removeListener(_onSessionChanged);
    _appRouter.router.dispose();
    _session.dispose();
    super.dispose();
  }

  void _onSessionChanged() {
    setState(() {
      _themeMode = _session.isDarkTheme ? ThemeMode.dark : ThemeMode.light;
    });
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp.router(
      routerConfig: _appRouter.router,
      title: 'Mashin',
      debugShowCheckedModeBanner: false,
      themeMode: _themeMode,
      theme: AppTheme.light(),
      darkTheme: AppTheme.dark(),
      builder: (BuildContext context, Widget? child) {
        return ExcludeSemantics(child: child ?? const SizedBox.shrink());
      },
    );
  }
}
