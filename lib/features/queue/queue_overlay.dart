import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
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
    this.desktopBottomInset = 0,
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
  final double desktopBottomInset;
  final VoidCallback onClose;
  final Future<void> Function() onToggleDontStopTheMusic;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

  @override
  State<QueueOverlay> createState() => _QueueOverlayState();
}

class _QueueOverlayState extends State<QueueOverlay> {
  static const double _mobileDismissThreshold = 120;
  static const double _dragResistance = 0.92;
  static const Duration _overlayAnimationDuration = Duration(milliseconds: 260);
  double _dragOffset = 0;

  @override
  void didUpdateWidget(covariant QueueOverlay oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!widget.isOpen && _dragOffset != 0) {
      _dragOffset = 0;
    }
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool interactable = widget.isOpen || _dragOffset > 0;
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
                  duration: _overlayAnimationDuration,
                  curve: Curves.easeOutCubic,
                  opacity: widget.isOpen ? 1 : 0,
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
                    duration: _overlayAnimationDuration,
                    curve: Curves.easeOutCubic,
                    offset: widget.isOpen ? Offset.zero : const Offset(0, 1),
                    child: Transform.translate(
                      offset: Offset(0, _dragOffset),
                      child: GestureDetector(
                        onVerticalDragUpdate: widget.isDesktop
                            ? null
                            : (details) {
                                if (!widget.isOpen) {
                                  return;
                                }

                                final double next =
                                    _dragOffset +
                                    (details.delta.dy * _dragResistance);
                                setState(() {
                                  _dragOffset = next < 0 ? 0 : next;
                                });
                              },
                        onVerticalDragEnd: widget.isDesktop
                            ? null
                            : (_) {
                                if (_dragOffset >= _mobileDismissThreshold) {
                                  setState(() {
                                    _dragOffset = 0;
                                  });
                                  widget.onClose();
                                  return;
                                }

                                setState(() {
                                  _dragOffset = 0;
                                });
                              },
                        child: Center(
                          child: Container(
                            constraints: BoxConstraints(
                              maxWidth: widget.isDesktop
                                  ? 1400
                                  : double.infinity,
                              minHeight: panelHeight,
                              maxHeight: panelHeight,
                            ),
                            decoration: BoxDecoration(
                              color: colors.bgSecondary,
                              borderRadius: widget.isDesktop
                                  ? BorderRadius.zero
                                  : const BorderRadius.vertical(
                                      top: Radius.circular(20),
                                    ),
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
                                      currentQueueIndex:
                                          widget.currentQueueIndex,
                                      playbackCoordinator:
                                          widget.playbackCoordinator,
                                      onClose: widget.onClose,
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
          );
        },
      ),
    );
  }
}
