import 'package:flutter/material.dart' hide RepeatMode;

import '../core/core_events.dart';
import '../core/core_models.dart';
import '../core/core_userdata.dart';
import 'playerbar_overlay_mobile.dart';

class PlayerBarOverlay extends StatelessWidget {
  const PlayerBarOverlay({
    super.key,
    required this.coreEvents,
    required this.isOpen,
    required this.openProgress,
    required this.trackTitle,
    required this.trackSubtitle,
    required this.trackCoverUrl,
    required this.currentTrack,
    required this.playbackStateType,
    required this.playbackQueue,
    required this.maxSeconds,
    required this.dontStopTheMusicEnabled,
    required this.userDataController,
    required this.onClose,
    required this.onAnimationEnd,
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
    this.shouldShowMultiDeviceButton = true,
  });

  final MashinCoreBridgeEvents coreEvents;
  final bool isOpen;
  final double openProgress;
  final String trackTitle;
  final String trackSubtitle;
  final String? trackCoverUrl;
  final Track? currentTrack;
  final PlayState playbackStateType;
  final Queue playbackQueue;
  final double maxSeconds;
  final bool dontStopTheMusicEnabled;
  final bool shouldShowMultiDeviceButton;
  final UserData? userDataController;
  final VoidCallback onClose;
  final VoidCallback onAnimationEnd;
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
  Widget build(BuildContext context) {
    return PlayerBarOverlayMobile(
      coreEvents: coreEvents,
      isOpen: isOpen,
      openProgress: openProgress,
      trackTitle: trackTitle,
      trackSubtitle: trackSubtitle,
      trackCoverUrl: trackCoverUrl,
      currentTrack: currentTrack,
      playbackStateType: playbackStateType,
      playbackQueue: playbackQueue,
      maxSeconds: maxSeconds,
      dontStopTheMusicEnabled: dontStopTheMusicEnabled,
      shouldShowMultiDeviceButton: shouldShowMultiDeviceButton,
      userDataController: userDataController,
      onClose: onClose,
      onAnimationEnd: onAnimationEnd,
      onOpenQueue: onOpenQueue,
      onQueueDragUpdate: onQueueDragUpdate,
      onQueueDragEnd: onQueueDragEnd,
      onTogglePlayPause: onTogglePlayPause,
      onPrevious: onPrevious,
      onNext: onNext,
      onToggleShuffle: onToggleShuffle,
      onToggleRepeatMode: onToggleRepeatMode,
      onToggleDontStopTheMusic: onToggleDontStopTheMusic,
      onToggleMute: onToggleMute,
      onSeek: onSeek,
      onOpenAudioOptions: onOpenAudioOptions,
      onOpenArtist: onOpenArtist,
      onOpenAlbum: onOpenAlbum,
    );
  }
}
