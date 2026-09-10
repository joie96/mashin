import 'dart:async';
import 'dart:convert';
import 'dart:ffi' as ffi;
import 'dart:io';

import 'package:ffi/ffi.dart';

import '../../../app/config/app_config.dart';
import '../../../data/music_assistant/ma_auth_store.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../playback_models.dart';
import 'sendspin_models.dart';

typedef _NativeInitFn = ffi.Int32 Function(ffi.Pointer<Utf8>);
typedef _DartInitFn = int Function(ffi.Pointer<Utf8>);

typedef _NativeVoidFn = ffi.Int32 Function();
typedef _DartVoidFn = int Function();

typedef _NativeSeekFn = ffi.Int32 Function(ffi.Double);
typedef _DartSeekFn = int Function(double);

typedef _NativeVolumeFn = ffi.Int32 Function(ffi.Int32);
typedef _DartVolumeFn = int Function(int);

typedef _NativeMutedFn = ffi.Int32 Function(ffi.Int32);
typedef _DartMutedFn = int Function(int);
typedef _NativeCodecFn = ffi.Int32 Function(ffi.Pointer<Utf8>);
typedef _DartCodecFn = int Function(ffi.Pointer<Utf8>);
typedef _NativeQueueMoveFn = ffi.Int32 Function(ffi.Pointer<Utf8>, ffi.Int32);
typedef _DartQueueMoveFn = int Function(ffi.Pointer<Utf8>, int);
typedef _NativeGetStringFn = ffi.Pointer<Utf8> Function();
typedef _DartGetStringFn = ffi.Pointer<Utf8> Function();

typedef _NativeFreeStringFn = ffi.Void Function(ffi.Pointer<Utf8>);
typedef _DartFreeStringFn = void Function(ffi.Pointer<Utf8>);

typedef _NativeEventCallbackFn = ffi.Void Function(ffi.Pointer<Utf8>);
typedef _NativeRegisterEventCallbackFn =
    ffi.Int32 Function(ffi.Pointer<ffi.NativeFunction<_NativeEventCallbackFn>>);
typedef _DartRegisterEventCallbackFn =
    int Function(ffi.Pointer<ffi.NativeFunction<_NativeEventCallbackFn>>);
typedef _NativeEngineSelectFn =
    ffi.Int32 Function(ffi.Pointer<Utf8>, ffi.Pointer<Utf8>);
typedef _DartEngineSelectFn =
    int Function(ffi.Pointer<Utf8>, ffi.Pointer<Utf8>);

abstract class _SendspinRustBridgeApi {
  Stream<SendspinPlayerState> get playbackStates;
  Stream<Map<String, dynamic>> get events;

  Future<void> engineInit();
  Future<void> engineConnect();
  Future<void> engineReconnect();
  Future<void> engineDisconnect({required String reason});
  Future<void> engineTerminate();
  Future<void> transportPlay();
  Future<void> transportPause();
  Future<void> transportStop();
  Future<void> transportNext();
  Future<void> transportPrevious();
  Future<void> transportSeek(double seconds);
  Future<void> transportSeekRelativeMs(int offsetMs);
  Future<void> transportRepeatMode(RepeatMode mode);
  Future<void> transportShuffle(bool enabled);
  Future<void> transportSwitchGroup();
  Future<void> setVolume(int volume);
  Future<void> setMuted(bool muted);
  Future<void> playMediaJson(String mediaItemsJson);
  Future<void> playMediaNextJson(String mediaItemsJson);
  Future<void> playMediaReplaceNextJson(String mediaItemsJson);
  Future<void> playMediaLastJson(String mediaItemsJson);
  Future<void> clearQueue();
  Future<void> moveQueueItem(String itemId, int shift);
  Future<void> deleteQueueItem(String itemId);
  Future<void> playQueueIndex(int index);
  Future<void> requestCodec(String codec);
  Future<void> dispose();
}

class SendspinRustBridge implements _SendspinRustBridgeApi {
  SendspinRustBridge() : _bindings = _RustBindings(_openLibrary()) {
    _instance = this;
    final logLevel = const String.fromEnvironment(
      'MASHIN_LOG_LEVEL',
      defaultValue: 'info',
    ).toNativeUtf8();
    try {
      _throwIfError(_bindings.setLogLevel(logLevel), 'setLogLevel');
    } finally {
      calloc.free(logLevel);
    }
    _nativeEventCallback = ffi.NativeCallable<_NativeEventCallbackFn>.listener(
      _dispatchNativeEvent,
    );

    final code = _bindings.registerEventCallback(
      _nativeEventCallback.nativeFunction,
    );
    _throwIfError(code, 'registerEventCallback');
  }

  static SendspinRustBridge? _instance;

  final _RustBindings _bindings;
  late final ffi.NativeCallable<_NativeEventCallbackFn> _nativeEventCallback;
  final StreamController<SendspinPlayerState> _playbackStateController =
      StreamController<SendspinPlayerState>.broadcast();
  final StreamController<Map<String, dynamic>> _eventController =
      StreamController<Map<String, dynamic>>.broadcast();
  SendspinPlayerState _playbackState = const SendspinPlayerState();
  String _playbackStateHash = '';
  bool _disposed = false;

  PlaybackOutputMode get outputMode => PlaybackOutputMode.localSendspin;

  @override
  Stream<SendspinPlayerState> get playbackStates =>
      _playbackStateController.stream;

  Stream<SendspinPlayerState> get playerStates => playbackStates;

  Future<void> activate({String? targetPlayerId}) => engineInit();

  Future<void> deactivate() => engineDisconnect(reason: 'deactivate');

  Future<void> connect() => engineConnect();

  Future<void> reconnect() => engineReconnect();

  Future<void> disconnect({String reason = 'client_disconnect'}) =>
      engineDisconnect(reason: reason);

  Future<void> terminate() async {
    try {
      await engineTerminate();
    } finally {
      await dispose();
    }
  }

  @override
  Stream<Map<String, dynamic>> get events => _eventController.stream;

  @override
  Future<void> engineInit() async {
    final resolvedSendspinUrl = await AppConfig.getSendspinUrl();
    final authSession = await AuthStore().read();

    final config = <String, dynamic>{
      'sendspinUrl': resolvedSendspinUrl,
      'musicAssistantUrl':
          authSession?.baseUrl ?? AppConfig.defaultMusicAssistantUrl,
      'musicAssistantAuthToken': authSession?.token,
      'clientId': AppConfig.appId,
      'clientName': AppConfig.appName,
      'bufferCapacityBytes': AppConfig.sendspinBufferCapacityBytes,
      'cacheCapacityBytes': await AppConfig.getSendspinCacheCapacityBytes(),
      'preferredCodec': await AppConfig.getSendspinPreferredCodec(),
    };

    final pointer = jsonEncode(config).toNativeUtf8();
    try {
      final enginePointer = 'sendspin'.toNativeUtf8();
      final code = _bindings.setActiveEngine(enginePointer, pointer);
      calloc.free(enginePointer);
      _throwIfError(code, 'engineInit');
    } finally {
      calloc.free(pointer);
    }
  }

  @override
  Future<void> engineConnect() async {
    final code = _bindings.activate();
    _throwIfError(code, 'engineConnect');

    final volumeCode = _bindings.setVolume(await AppConfig.getLocalVolume());
    _throwIfError(volumeCode, 'restoreVolume');
  }

  @override
  Future<void> engineReconnect() async {
    await engineDisconnect(reason: 'reconnect');
    await engineConnect();
  }

  @override
  Future<void> engineDisconnect({required String reason}) async {
    final code = _bindings.deactivate();
    _throwIfError(code, 'engineDisconnect');
  }

  @override
  Future<void> engineTerminate() async {
    final code = _bindings.deactivate();
    _throwIfError(code, 'engineTerminate');
  }

  @override
  Future<void> transportPlay() async {
    final code = _bindings.transportPlay();
    _throwIfError(code, 'transportPlay');
  }

  @override
  Future<void> transportPause() async {
    final code = _bindings.transportPause();
    _throwIfError(code, 'transportPause');
  }

  @override
  Future<void> transportStop() async {
    final code = _bindings.transportStop();
    _throwIfError(code, 'transportStop');
  }

  @override
  Future<void> transportNext() async {
    final code = _bindings.transportNext();
    _throwIfError(code, 'transportNext');
  }

  @override
  Future<void> transportPrevious() async {
    final code = _bindings.transportPrevious();
    _throwIfError(code, 'transportPrevious');
  }

  @override
  Future<void> transportSeek(double seconds) async {
    final code = _bindings.transportSeek(seconds);
    _throwIfError(code, 'transportSeek');
  }

  @override
  Future<void> transportSeekRelativeMs(int offsetMs) async {
    return;
  }

  @override
  Future<void> transportRepeatMode(RepeatMode mode) async {
    final int value = switch (mode) {
      RepeatMode.off => 0,
      RepeatMode.one => 1,
      RepeatMode.all => 2,
    };

    final code = _bindings.setRepeatMode(value);
    _throwIfError(code, 'transportRepeatMode');
  }

  @override
  Future<void> transportShuffle(bool enabled) async {
    final code = _bindings.setShuffle(enabled ? 1 : 0);
    _throwIfError(code, 'transportShuffle');
  }

  @override
  Future<void> transportSwitchGroup() async {
    return;
  }

  @override
  Future<void> setVolume(int volume) async {
    final clamped = volume.clamp(0, 100);
    await AppConfig.setLocalVolume(clamped);
    final code = _bindings.setVolume(clamped);
    _throwIfError(code, 'setVolume');
  }

  @override
  Future<void> setMuted(bool muted) async {
    final code = _bindings.setMuted(muted ? 1 : 0);
    _throwIfError(code, 'setMuted');
  }

  @override
  Future<void> playMediaJson(String mediaItemsJson) async {
    final pointer = mediaItemsJson.toNativeUtf8();
    try {
      final code = _bindings.playMedia(pointer);
      _throwIfError(code, 'playMedia');
    } finally {
      calloc.free(pointer);
    }
  }

  @override
  Future<void> playMediaNextJson(String mediaItemsJson) =>
      _sendMediaItems(_bindings.playMediaNext, mediaItemsJson, 'playMediaNext');

  @override
  Future<void> playMediaReplaceNextJson(String mediaItemsJson) =>
      _sendMediaItems(
        _bindings.playMediaReplaceNext,
        mediaItemsJson,
        'playMediaReplaceNext',
      );

  @override
  Future<void> playMediaLastJson(String mediaItemsJson) =>
      _sendMediaItems(_bindings.playMediaLast, mediaItemsJson, 'playMediaLast');

  @override
  Future<void> clearQueue() async {
    _throwIfError(_bindings.clearQueue(), 'clearQueue');
  }

  @override
  Future<void> moveQueueItem(String itemId, int shift) async {
    final pointer = itemId.toNativeUtf8();
    try {
      _throwIfError(_bindings.moveQueueItem(pointer, shift), 'moveQueueItem');
    } finally {
      calloc.free(pointer);
    }
  }

  @override
  Future<void> deleteQueueItem(String itemId) async {
    final pointer = itemId.toNativeUtf8();
    try {
      _throwIfError(_bindings.deleteQueueItem(pointer), 'deleteQueueItem');
    } finally {
      calloc.free(pointer);
    }
  }

  @override
  Future<void> playQueueIndex(int index) async {
    _throwIfError(_bindings.playQueueIndex(index), 'playQueueIndex');
  }

  Future<void> _sendMediaItems(
    _DartCodecFn function,
    String mediaItemsJson,
    String operation,
  ) async {
    final pointer = mediaItemsJson.toNativeUtf8();
    try {
      _throwIfError(function(pointer), operation);
    } finally {
      calloc.free(pointer);
    }
  }

  @override
  Future<void> requestCodec(String codec) async {
    final pointer = codec.toNativeUtf8();
    try {
      _throwIfError(_bindings.setPreferredCodec(pointer), 'requestCodec');
    } finally {
      calloc.free(pointer);
    }
  }

  @override
  Future<void> dispose() async {
    if (_disposed) {
      return;
    }

    _disposed = true;
    _instance = null;
    _bindings.registerEventCallback(ffi.nullptr);
    _nativeEventCallback.close();
    await _eventController.close();
    await _playbackStateController.close();
  }

  static void _dispatchNativeEvent(ffi.Pointer<Utf8> pointer) {
    final bridge = _instance;
    if (bridge == null) {
      return;
    }

    bridge._handleNativeEvent(pointer);
  }

  void _handleNativeEvent(ffi.Pointer<Utf8> pointer) {
    if (pointer == ffi.nullptr) {
      return;
    }

    final jsonText = pointer.toDartString();
    _bindings.freeRustString(pointer);

    if (jsonText.trim().isEmpty) {
      return;
    }

    try {
      final dynamic decoded = jsonDecode(jsonText);
      if (decoded is! Map<String, dynamic>) {
        return;
      }

      final statePayload = decoded;

      _eventController.add(statePayload);

      final nextState = _mergeNativeStateEvent(statePayload);
      if (nextState == null) {
        return;
      }
      final nextHash = nextState.toJsonString();
      if (nextHash == _playbackStateHash) {
        return;
      }

      _playbackState = nextState;
      _playbackStateHash = nextHash;
      _playbackStateController.add(nextState);
    } catch (_) {
      return;
    }
  }

  SendspinPlayerState? _mergeNativeStateEvent(Map<String, dynamic> payload) {
    final eventType = payload['type'];
    if (eventType == null) {
      return SendspinPlayerState.fromJson(payload);
    }
    switch (eventType) {
      case 'queue':
        final eventState = SendspinPlayerState.fromJson(payload);
        return _copyState(queue: eventState.queue);
      case 'playback':
        final eventState = SendspinPlayerState.fromJson(payload);
        return _copyState(
          connectionState: eventState.connectionState,
          playbackState: eventState.playbackState,
          volume: eventState.volume,
          muted: eventState.muted,
          repeatMode: eventState.repeatMode,
          shuffleEnabled: eventState.shuffleEnabled,
          positionMillis: eventState.positionMillis,
          currentIndex: (payload['currentIndex'] as num?)?.toInt(),
          clearCurrentIndex: payload['currentIndex'] == null,
        );
      case 'cache':
        final updates = <String, PlaybackQueueItemCache>{};
        for (final item in (payload['items'] as List? ?? const <Object>[])) {
          if (item is! Map<String, dynamic>) {
            continue;
          }
          final queueItemId = (item['queueItemId'] as String?)?.trim();
          final cacheJson = item['cache'];
          if (queueItemId == null || queueItemId.isEmpty ||
              cacheJson is! Map<String, dynamic>) {
            continue;
          }
          updates[queueItemId] = PlaybackQueueItemCache.fromJson(cacheJson);
        }
        if (updates.isEmpty) {
          return null;
        }
        return _copyState(
          queue: _playbackState.queue.map((item) {
            final cache = updates[item.queueItemId];
            if (cache == null) {
              return item;
            }
            return PlaybackQueueItem(
              queueId: item.queueId,
              queueItemId: item.queueItemId,
              name: item.name,
              duration: item.duration,
              sortIndex: item.sortIndex,
              available: item.available,
              mediaItem: item.mediaItem,
              cache: cache,
            );
          }).toList(growable: false),
        );
      default:
        return null;
    }
  }

  SendspinPlayerState _copyState({
    SendspinConnectionState? connectionState,
    PlaybackStateType? playbackState,
    int? volume,
    bool? muted,
    RepeatMode? repeatMode,
    bool? shuffleEnabled,
    int? positionMillis,
    List<PlaybackQueueItem>? queue,
    int? currentIndex,
    bool clearCurrentIndex = false,
  }) {
    return SendspinPlayerState(
      connectionState: connectionState ?? _playbackState.connectionState,
      playbackState: playbackState ?? _playbackState.playbackState,
      volume: volume ?? _playbackState.volume,
      muted: muted ?? _playbackState.muted,
      repeatMode: repeatMode ?? _playbackState.repeatMode,
      shuffleEnabled: shuffleEnabled ?? _playbackState.shuffleEnabled,
      positionMillis: positionMillis ?? _playbackState.positionMillis,
      queue: queue ?? _playbackState.queue,
      currentIndex: clearCurrentIndex
          ? null
          : (currentIndex ?? _playbackState.currentIndex),
    );
  }

  void _throwIfError(int code, String operation) {
    if (code == 0) {
      return;
    }

    final messagePointer = _bindings.getLastError();
    if (messagePointer == ffi.nullptr) {
      throw StateError('Rust bridge operation failed: $operation (code $code)');
    }

    try {
      final message = messagePointer.toDartString();
      throw StateError('Rust bridge operation failed: $operation: $message');
    } finally {
      _bindings.freeRustString(messagePointer);
    }
  }

  static ffi.DynamicLibrary _openLibrary() {
    final fileName = _libraryFileNameForPlatform();
    final candidates = <String>[];

    final envOverride =
        (Platform.environment['MASHIN_PLAYBACK_ENGINE_PATH'] ??
                // Backward-compatible fallback for existing local launch settings.
                Platform.environment['MASHIN_SENDSPIN_BRIDGE_PATH'])
            ?.trim();
    if (envOverride != null && envOverride.isNotEmpty) {
      candidates.add(envOverride);
    }

    candidates.add(fileName);

    final executableDir = File(Platform.resolvedExecutable).parent.path;
    candidates.add(_joinPath(executableDir, fileName));

    final cwd = Directory.current.path;
    candidates.add(_joinPath(cwd, fileName));
    candidates.add(_joinPath(cwd, 'windows', 'native', fileName));
    candidates.add(
      _joinPath(
        cwd,
        'rust',
        'mashin_playback_engine',
        'target',
        'debug',
        fileName,
      ),
    );
    candidates.add(
      _joinPath(
        cwd,
        'rust',
        'mashin_playback_engine',
        'target',
        'release',
        fileName,
      ),
    );

    Object? lastError;
    for (final candidate in candidates.toSet()) {
      try {
        return ffi.DynamicLibrary.open(candidate);
      } catch (error) {
        lastError = error;
      }
    }

    throw StateError(
      'Sendspin Rust bridge library konnte nicht geladen werden. '
      'Gepruefte Pfade: ${candidates.join(' | ')}. '
      'Letzter Fehler: $lastError',
    );
  }

  static String _libraryFileNameForPlatform() {
    if (Platform.isWindows) {
      return 'mashin_playback_engine.dll';
    }

    if (Platform.isMacOS || Platform.isIOS) {
      return 'libmashin_playback_engine.dylib';
    }

    if (Platform.isAndroid || Platform.isLinux) {
      return 'libmashin_playback_engine.so';
    }

    throw UnsupportedError(
      'Sendspin Rust bridge is not supported on this platform.',
    );
  }

  static String _joinPath(
    String a,
    String b, [
    String? c,
    String? d,
    String? e,
    String? f,
  ]) {
    final separator = Platform.pathSeparator;
    final parts = <String>[a, b];
    if (c != null) parts.add(c);
    if (d != null) parts.add(d);
    if (e != null) parts.add(e);
    if (f != null) parts.add(f);
    return parts.join(separator);
  }
}

class _RustBindings {
  _RustBindings(ffi.DynamicLibrary library)
    : setActiveEngine = library
          .lookupFunction<_NativeEngineSelectFn, _DartEngineSelectFn>(
            'mashin_playback_engine_set_active_engine',
          ),
      setLogLevel = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_playback_engine_set_log_level',
      ),
      initialize = library.lookupFunction<_NativeInitFn, _DartInitFn>(
        'mashin_playback_engine_initialize',
      ),
      activate = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_playback_engine_activate',
      ),
      deactivate = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_playback_engine_deactivate',
      ),
      transportPlay = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_playback_engine_transport_play',
      ),
      transportPause = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_playback_engine_transport_pause',
      ),
      transportStop = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_playback_engine_transport_stop',
      ),
      transportNext = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_playback_engine_transport_next',
      ),
      transportPrevious = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_playback_engine_transport_previous',
      ),
      transportSeek = library.lookupFunction<_NativeSeekFn, _DartSeekFn>(
        'mashin_playback_engine_transport_seek',
      ),
      setRepeatMode = library.lookupFunction<_NativeVolumeFn, _DartVolumeFn>(
        'mashin_playback_engine_set_repeat_mode',
      ),
      setShuffle = library.lookupFunction<_NativeMutedFn, _DartMutedFn>(
        'mashin_playback_engine_set_shuffle',
      ),
      setVolume = library.lookupFunction<_NativeVolumeFn, _DartVolumeFn>(
        'mashin_playback_engine_set_volume',
      ),
      setMuted = library.lookupFunction<_NativeMutedFn, _DartMutedFn>(
        'mashin_playback_engine_set_muted',
      ),
      playMedia = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_playback_engine_play_media',
      ),
      playMediaNext = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_playback_engine_play_media_next',
      ),
      playMediaReplaceNext = library
          .lookupFunction<_NativeCodecFn, _DartCodecFn>(
            'mashin_playback_engine_play_media_replace_next',
          ),
      playMediaLast = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_playback_engine_play_media_last',
      ),
      clearQueue = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_playback_engine_clear_queue',
      ),
      moveQueueItem = library
          .lookupFunction<_NativeQueueMoveFn, _DartQueueMoveFn>(
            'mashin_playback_engine_move_queue_item',
          ),
      deleteQueueItem = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_playback_engine_delete_queue_item',
      ),
      playQueueIndex = library.lookupFunction<_NativeVolumeFn, _DartVolumeFn>(
        'mashin_playback_engine_play_queue_index',
      ),
      setPreferredCodec = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_playback_engine_set_preferred_codec',
      ),
      registerEventCallback = library
          .lookupFunction<
            _NativeRegisterEventCallbackFn,
            _DartRegisterEventCallbackFn
          >('mashin_playback_engine_register_event_callback'),
      getLastError = library
          .lookupFunction<_NativeGetStringFn, _DartGetStringFn>(
            'mashin_playback_engine_get_last_error',
          ),
      freeRustString = library
          .lookupFunction<_NativeFreeStringFn, _DartFreeStringFn>(
            'mashin_playback_engine_free_string',
          );

  final _DartEngineSelectFn setActiveEngine;
  final _DartCodecFn setLogLevel;
  final _DartInitFn initialize;
  final _DartVoidFn activate;
  final _DartVoidFn deactivate;
  final _DartVoidFn transportPlay;
  final _DartVoidFn transportPause;
  final _DartVoidFn transportStop;
  final _DartVoidFn transportNext;
  final _DartVoidFn transportPrevious;
  final _DartSeekFn transportSeek;
  final _DartVolumeFn setRepeatMode;
  final _DartMutedFn setShuffle;
  final _DartVolumeFn setVolume;
  final _DartMutedFn setMuted;
  final _DartCodecFn playMedia;
  final _DartCodecFn playMediaNext;
  final _DartCodecFn playMediaReplaceNext;
  final _DartCodecFn playMediaLast;
  final _DartVoidFn clearQueue;
  final _DartQueueMoveFn moveQueueItem;
  final _DartCodecFn deleteQueueItem;
  final _DartVolumeFn playQueueIndex;
  final _DartCodecFn setPreferredCodec;
  final _DartRegisterEventCallbackFn registerEventCallback;
  final _DartGetStringFn getLastError;
  final _DartFreeStringFn freeRustString;
}
