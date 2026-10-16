import 'dart:async';

import 'package:flutter/material.dart' hide RepeatMode;

import '../../app/theme/app_theme.dart';
import '../core/core_models.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_image.dart';
import 'playback_seek_slider.dart';

class PlayerBarOverlayMobile extends StatefulWidget {
  const PlayerBarOverlayMobile({
    super.key,
    required this.isOpen,
    required this.openProgress,
    required this.trackTitle,
    required this.trackSubtitle,
    required this.trackCoverUrl,
    required this.currentTrack,
    required this.playbackStateType,
    required this.playbackState,
    required this.playbackQueue,
    required this.maxSeconds,
    required this.currentSeconds,
    required this.dontStopTheMusicEnabled,
    required this.userDataController,
    required this.onClose,
    required this.onOpenQueue,
    required this.onQueueDragUpdate,
    required this.onQueueDragEnd,
    required this.onTogglePlayPause,
    required this.onPrevious,
    required this.onNext,
    required this.onToggleShuffle,
    required this.onToggleRepeatMode,
    required this.onToggleDontStopTheMusic,
    required this.onToggleMute,
    required this.onSeek,
    required this.onOpenAudioOptions,
    this.onOpenArtist,
    this.onOpenAlbum,
  });

  final bool isOpen;
  final double openProgress;
  final String trackTitle;
  final String trackSubtitle;
  final String? trackCoverUrl;
  final Track? currentTrack;
  final PlayState playbackStateType;
  final PlayerState playbackState;
  final Queue playbackQueue;
  final double maxSeconds;
  final double currentSeconds;
  final bool dontStopTheMusicEnabled;
  final UserData? userDataController;
  final VoidCallback onClose;
  final VoidCallback onOpenQueue;
  final void Function(double upwardDelta, double viewportHeight)
  onQueueDragUpdate;
  final ValueChanged<double> onQueueDragEnd;
  final Future<void> Function() onTogglePlayPause;
  final Future<void> Function() onPrevious;
  final Future<void> Function() onNext;
  final Future<void> Function() onToggleShuffle;
  final Future<void> Function() onToggleRepeatMode;
  final Future<void> Function() onToggleDontStopTheMusic;
  final Future<void> Function() onToggleMute;
  final Future<void> Function(double) onSeek;
  final ValueChanged<BuildContext> onOpenAudioOptions;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

  @override
  State<PlayerBarOverlayMobile> createState() => _PlayerBarOverlayMobileState();
}

class _PlayerBarOverlayMobileState extends State<PlayerBarOverlayMobile> {
  static const double _dismissThreshold = 110;
  static const double _dragResistance = 0.92;
  static const Duration _animationDuration = Duration(milliseconds: 260);

  double _dragOffset = 0;
  Duration _dragOffsetAnimationDuration = Duration.zero;
  bool _isDragging = false;

  @override
  void didUpdateWidget(covariant PlayerBarOverlayMobile oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!widget.isOpen && _dragOffset != 0) {
      _dragOffsetAnimationDuration = Duration.zero;
    }
    if (!widget.isOpen && _isDragging) {
      _isDragging = false;
    }
  }

  Future<void> _showAddToPlaylistMenu(BuildContext context, Track track) {
    final UserData? controller = widget.userDataController;
    if (controller == null || controller.playlists.isEmpty) {
      return Future<void>.value();
    }

    return const AppContextMenuController().showMobileDialog(
      context: context,
      title: 'Zu Wiedergabeliste hinzufügen',
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

  void _toggleCurrentTrackFavorite() {
    final Track? track = widget.currentTrack;
    final UserData? controller = widget.userDataController;
    if (track == null || controller == null) {
      return;
    }

    final bool isFavorite = controller.isFavorite(track) || track.favorite;
    controller.setFavorite(<Track>[track], !isFavorite);
  }

  void _handleDragUpdate(DragUpdateDetails details) {
    if (!widget.isOpen) {
      return;
    }

    final double dy = details.delta.dy;
    if (dy < 0) {
      widget.onQueueDragUpdate(
        -dy * _dragResistance,
        MediaQuery.sizeOf(context).height,
      );
    }

    setState(() {
      if (dy > 0) {
        _dragOffsetAnimationDuration = Duration.zero;
        _dragOffset += dy * _dragResistance;
        return;
      }

      if (dy < 0) {
        _dragOffsetAnimationDuration = Duration.zero;
        _dragOffset = 0;
      }
    });
  }

  void _handleDragStart(DragStartDetails details) {
    if (widget.isOpen && !_isDragging) {
      setState(() => _isDragging = true);
    }
  }

  void _handleDragEnd(DragEndDetails details) {
    if (_isDragging) {
      setState(() => _isDragging = false);
    }
    if (!widget.isOpen) {
      return;
    }

    final double velocity = details.primaryVelocity ?? 0;
    widget.onQueueDragEnd(velocity);
    final bool shouldDismiss =
        _dragOffset >= _dismissThreshold || velocity > 700;

    if (shouldDismiss) {
      widget.onClose();
      return;
    }

    setState(() {
      _dragOffsetAnimationDuration = _animationDuration;
      _dragOffset = 0;
    });
  }

  void _handleDragCancel() {
    if (_isDragging) {
      setState(() => _isDragging = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool hasPlaybackSession =
        widget.playbackStateType != PlayState.uninitialized;
    final bool isPlaying = widget.playbackStateType == PlayState.playing;
    final bool isBusy =
        widget.playbackStateType == PlayState.buffering ||
        widget.playbackStateType == PlayState.seeking;
    final bool isShuffleActive = widget.playbackQueue.shuffleEnabled;
    final RepeatMode repeatMode = widget.playbackQueue.repeatMode;
    final bool isRepeatOne = repeatMode == RepeatMode.one;
    final IconData repeatIcon = isRepeatOne
        ? Icons.repeat_one_outlined
        : Icons.repeat_outlined;
    final Color repeatColor = repeatMode == RepeatMode.off
        ? colors.textSecondary
        : colors.accent;
    final Track? track = widget.currentTrack;
    final bool isFavorite =
        track != null &&
        (widget.userDataController?.isFavorite(track) ?? track.favorite);
    final String albumName = track?.album?.name.trim() ?? '';
    final Album? album = track?.album;
    final Artist? artist = track?.artists.firstOrNull;
    final double clampedMaxSeconds = widget.maxSeconds
        .clamp(0, 36000)
        .toDouble();
    final double clampedCurrentSeconds = widget.currentSeconds
        .clamp(0, clampedMaxSeconds > 0 ? clampedMaxSeconds : 1)
        .toDouble();
    final double effectiveOpenProgress = widget.isOpen
        ? 1
        : widget.openProgress.clamp(0, 1).toDouble();
    final bool isInteractivelyOpening =
        !widget.isOpen && effectiveOpenProgress > 0;

    return IgnorePointer(
      ignoring: effectiveOpenProgress == 0,
      child: Stack(
        children: <Widget>[
          Positioned.fill(
            child: AnimatedOpacity(
              duration: isInteractivelyOpening
                  ? Duration.zero
                  : _animationDuration,
              curve: Curves.easeOutCubic,
              opacity: effectiveOpenProgress,
              child: GestureDetector(
                onTap: widget.onClose,
                child: Container(color: Colors.black54),
              ),
            ),
          ),
          Positioned.fill(
            child: ClipRect(
              child: AnimatedSlide(
                duration: isInteractivelyOpening
                    ? Duration.zero
                    : _animationDuration,
                curve: Curves.easeOutCubic,
                offset: Offset(0, 1 - effectiveOpenProgress),
                onEnd: () {
                  if (!widget.isOpen && _dragOffset != 0) {
                    setState(() {
                      _dragOffsetAnimationDuration = Duration.zero;
                      _dragOffset = 0;
                    });
                  }
                },
                child: AnimatedSlide(
                  duration: _dragOffsetAnimationDuration,
                  curve: Curves.easeOutCubic,
                  offset: Offset(
                    0,
                    MediaQuery.sizeOf(context).height <= 0
                        ? 0
                        : _dragOffset / MediaQuery.sizeOf(context).height,
                  ),
                  child: GestureDetector(
                    /// Down closes the expanded player; up promotes the view to the queue.
                    onVerticalDragStart: _handleDragStart,
                    onVerticalDragUpdate: _handleDragUpdate,
                    onVerticalDragEnd: _handleDragEnd,
                    onVerticalDragCancel: _handleDragCancel,
                    child: Container(
                      decoration: BoxDecoration(
                        color: colors.bgSecondary,
                        border: Border(top: BorderSide(color: colors.border)),
                      ),
                      child: SafeArea(
                        top: true,
                        bottom: true,
                        child: Padding(
                          padding: const EdgeInsets.fromLTRB(16, 10, 16, 18),
                          child: Column(
                            children: <Widget>[
                              MobileOverlayDragHandle(isDragging: _isDragging),
                              const SizedBox(height: 16),
                              Expanded(
                                child: Center(
                                  child: OverflowBox(
                                    maxWidth: MediaQuery.sizeOf(context).width,
                                    child: SizedBox(
                                      width: MediaQuery.sizeOf(context).width,
                                      child: _SwipeablePlayerArtwork(
                                        key: ValueKey(
                                          widget.playbackQueue.queueId,
                                        ),
                                        coverUrl: widget.trackCoverUrl,
                                        queue: widget.playbackQueue,
                                        onPrevious: widget.onPrevious,
                                        onNext: widget.onNext,
                                      ),
                                    ),
                                  ),
                                ),
                              ),
                              const SizedBox(height: 18),
                              Text(
                                widget.trackTitle,
                                maxLines: 2,
                                overflow: TextOverflow.ellipsis,
                                textAlign: TextAlign.center,
                                style: TextStyle(
                                  color: colors.textPrimary,
                                  fontFamily: 'PoppinsRegular',
                                  fontSize: 24,
                                  fontWeight: FontWeight.w600,
                                ),
                              ),
                              const SizedBox(height: 6),
                              if (albumName.isNotEmpty) ...<Widget>[
                                _PlayerOverlayLinkText(
                                  text: albumName,
                                  color: colors.textTertiary,
                                  onTap: album == null
                                      ? null
                                      : () => widget.onOpenAlbum?.call(album),
                                ),
                                const SizedBox(height: 6),
                              ],
                              _PlayerOverlayLinkText(
                                text: widget.trackSubtitle,
                                color: colors.textSecondary,
                                onTap: artist == null
                                    ? null
                                    : () => widget.onOpenArtist?.call(artist),
                              ),
                              const SizedBox(height: 30),
                              Center(
                                child: ConstrainedBox(
                                  constraints: const BoxConstraints(
                                    maxWidth: 400,
                                  ),
                                  child: Row(
                                    children: <Widget>[
                                      Text(
                                        _formatSeconds(clampedCurrentSeconds),
                                        style: TextStyle(
                                          color: colors.textTertiary,
                                          fontSize: 12,
                                        ),
                                      ),
                                      const SizedBox(width: 10),
                                      Expanded(
                                        child: SliderTheme(
                                          data: SliderTheme.of(context).copyWith(
                                            trackHeight: 3,
                                            thumbShape:
                                                const RoundSliderThumbShape(
                                                  enabledThumbRadius: 7,
                                                  disabledThumbRadius: 7,
                                                ),
                                            overlayShape:
                                                const RoundSliderOverlayShape(
                                                  overlayRadius: 10,
                                                ),
                                            thumbColor: colors.textSecondary,
                                            activeTrackColor:
                                                colors.textSecondary,
                                            secondaryActiveTrackColor:
                                                colors.textTertiary,
                                            inactiveTrackColor:
                                                colors.bgTertiary,
                                          ),
                                          child: PlaybackSeekSlider(
                                            value: clampedCurrentSeconds,
                                            bufferedValue:
                                                clampedMaxSeconds > 0 &&
                                                    widget
                                                            .playbackState
                                                            .bufferedPositionMillis !=
                                                        null
                                                ? widget
                                                          .playbackState
                                                          .bufferedPositionMillis! /
                                                      1000
                                                : null,
                                            min: 0,
                                            max: clampedMaxSeconds > 0
                                                ? clampedMaxSeconds
                                                : 1,
                                            enabled: hasPlaybackSession,
                                            onSeek: widget.onSeek,
                                          ),
                                        ),
                                      ),
                                      const SizedBox(width: 10),
                                      Text(
                                        _formatSeconds(clampedMaxSeconds),
                                        style: TextStyle(
                                          color: colors.textTertiary,
                                          fontSize: 12,
                                        ),
                                      ),
                                    ],
                                  ),
                                ),
                              ),
                              const SizedBox(height: 22),
                              Row(
                                mainAxisSize: MainAxisSize.min,
                                mainAxisAlignment: MainAxisAlignment.center,
                                children: <Widget>[
                                  _MobileOverlayIconButton(
                                    icon: Icons.shuffle_outlined,
                                    onTap: hasPlaybackSession
                                        ? () => unawaited(
                                            widget.onToggleShuffle(),
                                          )
                                        : null,
                                    color: isShuffleActive
                                        ? colors.accent
                                        : colors.textSecondary,
                                  ),
                                  const Flexible(child: SizedBox(width: 28)),
                                  _MobileOverlayIconButton(
                                    icon: Icons.skip_previous_sharp,
                                    iconSize: 26,
                                    onTap: hasPlaybackSession
                                        ? () => unawaited(widget.onPrevious())
                                        : null,
                                    color: colors.textPrimary,
                                  ),
                                  const Flexible(child: SizedBox(width: 28)),
                                  _MobileOverlayIconButton(
                                    size: 60,
                                    iconSize: 34,
                                    icon: isPlaying
                                        ? Icons.pause_sharp
                                        : Icons.play_arrow_sharp,
                                    onTap: isBusy
                                        ? null
                                        : () => unawaited(
                                            widget.onTogglePlayPause(),
                                          ),
                                    color: colors.bgPrimary,
                                    backgroundColor: colors.textPrimary,
                                    child: isBusy
                                        ? SizedBox(
                                            width: 24,
                                            height: 24,
                                            child: CircularProgressIndicator(
                                              strokeWidth: 2,
                                              valueColor:
                                                  AlwaysStoppedAnimation<Color>(
                                                    colors.bgPrimary,
                                                  ),
                                            ),
                                          )
                                        : null,
                                  ),
                                  const Flexible(child: SizedBox(width: 28)),
                                  _MobileOverlayIconButton(
                                    icon: Icons.skip_next_sharp,
                                    iconSize: 26,
                                    onTap: hasPlaybackSession
                                        ? () => unawaited(widget.onNext())
                                        : null,
                                    color: colors.textPrimary,
                                  ),
                                  const Flexible(child: SizedBox(width: 28)),
                                  _MobileOverlayIconButton(
                                    icon: repeatIcon,
                                    onTap: hasPlaybackSession
                                        ? () => unawaited(
                                            widget.onToggleRepeatMode(),
                                          )
                                        : null,
                                    color: repeatColor,
                                  ),
                                ],
                              ),
                              const SizedBox(height: 34),
                              Wrap(
                                alignment: WrapAlignment.center,
                                spacing: 14,
                                runSpacing: 10,
                                children: <Widget>[
                                  Builder(
                                    builder: (BuildContext menuContext) =>
                                        _MobileOverlayIconButton(
                                          icon: Icons.add_outlined,
                                          onTap:
                                              track == null ||
                                                  widget.userDataController ==
                                                      null ||
                                                  widget
                                                      .userDataController!
                                                      .playlists
                                                      .isEmpty
                                              ? null
                                              : () => unawaited(
                                                  _showAddToPlaylistMenu(
                                                    menuContext,
                                                    track,
                                                  ),
                                                ),
                                          color: colors.textSecondary,
                                        ),
                                  ),
                                  _MobileOverlayIconButton(
                                    icon: isFavorite
                                        ? Icons.favorite_outlined
                                        : Icons.favorite_border_outlined,
                                    onTap:
                                        track == null ||
                                            widget.userDataController == null
                                        ? null
                                        : _toggleCurrentTrackFavorite,
                                    color: isFavorite
                                        ? colors.accent
                                        : colors.textSecondary,
                                  ),
                                  _MobileOverlayIconButton(
                                    icon: Icons.queue_music_outlined,
                                    onTap: widget.onOpenQueue,
                                    color: colors.textSecondary,
                                  ),
                                  _MobileOverlayIconButton(
                                    icon: Icons.cell_tower_outlined,
                                    onTap: hasPlaybackSession
                                        ? () => unawaited(
                                            widget.onToggleDontStopTheMusic(),
                                          )
                                        : null,
                                    color: widget.dontStopTheMusicEnabled
                                        ? colors.accent
                                        : colors.textSecondary,
                                  ),
                                  _MobileOverlayIconButton(
                                    icon: Icons.devices_outlined,
                                    onTap: () {},
                                    color: colors.textSecondary,
                                  ),
                                  Builder(
                                    builder: (BuildContext menuContext) =>
                                        _MobileOverlayIconButton(
                                          icon: Icons.tune_outlined,
                                          onTap: () => widget
                                              .onOpenAudioOptions(menuContext),
                                          color: colors.textSecondary,
                                        ),
                                  ),
                                ],
                              ),
                            ],
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
    );
  }
}

class _PlayerOverlayLinkText extends StatelessWidget {
  const _PlayerOverlayLinkText({
    required this.text,
    required this.color,
    required this.onTap,
  });

  final String text;
  final Color color;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      behavior: HitTestBehavior.opaque,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 2, horizontal: 8),
        child: Text(
          text,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          textAlign: TextAlign.center,
          style: TextStyle(
            color: color,
            fontFamily: 'PoppinsRegular',
            fontSize: 17,
          ),
        ),
      ),
    );
  }
}

class _SwipeablePlayerArtwork extends StatefulWidget {
  const _SwipeablePlayerArtwork({
    super.key,
    required this.coverUrl,
    required this.queue,
    required this.onPrevious,
    required this.onNext,
  });

  final String? coverUrl;
  final Queue queue;
  final Future<void> Function() onPrevious;
  final Future<void> Function() onNext;

  @override
  State<_SwipeablePlayerArtwork> createState() =>
      _SwipeablePlayerArtworkState();
}

class _SwipeablePlayerArtworkState extends State<_SwipeablePlayerArtwork> {
  late final PageController _pageController;
  Timer? _syncTimer;
  late int _requestedIndex;
  bool _isResetting = false;

  int get _currentIndex {
    final int? index = widget.queue.currentIndex;
    return index != null && index >= 0 && index < widget.queue.items.length
        ? index
        : 0;
  }

  @override
  void initState() {
    super.initState();
    _requestedIndex = _currentIndex;
    _pageController = PageController(initialPage: _requestedIndex);
  }

  @override
  void didUpdateWidget(covariant _SwipeablePlayerArtwork oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.queue.currentIndex == _requestedIndex) {
      _syncTimer?.cancel();
      _syncTimer = null;
    } else if (_syncTimer == null &&
        oldWidget.queue.currentIndex != widget.queue.currentIndex) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && _pageController.hasClients) {
          _resetToCurrentIndex();
        }
      });
    }
  }

  @override
  void dispose() {
    _syncTimer?.cancel();
    _pageController.dispose();
    super.dispose();
  }

  void _resetToCurrentIndex() {
    _isResetting = true;
    _requestedIndex = _currentIndex;
    _pageController.jumpToPage(_requestedIndex);
    _isResetting = false;
  }

  void _handlePointerDown(PointerDownEvent event) {
    if (!_pageController.hasClients || widget.queue.items.isEmpty) {
      return;
    }
    final double currentPage =
        _pageController.page ?? _requestedIndex.toDouble();
    final int page = currentPage.round().clamp(
      0,
      widget.queue.items.length - 1,
    );
    if ((currentPage - page).abs() < 0.01) {
      return;
    }

    _isResetting = true;
    _pageController.jumpToPage(page);
    _isResetting = false;
    if (page != _requestedIndex) {
      _requestPage(page);
    }
  }

  void _requestPage(int page) {
    final int previousIndex = _requestedIndex;
    _requestedIndex = page;
    _syncTimer?.cancel();
    _syncTimer = Timer(const Duration(seconds: 2), () {
      if (mounted && _pageController.hasClients) {
        _resetToCurrentIndex();
      }
    });
    for (int index = previousIndex; index < page; index++) {
      unawaited(widget.onNext());
    }
    for (int index = previousIndex; index > page; index--) {
      unawaited(widget.onPrevious());
    }
  }

  bool _onScrollEnd(ScrollEndNotification notification) {
    if (_isResetting || !_pageController.hasClients) {
      return false;
    }
    final int page = _pageController.page?.round() ?? _requestedIndex;
    if (page == _requestedIndex) {
      return false;
    }

    _requestPage(page);
    return false;
  }

  @override
  Widget build(BuildContext context) {
    final List<String?> covers = widget.queue.items.isEmpty
        ? <String?>[widget.coverUrl]
        : <String?>[
            for (int index = 0; index < widget.queue.items.length; index++)
              index == widget.queue.currentIndex
                  ? widget.coverUrl
                  : widget.queue.items[index].mediaItem.imageUriForSize(512),
          ];

    return ClipRect(
      child: NotificationListener<ScrollEndNotification>(
        onNotification: _onScrollEnd,
        child: Listener(
          onPointerDown: _handlePointerDown,
          child: PageView.builder(
            controller: _pageController,
            itemCount: covers.length,
            physics: covers.length == 1
                ? const NeverScrollableScrollPhysics()
                : const PageScrollPhysics(),
            itemBuilder: (BuildContext context, int index) => Padding(
              padding: const EdgeInsets.symmetric(horizontal: 8),
              child: Center(
                child: AspectRatio(
                  aspectRatio: 1,
                  child: _PlayerOverlayArtwork(
                    key: ValueKey(covers[index]),
                    coverUrl: covers[index],
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _PlayerOverlayArtwork extends StatelessWidget {
  const _PlayerOverlayArtwork({super.key, required this.coverUrl});

  final String? coverUrl;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final String? normalizedCoverUrl = coverUrl?.trim();
    final bool hasCover =
        normalizedCoverUrl != null && normalizedCoverUrl.isNotEmpty;

    return RepaintBoundary(
      child: Container(
        decoration: BoxDecoration(
          color: colors.bgTertiary,
          borderRadius: BorderRadius.circular(16),
        ),
        clipBehavior: Clip.antiAlias,
        child: hasCover
            ? MediaImage(
                source: normalizedCoverUrl,
                fit: BoxFit.cover,
                fallback: Icon(
                  Icons.music_note_outlined,
                  color: colors.textSecondary,
                  size: 52,
                ),
              )
            : Icon(
                Icons.music_note_outlined,
                color: colors.textSecondary,
                size: 52,
              ),
      ),
    );
  }
}

class _MobileOverlayIconButton extends StatelessWidget {
  const _MobileOverlayIconButton({
    required this.icon,
    required this.onTap,
    required this.color,
    this.size = 44,
    this.iconSize = 20,
    this.backgroundColor,
    this.child,
  });

  final IconData icon;
  final VoidCallback? onTap;
  final Color color;
  final double size;
  final double iconSize;
  final Color? backgroundColor;
  final Widget? child;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool isDisabled = onTap == null;

    return Material(
      color: Colors.transparent,
      shape: const CircleBorder(),
      child: InkWell(
        customBorder: const CircleBorder(),
        onTap: onTap,
        splashColor: colors.selection,
        child: Opacity(
          opacity: isDisabled ? 0.45 : 1,
          child: Container(
            width: size,
            height: size,
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: backgroundColor ?? Colors.transparent,
              borderRadius: BorderRadius.circular(size / 2),
            ),
            child: child ?? Icon(icon, size: iconSize, color: color),
          ),
        ),
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
