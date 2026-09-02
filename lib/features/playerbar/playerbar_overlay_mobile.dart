import 'dart:async';

import 'package:flutter/material.dart' hide RepeatMode;

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_models.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../shared/userdata_controller.dart';
import '../shared/widgets/media_image.dart';

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
  });

  final bool isOpen;
  final double openProgress;
  final String trackTitle;
  final String trackSubtitle;
  final String? trackCoverUrl;
  final Track? currentTrack;
  final PlaybackStateType playbackStateType;
  final PlaybackState playbackState;
  final double maxSeconds;
  final double currentSeconds;
  final bool dontStopTheMusicEnabled;
  final UserDataController? userDataController;
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
    final UserDataController? controller = widget.userDataController;
    if (controller == null || controller.playlists.isEmpty) {
      return Future<void>.value();
    }

    return const AppContextMenuController().show(
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

  void _toggleCurrentTrackFavorite() {
    final Track? track = widget.currentTrack;
    final UserDataController? controller = widget.userDataController;
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
        widget.playbackStateType != PlaybackStateType.uninitialized;
    final bool isPlaying =
        widget.playbackStateType == PlaybackStateType.playing;
    final bool isBusy =
        widget.playbackStateType == PlaybackStateType.buffering ||
        widget.playbackStateType == PlaybackStateType.seeking;
    final bool isShuffleActive = widget.playbackState.shuffleEnabled == true;
    final RepeatMode repeatMode =
        widget.playbackState.repeatMode ?? RepeatMode.off;
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
                        child: LayoutBuilder(
                          builder: (BuildContext context, BoxConstraints constraints) {
                            final double coverSide = (constraints.maxWidth - 32)
                                .clamp(180, constraints.maxHeight * 0.48)
                                .toDouble();

                            return SingleChildScrollView(
                              physics: const ClampingScrollPhysics(),
                              child: ConstrainedBox(
                                constraints: BoxConstraints(
                                  minHeight: constraints.maxHeight,
                                ),
                                child: Padding(
                                  padding: const EdgeInsets.fromLTRB(
                                    16,
                                    10,
                                    16,
                                    18,
                                  ),
                                  child: Column(
                                    children: <Widget>[
                                      MobileOverlayDragHandle(
                                        isDragging: _isDragging,
                                      ),
                                      const SizedBox(height: 16),
                                      _PlayerOverlayArtwork(
                                        side: coverSide,
                                        coverUrl: widget.trackCoverUrl,
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
                                      const SizedBox(height: 4),
                                      Text(
                                        widget.trackSubtitle,
                                        maxLines: 1,
                                        overflow: TextOverflow.ellipsis,
                                        textAlign: TextAlign.center,
                                        style: TextStyle(
                                          color: colors.textSecondary,
                                          fontFamily: 'PoppinsRegular',
                                          fontSize: 14,
                                        ),
                                      ),
                                      const SizedBox(height: 18),
                                      Row(
                                        children: <Widget>[
                                          Text(
                                            _formatSeconds(
                                              clampedCurrentSeconds,
                                            ),
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
                                                thumbColor:
                                                    colors.textSecondary,
                                                activeTrackColor:
                                                    colors.textSecondary,
                                                inactiveTrackColor:
                                                    colors.bgTertiary,
                                              ),
                                              child:
                                                  _PlayerbarOverlaySeekSlider(
                                                    value:
                                                        clampedCurrentSeconds,
                                                    min: 0,
                                                    max: clampedMaxSeconds > 0
                                                        ? clampedMaxSeconds
                                                        : 1,
                                                    enabled: hasPlaybackSession,
                                                    onSeek: widget.onSeek,
                                                  ),
                                            ),
                                          ),
                                        ],
                                      ),
                                      const SizedBox(height: 22),
                                      Row(
                                        mainAxisAlignment:
                                            MainAxisAlignment.spaceBetween,
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
                                          _MobileOverlayIconButton(
                                            icon: Icons.skip_previous_sharp,
                                            iconSize: 26,
                                            onTap: hasPlaybackSession
                                                ? () => unawaited(
                                                    widget.onPrevious(),
                                                  )
                                                : null,
                                            color: colors.textPrimary,
                                          ),
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
                                                          AlwaysStoppedAnimation<
                                                            Color
                                                          >(colors.bgPrimary),
                                                    ),
                                                  )
                                                : null,
                                          ),
                                          _MobileOverlayIconButton(
                                            icon: Icons.skip_next_sharp,
                                            iconSize: 26,
                                            onTap: hasPlaybackSession
                                                ? () =>
                                                      unawaited(widget.onNext())
                                                : null,
                                            color: colors.textPrimary,
                                          ),
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
                                      const SizedBox(height: 18),
                                      Wrap(
                                        alignment: WrapAlignment.center,
                                        spacing: 14,
                                        runSpacing: 10,
                                        children: <Widget>[
                                          Builder(
                                            builder:
                                                (
                                                  BuildContext menuContext,
                                                ) => _MobileOverlayIconButton(
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
                                                : Icons
                                                      .favorite_border_outlined,
                                            onTap:
                                                track == null ||
                                                    widget.userDataController ==
                                                        null
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
                                                    widget
                                                        .onToggleDontStopTheMusic(),
                                                  )
                                                : null,
                                            color:
                                                widget.dontStopTheMusicEnabled
                                                ? colors.accent
                                                : colors.textSecondary,
                                          ),
                                          _MobileOverlayIconButton(
                                            icon: widget.playbackState.muted
                                                ? Icons.volume_off_outlined
                                                : Icons.volume_up_outlined,
                                            onTap: () => unawaited(
                                              widget.onToggleMute(),
                                            ),
                                            color: widget.playbackState.muted
                                                ? colors.accent
                                                : colors.textSecondary,
                                          ),
                                          Builder(
                                            builder:
                                                (
                                                  BuildContext menuContext,
                                                ) => _MobileOverlayIconButton(
                                                  icon: Icons.tune_outlined,
                                                  onTap: () =>
                                                      widget.onOpenAudioOptions(
                                                        menuContext,
                                                      ),
                                                  color: colors.textSecondary,
                                                ),
                                          ),
                                        ],
                                      ),
                                    ],
                                  ),
                                ),
                              ),
                            );
                          },
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

class _PlayerOverlayArtwork extends StatelessWidget {
  const _PlayerOverlayArtwork({required this.side, required this.coverUrl});

  final double side;
  final String? coverUrl;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final String? normalizedCoverUrl = coverUrl?.trim();
    final bool hasCover =
        normalizedCoverUrl != null && normalizedCoverUrl.isNotEmpty;

    return RepaintBoundary(
      child: Container(
        width: side,
        height: side,
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

class _PlayerbarOverlaySeekSlider extends StatefulWidget {
  const _PlayerbarOverlaySeekSlider({
    required this.value,
    required this.min,
    required this.max,
    required this.enabled,
    required this.onSeek,
  });

  final double value;
  final double min;
  final double max;
  final bool enabled;
  final Future<void> Function(double) onSeek;

  @override
  State<_PlayerbarOverlaySeekSlider> createState() =>
      _PlayerbarOverlaySeekSliderState();
}

class _PlayerbarOverlaySeekSliderState
    extends State<_PlayerbarOverlaySeekSlider> {
  bool _isDragging = false;
  double? _dragValue;

  @override
  void didUpdateWidget(covariant _PlayerbarOverlaySeekSlider oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!_isDragging) {
      _dragValue = null;
    }
  }

  @override
  Widget build(BuildContext context) {
    final double effectiveValue =
        (_isDragging ? (_dragValue ?? widget.value) : widget.value)
            .clamp(widget.min, widget.max)
            .toDouble();

    return Slider(
      value: effectiveValue,
      min: widget.min,
      max: widget.max,
      padding: EdgeInsets.zero,
      onChanged: widget.enabled
          ? (double next) {
              setState(() {
                _isDragging = true;
                _dragValue = next;
              });
            }
          : null,
      onChangeEnd: widget.enabled
          ? (double next) {
              setState(() {
                _isDragging = false;
                _dragValue = null;
              });
              unawaited(widget.onSeek(next));
            }
          : null,
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
