import 'package:flutter/services.dart';

enum AndroidCoreCommand {
  login,
  restorePersistedSession,
  selectLocalOutput,
  selectRemotePlayer,
  activateEngine,
  deactivateEngine,
  shutdown,
  transportPlay,
  transportPause,
  transportStop,
  transportNext,
  transportPrevious,
  transportSeek,
  transportRepeatMode,
  transportShuffle,
  setAutoplay,
  setVolume,
  setMuted,
  saveSessionSettings,
  setPreferredProvider,
  queuePlayMedia,
  startRadio,
  moveQueueItem,
  moveQueueItemToEnd,
  deleteQueueItems,
  selectQueueItem,
  playQueueIndex,
  setAudioProfile,
}

/// Android playback commands are acknowledged by the service, never by FFI.
class AndroidBridge {
  AndroidBridge({MethodChannel? channel})
    : _channel = channel ?? const MethodChannel('mashin_flutter/playback');

  final MethodChannel _channel;

  // Engine detach may already have removed the channel; no playback is stopped.
  Future<void> dispose() async {
    try {
      await _channel.invokeMethod<void>('disconnect');
    } on MissingPluginException {
      return;
    }
  }

  // Service command transport and boundary serialization.
  Future<Object?> execute(
    AndroidCoreCommand command,
    Map<String, Object?> arguments,
  ) => _channel.invokeMethod<Object?>('execute', <String, Object?>{
    'command': command.name,
    'arguments': arguments,
  });
}
