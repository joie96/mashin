import 'dart:async';

import 'package:flutter/material.dart';

import '../core/core_events.dart';
import '../core/core_models.dart';

/// Restricts position and buffer updates to the rendered progress controls.
class PlaybackProgressBuilder extends StatelessWidget {
  const PlaybackProgressBuilder({
    super.key,
    required this.coreEvents,
    required this.builder,
  });

  final MashinCoreBridgeEvents coreEvents;
  final Widget Function(BuildContext context, PlayerState playerState) builder;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: coreEvents.playbackProgressChanges,
    builder: (context, child) =>
        builder(context, coreEvents.mediaSessionState.playerState),
  );
}

class PlaybackSeekSlider extends StatefulWidget {
  const PlaybackSeekSlider({
    super.key,
    required this.value,
    required this.max,
    required this.enabled,
    required this.onSeek,
    this.min = 0,
    this.bufferedValue,
  });

  final double value;
  final double min;
  final double max;
  final double? bufferedValue;
  final bool enabled;
  final Future<void> Function(double) onSeek;

  @override
  State<PlaybackSeekSlider> createState() => _PlaybackSeekSliderState();
}

class _PlaybackSeekSliderState extends State<PlaybackSeekSlider> {
  double? _dragValue;

  // Drag previews stay local; playback commands are sent only on release.
  @override
  Widget build(BuildContext context) {
    final double value = (_dragValue ?? widget.value)
        .clamp(widget.min, widget.max)
        .toDouble();
    final double? bufferedValue = widget.bufferedValue
        ?.clamp(widget.min, widget.max)
        .toDouble();

    return Slider(
      value: value,
      min: widget.min,
      max: widget.max,
      secondaryTrackValue:
          widget.enabled && bufferedValue != null && bufferedValue > value
          ? bufferedValue
          : null,
      secondaryActiveColor: Theme.of(
        context,
      ).colorScheme.onSurface.withValues(alpha: 0.18),
      padding: EdgeInsets.zero,
      onChanged: widget.enabled
          ? (double next) => setState(() => _dragValue = next)
          : null,
      onChangeEnd: widget.enabled
          ? (double next) {
              setState(() => _dragValue = null);
              unawaited(widget.onSeek(next));
            }
          : null,
    );
  }
}
