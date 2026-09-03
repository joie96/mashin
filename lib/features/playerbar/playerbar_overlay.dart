import 'package:flutter/material.dart' hide RepeatMode;

import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_models.dart';
import '../shared/userdata_controller.dart';
import 'playerbar_overlay_mobile.dart';

class PlayerBarOverlay extends StatelessWidget {
  const PlayerBarOverlay({
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
    this.onOpenArtist,
    this.onOpenAlbum,
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
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

  @override
  Widget build(BuildContext context) {
    return PlayerBarOverlayMobile(
      isOpen: isOpen,
      openProgress: openProgress,
      trackTitle: trackTitle,
      trackSubtitle: trackSubtitle,
      trackCoverUrl: trackCoverUrl,
      currentTrack: currentTrack,
      playbackStateType: playbackStateType,
      playbackState: playbackState,
      maxSeconds: maxSeconds,
      currentSeconds: currentSeconds,
      dontStopTheMusicEnabled: dontStopTheMusicEnabled,
      userDataController: userDataController,
      onClose: onClose,
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
