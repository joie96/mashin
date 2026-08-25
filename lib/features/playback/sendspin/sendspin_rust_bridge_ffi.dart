import 'dart:async';
import 'dart:convert';
import 'dart:ffi' as ffi;
import 'dart:io';

import 'package:ffi/ffi.dart';

import '../../../app/config/app_config.dart';
import '../../../data/music_assistant/ma_models.dart';
import 'sendspin_player_models.dart';
import 'sendspin_rust_bridge.dart';

typedef _NativeInitFn = ffi.Int32 Function(ffi.Pointer<Utf8>);
typedef _DartInitFn = int Function(ffi.Pointer<Utf8>);

typedef _NativeVoidFn = ffi.Int32 Function();
typedef _DartVoidFn = int Function();

typedef _NativeDisconnectFn = ffi.Int32 Function(ffi.Pointer<Utf8>);
typedef _DartDisconnectFn = int Function(ffi.Pointer<Utf8>);

typedef _NativeSeekFn = ffi.Int32 Function(ffi.Double);
typedef _DartSeekFn = int Function(double);

typedef _NativeI64Fn = ffi.Int32 Function(ffi.Int64);
typedef _DartI64Fn = int Function(int);

typedef _NativeVolumeFn = ffi.Int32 Function(ffi.Int32);
typedef _DartVolumeFn = int Function(int);

typedef _NativeMutedFn = ffi.Int32 Function(ffi.Int32);
typedef _DartMutedFn = int Function(int);

typedef _NativeGetStringFn = ffi.Pointer<Utf8> Function();
typedef _DartGetStringFn = ffi.Pointer<Utf8> Function();

typedef _NativeFreeStringFn = ffi.Void Function(ffi.Pointer<Utf8>);
typedef _DartFreeStringFn = void Function(ffi.Pointer<Utf8>);

typedef _NativeEventCallbackFn = ffi.Void Function(ffi.Pointer<Utf8>);
typedef _NativeRegisterEventCallbackFn =
    ffi.Int32 Function(ffi.Pointer<ffi.NativeFunction<_NativeEventCallbackFn>>);
typedef _DartRegisterEventCallbackFn =
    int Function(ffi.Pointer<ffi.NativeFunction<_NativeEventCallbackFn>>);

class FfiSendspinRustBridge implements SendspinRustBridge {
  FfiSendspinRustBridge() : _bindings = _RustBindings(_openLibrary()) {
    _instance = this;
    _nativeEventCallback = ffi.NativeCallable<_NativeEventCallbackFn>.listener(
      _dispatchNativeEvent,
    );

    final code = _bindings.registerEventCallback(
      _nativeEventCallback.nativeFunction,
    );
    _throwIfError(code, 'registerEventCallback');
  }

  static FfiSendspinRustBridge? _instance;

  final _RustBindings _bindings;
  late final ffi.NativeCallable<_NativeEventCallbackFn> _nativeEventCallback;
  final StreamController<SendspinPlayerState> _playbackStateController =
      StreamController<SendspinPlayerState>.broadcast();
  final StreamController<Map<String, dynamic>> _eventController =
      StreamController<Map<String, dynamic>>.broadcast();
  SendspinPlayerState _playbackState = const SendspinPlayerState();
  String _playbackStateHash = '';
  bool _disposed = false;

  @override
  Stream<SendspinPlayerState> get playbackStates => _playbackStateController.stream;

  @override
  Stream<Map<String, dynamic>> get events => _eventController.stream;

  @override
  Future<void> engineInit() async {
    final resolvedSendspinUrl = await AppConfig.getSendspinUrl();

    final config = <String, dynamic>{
      'sendspinUrl': resolvedSendspinUrl,
      'clientId': AppConfig.appId,
      'clientName': AppConfig.appName,
      'bufferCapacityBytes': AppConfig.sendspinBufferCapacityBytes,
      'preferredCodec': AppConfig.sendspinPreferredCodec,
      'initialVolume': AppConfig.sendspinInitialVolume,
      'initialMuted': AppConfig.sendspinInitialMuted,
    };

    final pointer = jsonEncode(config).toNativeUtf8();
    try {
      final code = _bindings.engineInit(pointer);
      _throwIfError(code, 'engineInit');
    } finally {
      calloc.free(pointer);
    }
  }

  @override
  Future<void> engineConnect() async {
    final code = _bindings.engineConnect();
    _throwIfError(code, 'engineConnect');
  }

  @override
  Future<void> engineReconnect() async {
    final code = _bindings.engineReconnect();
    _throwIfError(code, 'engineReconnect');
  }

  @override
  Future<void> engineDisconnect({required String reason}) async {
    final pointer = reason.toNativeUtf8();
    try {
      final code = _bindings.engineDisconnect(pointer);
      _throwIfError(code, 'engineDisconnect');
    } finally {
      calloc.free(pointer);
    }
  }

  @override
  Future<void> engineTerminate() async {
    final code = _bindings.engineTerminate();
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
    final code = _bindings.transportSeekRelativeMs(offsetMs);
    _throwIfError(code, 'transportSeekRelativeMs');
  }

  @override
  Future<void> transportRepeatMode(RepeatMode mode) async {
    final int value = switch (mode) {
      RepeatMode.off => 0,
      RepeatMode.one => 1,
      RepeatMode.all => 2,
    };

    final code = _bindings.transportRepeatMode(value);
    _throwIfError(code, 'transportRepeatMode');
  }

  @override
  Future<void> transportShuffle(bool enabled) async {
    final code = _bindings.transportShuffle(enabled ? 1 : 0);
    _throwIfError(code, 'transportShuffle');
  }

  @override
  Future<void> transportSwitchGroup() async {
    final code = _bindings.transportSwitchGroup();
    _throwIfError(code, 'transportSwitchGroup');
  }

  @override
  Future<void> setVolume(int volume) async {
    final clamped = volume.clamp(0, 100);
    final code = _bindings.setVolume(clamped);
    _throwIfError(code, 'setVolume');
  }

  @override
  Future<void> setMuted(bool muted) async {
    final code = _bindings.setMuted(muted ? 1 : 0);
    _throwIfError(code, 'setMuted');
  }

  @override
  Future<void> senderSendSyncState(String state) async {
    final pointer = state.toNativeUtf8();
    try {
      final code = _bindings.senderSendSyncState(pointer);
      _throwIfError(code, 'senderSendSyncState');
    } finally {
      calloc.free(pointer);
    }
  }

  @override
  Future<void> senderEnterExternalSource() async {
    final code = _bindings.senderEnterExternalSource();
    _throwIfError(code, 'senderEnterExternalSource');
  }

  @override
  Future<void> senderExitExternalSource({
    Map<String, dynamic>? playerState,
  }) async {
    ffi.Pointer<Utf8> pointer = ffi.nullptr;
    if (playerState != null) {
      pointer = jsonEncode(playerState).toNativeUtf8();
    }

    try {
      final code = _bindings.senderExitExternalSource(pointer);
      _throwIfError(code, 'senderExitExternalSource');
    } finally {
      if (pointer != ffi.nullptr) {
        calloc.free(pointer);
      }
    }
  }

  @override
  Future<void> senderRequestStreamFormatsJson(String requestJson) async {
    final pointer = requestJson.toNativeUtf8();
    try {
      final code = _bindings.senderRequestStreamFormats(pointer);
      _throwIfError(code, 'senderRequestStreamFormatsJson');
    } finally {
      calloc.free(pointer);
    }
  }

  @override
  Future<void> senderSendMessageJson(String messageJson) async {
    final pointer = messageJson.toNativeUtf8();
    try {
      final code = _bindings.senderSendMessageJson(pointer);
      _throwIfError(code, 'senderSendMessageJson');
    } finally {
      calloc.free(pointer);
    }
  }

  @override
  Future<SendspinPlayerState> getPlaybackState() async {
    await _pullState();
    return _playbackState;
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

      final nextState = SendspinPlayerState.fromJson(statePayload);
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

  Future<void> _pullState() async {
    final pointer = _bindings.getSnapshotJson();
    if (pointer == ffi.nullptr) {
      return;
    }

    try {
      final jsonText = pointer.toDartString();
      if (jsonText.trim().isEmpty) {
        return;
      }

      final dynamic decoded = jsonDecode(jsonText);
      if (decoded is! Map<String, dynamic>) {
        return;
      }

      final nextState = SendspinPlayerState.fromJson(decoded);
      _playbackState = nextState;
      _playbackStateHash = nextState.toJsonString();
      _playbackStateController.add(nextState);
    } finally {
      _bindings.freeRustString(pointer);
    }
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

    final envOverride = Platform.environment['MASHIN_SENDSPIN_BRIDGE_PATH']
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
      _joinPath(cwd, 'rust', 'sendspin_bridge', 'target', 'debug', fileName),
    );
    candidates.add(
      _joinPath(cwd, 'rust', 'sendspin_bridge', 'target', 'release', fileName),
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
      return 'mashin_sendspin_bridge.dll';
    }

    if (Platform.isMacOS || Platform.isIOS) {
      return 'libmashin_sendspin_bridge.dylib';
    }

    if (Platform.isAndroid || Platform.isLinux) {
      return 'libmashin_sendspin_bridge.so';
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
    : engineInit = library.lookupFunction<_NativeInitFn, _DartInitFn>(
        'mashin_sendspin_engine_init',
      ),
      engineConnect = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_sendspin_engine_connect',
      ),
      engineReconnect = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_sendspin_engine_reconnect',
      ),
      engineDisconnect = library
          .lookupFunction<_NativeDisconnectFn, _DartDisconnectFn>(
            'mashin_sendspin_engine_disconnect',
          ),
      engineTerminate = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_sendspin_engine_terminate',
      ),
      transportPlay = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_sendspin_transport_play',
      ),
      transportPause = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_sendspin_transport_pause',
      ),
      transportStop = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_sendspin_transport_stop',
      ),
      transportNext = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_sendspin_transport_next',
      ),
      transportPrevious = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_sendspin_transport_previous',
      ),
      transportSeek = library.lookupFunction<_NativeSeekFn, _DartSeekFn>(
        'mashin_sendspin_transport_seek',
      ),
      transportSeekRelativeMs = library
          .lookupFunction<_NativeI64Fn, _DartI64Fn>(
            'mashin_sendspin_transport_seek_relative_ms',
          ),
      transportRepeatMode = library
          .lookupFunction<_NativeVolumeFn, _DartVolumeFn>(
            'mashin_sendspin_transport_repeat_mode',
          ),
      transportShuffle = library.lookupFunction<_NativeMutedFn, _DartMutedFn>(
        'mashin_sendspin_transport_shuffle',
      ),
      transportSwitchGroup = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_sendspin_transport_switch_group',
      ),
      setVolume = library.lookupFunction<_NativeVolumeFn, _DartVolumeFn>(
        'mashin_sendspin_set_volume',
      ),
      setMuted = library.lookupFunction<_NativeMutedFn, _DartMutedFn>(
        'mashin_sendspin_set_muted',
      ),
      senderSendSyncState = library
          .lookupFunction<_NativeDisconnectFn, _DartDisconnectFn>(
            'mashin_sendspin_sender_send_sync_state',
          ),
      senderEnterExternalSource = library
          .lookupFunction<_NativeVoidFn, _DartVoidFn>(
            'mashin_sendspin_sender_enter_external_source',
          ),
      senderExitExternalSource = library
          .lookupFunction<_NativeDisconnectFn, _DartDisconnectFn>(
            'mashin_sendspin_sender_exit_external_source',
          ),
      senderRequestStreamFormats = library
          .lookupFunction<_NativeDisconnectFn, _DartDisconnectFn>(
            'mashin_sendspin_sender_request_stream_formats',
          ),
      senderSendMessageJson = library
          .lookupFunction<_NativeDisconnectFn, _DartDisconnectFn>(
            'mashin_sendspin_sender_send_message_json',
          ),
      registerEventCallback = library
          .lookupFunction<
            _NativeRegisterEventCallbackFn,
            _DartRegisterEventCallbackFn
          >('mashin_sendspin_register_event_callback'),
      getSnapshotJson = library
          .lookupFunction<_NativeGetStringFn, _DartGetStringFn>(
            'mashin_sendspin_get_snapshot_json',
          ),
      getLastError = library
          .lookupFunction<_NativeGetStringFn, _DartGetStringFn>(
            'mashin_sendspin_get_last_error',
          ),
      freeRustString = library
          .lookupFunction<_NativeFreeStringFn, _DartFreeStringFn>(
            'mashin_sendspin_free_string',
          );

  final _DartInitFn engineInit;
  final _DartVoidFn engineConnect;
  final _DartVoidFn engineReconnect;
  final _DartDisconnectFn engineDisconnect;
  final _DartVoidFn engineTerminate;
  final _DartVoidFn transportPlay;
  final _DartVoidFn transportPause;
  final _DartVoidFn transportStop;
  final _DartVoidFn transportNext;
  final _DartVoidFn transportPrevious;
  final _DartSeekFn transportSeek;
  final _DartI64Fn transportSeekRelativeMs;
  final _DartVolumeFn transportRepeatMode;
  final _DartMutedFn transportShuffle;
  final _DartVoidFn transportSwitchGroup;
  final _DartVolumeFn setVolume;
  final _DartMutedFn setMuted;
  final _DartDisconnectFn senderSendSyncState;
  final _DartVoidFn senderEnterExternalSource;
  final _DartDisconnectFn senderExitExternalSource;
  final _DartDisconnectFn senderRequestStreamFormats;
  final _DartDisconnectFn senderSendMessageJson;
  final _DartRegisterEventCallbackFn registerEventCallback;
  final _DartGetStringFn getSnapshotJson;
  final _DartGetStringFn getLastError;
  final _DartFreeStringFn freeRustString;
}
