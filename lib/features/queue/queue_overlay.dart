import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/userdata_controller.dart';
import 'queue_overlay_desktop.dart';
import 'queue_overlay_mobile.dart';

class QueueOverlay extends StatefulWidget {
  const QueueOverlay({
    required this.isOpen,
    required this.isDesktop,
    required this.trackTitle,
    required this.trackSubtitle,
    required this.trackCoverUrl,
    required this.queueTrackCountText,
    required this.queueTotalDurationText,
    required this.dontStopTheMusicEnabled,
    required this.queueItems,
    required this.currentQueueIndex,
    required this.playbackCoordinator,
    this.userDataController,
    this.desktopBottomInset = 0,
    this.mobileOpenProgress = 0,
    required this.onClose,
    required this.onToggleDontStopTheMusic,
    this.onOpenArtist,
    this.onOpenAlbum,
    super.key,
  });

  final bool isOpen;
  final bool isDesktop;
  final String trackTitle;
  final String trackSubtitle;
  final String? trackCoverUrl;
  final String queueTrackCountText;
  final String queueTotalDurationText;
  final bool dontStopTheMusicEnabled;
  final List<QueueItem> queueItems;
  final int? currentQueueIndex;
  final PlaybackCoordinator playbackCoordinator;
  final UserDataController? userDataController;
  final double desktopBottomInset;
  final double mobileOpenProgress;
  final VoidCallback onClose;
  final Future<void> Function() onToggleDontStopTheMusic;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

  /// Public so callers can wait for the close animation before navigating away.
  static const Duration animationDuration = Duration(milliseconds: 260);

  @override
  State<QueueOverlay> createState() => _QueueOverlayState();
}

class _QueueOverlayState extends State<QueueOverlay> {
  static const double _mobileDismissThreshold = 120;
  static const double _dragResistance = 0.92;
  static const Duration _overlayAnimationDuration =
      QueueOverlay.animationDuration;
  double _dragOffset = 0;
  Duration _dragOffsetAnimationDuration = Duration.zero;

  void _handleMobileDismissDragUpdate(double downwardDelta) {
    if (!widget.isOpen || widget.isDesktop || downwardDelta <= 0) {
      return;
    }

    setState(() {
      _dragOffsetAnimationDuration = Duration.zero;
      _dragOffset = math.max(0, _dragOffset + downwardDelta * _dragResistance);
    });
  }

  void _handleMobileDismissDragEnd(double primaryVelocity) {
    if (!widget.isOpen || widget.isDesktop) {
      return;
    }

    if (_dragOffset <= 0) {
      return;
    }

    if (_dragOffset >= _mobileDismissThreshold || primaryVelocity > 700) {
      setState(() {
        _dragOffsetAnimationDuration = Duration.zero;
      });
      widget.onClose();
      return;
    }

    setState(() {
      _dragOffsetAnimationDuration = _overlayAnimationDuration;
      _dragOffset = 0;
    });
  }

  @override
  void didUpdateWidget(covariant QueueOverlay oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!widget.isOpen && _dragOffset != 0) {
      _dragOffsetAnimationDuration = Duration.zero;
    }
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final double effectiveOpenProgress = widget.isOpen
        ? 1
        : widget.mobileOpenProgress.clamp(0, 1).toDouble();
    final bool isInteractivelyOpening =
        !widget.isOpen && effectiveOpenProgress > 0;
    final bool interactable = effectiveOpenProgress > 0 || _dragOffset > 0;
    final double panelBottomInset = widget.isDesktop
        ? widget.desktopBottomInset
        : 0;

    return IgnorePointer(
      ignoring: !interactable,
      child: LayoutBuilder(
        builder: (BuildContext context, BoxConstraints constraints) {
          final double viewportHeight = constraints.maxHeight;
          final double panelHeight = (viewportHeight - panelBottomInset)
              .clamp(0, viewportHeight)
              .toDouble();

          return Stack(
            children: <Widget>[
              Positioned.fill(
                bottom: panelBottomInset,
                child: AnimatedOpacity(
                  duration: isInteractivelyOpening
                      ? Duration.zero
                      : _overlayAnimationDuration,
                  curve: Curves.easeOutCubic,
                  opacity: effectiveOpenProgress,
                  child: GestureDetector(
                    onTap: widget.onClose,
                    child: Container(color: Colors.black54),
                  ),
                ),
              ),
              Positioned(
                left: 0,
                right: 0,
                bottom: panelBottomInset,
                height: panelHeight,
                child: ClipRect(
                  child: AnimatedSlide(
                    duration: isInteractivelyOpening
                        ? Duration.zero
                        : _overlayAnimationDuration,
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
                        panelHeight <= 0 ? 0 : _dragOffset / panelHeight,
                      ),
                      onEnd: () {
                        if (_dragOffsetAnimationDuration != Duration.zero) {
                          setState(() {
                            _dragOffsetAnimationDuration = Duration.zero;
                          });
                        }
                      },
                      child: Center(
                        child: Container(
                          constraints: BoxConstraints(
                            maxWidth: widget.isDesktop ? 1400 : double.infinity,
                            minHeight: panelHeight,
                            maxHeight: panelHeight,
                          ),
                          decoration: BoxDecoration(
                            color: colors.bgSecondary,
                            border: Border(
                              top: BorderSide(color: colors.border),
                            ),
                          ),
                          child: widget.isDesktop
                              ? QueueOverlayDesktop(
                                  trackTitle: widget.trackTitle,
                                  trackSubtitle: widget.trackSubtitle,
                                  trackCoverUrl: widget.trackCoverUrl,
                                  queueTrackCountText:
                                      widget.queueTrackCountText,
                                  queueTotalDurationText:
                                      widget.queueTotalDurationText,
                                  dontStopTheMusicEnabled:
                                      widget.dontStopTheMusicEnabled,
                                  queueItems: widget.queueItems,
                                  currentQueueIndex: widget.currentQueueIndex,
                                  playbackCoordinator:
                                      widget.playbackCoordinator,
                                  userDataController: widget.userDataController,
                                  onClose: widget.onClose,
                                  onToggleDontStopTheMusic:
                                      widget.onToggleDontStopTheMusic,
                                  onOpenArtist: widget.onOpenArtist,
                                  onOpenAlbum: widget.onOpenAlbum,
                                )
                              : SafeArea(
                                  top: true,
                                  bottom: false,
                                  child: QueueOverlayMobile(
                                    queueTrackCountText:
                                        widget.queueTrackCountText,
                                    queueTotalDurationText:
                                        widget.queueTotalDurationText,
                                    queueItems: widget.queueItems,
                                    currentQueueIndex: widget.currentQueueIndex,
                                    playbackCoordinator:
                                        widget.playbackCoordinator,
                                    userDataController:
                                        widget.userDataController,
                                    onOpenArtist: widget.onOpenArtist,
                                    onOpenAlbum: widget.onOpenAlbum,
                                    onClose: widget.onClose,
                                    onDismissDragUpdate:
                                        _handleMobileDismissDragUpdate,
                                    onDismissDragEnd:
                                        _handleMobileDismissDragEnd,
                                  ),
                                ),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ],
          );
        },
      ),
    );
  }
}
