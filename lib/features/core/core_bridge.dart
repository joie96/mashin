import 'dart:async';
import 'dart:convert';
import 'dart:ffi' as ffi;
import 'dart:io';
import 'dart:isolate';

import 'package:ffi/ffi.dart';
import 'package:flutter/foundation.dart';

import '../../app/logging/app_log_buffer.dart';
import '../../app/updates/update_channel.dart';
import '../shared/item_provider_selection.dart';
import 'android_bridge.dart';
import 'core_models.dart';
import 'core_query_scope.dart';

/// Keeps binary image ownership separate from JSON and offloads large metadata.
Future<Map<String, dynamic>?> decodeNativeCoreEvent(
  String text,
  Uint8List? imageBytes, {
  Future<Object?> Function(String)? decodeLarge,
}) async {
  final decoded = text.length >= 64 * 1024
      ? await (decodeLarge?.call(text) ?? compute(jsonDecode, text))
      : jsonDecode(text);
  if (decoded is! Map<String, dynamic>) return null;
  if (imageBytes != null) {
    final field = decoded['type'] == 'query_completed' ? 'result' : 'snapshot';
    (decoded[field] as Map<String, dynamic>)['bytes'] = imageBytes;
  }
  return decoded;
}

class NativeCoreEventDelivery {
  NativeCoreEventDelivery({
    required this.onEvent,
    required this.onError,
    this.onDecoded,
  });

  final void Function(Map<String, dynamic>) onEvent;
  final void Function(Object, StackTrace) onError;
  final void Function(String type, int characters, int queueMs, int decodeMs)?
  onDecoded;
  Future<void> _pending = Future<void>.value();
  Future<_NativeCoreEventDecoder>? _decoder;
  bool _isClosed = false;

  // Closing retires queued metadata before its consumer releases native state.
  void close() {
    _isClosed = true;
    unawaited(
      _decoder?.then<void>(
        (decoder) => decoder.close(),
        onError: (Object error, StackTrace stack) {},
      ),
    );
  }

  // Ordered delivery prevents small terminal events from overtaking isolated parsing.
  Future<void> add(String text, Uint8List? bytes) {
    final queued = Stopwatch()..start();
    return _pending = _pending.then((_) async {
      if (_isClosed) return;
      final queueMs = queued.elapsedMilliseconds;
      final decoding = Stopwatch()..start();
      try {
        final event = await decodeNativeCoreEvent(
          text,
          bytes,
          decodeLarge: _decodeLarge,
        );
        if (!_isClosed && event != null) {
          onDecoded?.call(
            event['type'] as String? ?? 'unknown',
            text.length,
            queueMs,
            decoding.elapsedMilliseconds,
          );
          onEvent(event);
        }
      } catch (error, stack) {
        if (!_isClosed) onError(error, stack);
      }
    });
  }

  Future<Object?> _decodeLarge(String text) async {
    _NativeCoreEventDecoder? decoder;
    try {
      decoder = await (_decoder ??= _NativeCoreEventDecoder.start());
      if (_isClosed) return null;
      return await decoder.decode(text);
    } on FormatException {
      rethrow;
    } catch (_) {
      decoder?.close();
      _decoder = null;
      rethrow;
    }
  }
}

// One worker belongs to one bridge and never retains previously decoded catalogs.
class _NativeCoreEventDecoder {
  _NativeCoreEventDecoder(this.isolate, this.port, this.replies, this.requests);

  final Isolate isolate;
  final ReceivePort port;
  final StreamIterator<Object?> replies;
  final SendPort requests;

  // Decoder lifecycle.
  static Future<_NativeCoreEventDecoder> start() async {
    final port = ReceivePort();
    final replies = StreamIterator<Object?>(port);
    Isolate? isolate;
    try {
      isolate = await Isolate.spawn(
        _run,
        port.sendPort,
        onExit: port.sendPort,
        onError: port.sendPort,
      );
      if (!await replies.moveNext() || replies.current is! SendPort) {
        throw StateError('Native event decoder did not start.');
      }
      return _NativeCoreEventDecoder(
        isolate,
        port,
        replies,
        replies.current as SendPort,
      );
    } catch (_) {
      isolate?.kill(priority: Isolate.immediate);
      port.close();
      await replies.cancel();
      rethrow;
    }
  }

  void close() {
    isolate.kill(priority: Isolate.immediate);
    port.close();
    unawaited(replies.cancel());
  }

  Future<Object?> decode(String text) async {
    requests.send(text);
    if (!await replies.moveNext()) {
      throw StateError('Native event decoder closed.');
    }
    final response = replies.current;
    if (response is! (Object?, String?)) {
      throw StateError('Native event decoder stopped.');
    }
    if (response.$2 != null) throw FormatException(response.$2!);
    return response.$1;
  }

  static void _run(SendPort replies) {
    final requests = ReceivePort();
    replies.send(requests.sendPort);
    requests.listen((request) {
      try {
        replies.send((jsonDecode(request as String), null));
      } catch (error) {
        replies.send((null, error.toString()));
      }
    });
  }
}

/// Resolves the selected source for actions without changing the original item.
Future<Map<String, dynamic>> createSelectedProviderMediaItemPayload(
  MediaItem item, {
  String? providerInstance,
}) async {
  final requestedProvider = providerInstance?.trim();
  final hasProviderOverride =
      requestedProvider != null &&
      requestedProvider.isNotEmpty &&
      requestedProvider != 'library';
  final selection = ItemProviderSelection.forItem(item);
  if (!hasProviderOverride) await selection.initialize();
  final payload = createMusicAssistantMediaItemPayload(item);
  final selected = hasProviderOverride
      ? item.providerMappings
            .where(
              (mapping) =>
                  mapping.available &&
                  mapping.itemId.isNotEmpty &&
                  mapping.providerInstance == requestedProvider,
            )
            .firstOrNull
      : selection.selected;
  if (selected == null) {
    if (hasProviderOverride &&
        (item.provider != requestedProvider ||
            item.providerMappings.any(
              (mapping) => mapping.providerInstance == requestedProvider,
            ))) {
      throw StateError(
        'Media item has no available mapping for $requestedProvider',
      );
    }
    return payload;
  }
  payload['item_id'] = selected.itemId;
  payload['provider'] = selected.providerInstance;
  payload['uri'] =
      '${selected.providerInstance}://${mediaTypeToJson(item.mediaType)}/${selected.itemId}';
  final serializedMappings = <(String, String), Map<String, dynamic>>{
    for (final serialized
        in (payload['provider_mappings'] as List).cast<Map<String, dynamic>>())
      (
        serialized['provider_instance'] as String,
        serialized['item_id'] as String,
      ): serialized,
  };
  final mappings = hasProviderOverride
      ? <ProviderMapping>[
          selected,
          ...item.providerMappings.where((mapping) => mapping != selected),
        ]
      : selection.mappings;
  payload['provider_mappings'] = <Map<String, dynamic>>[
    for (final mapping in mappings)
      serializedMappings[(mapping.providerInstance, mapping.itemId)]!,
  ];
  return payload;
}

/// Expands albums and artists without applying each track's provider preference.
Future<({List<Map<String, dynamic>> mediaItems, int? startItem})>
resolveQueuePlayMedia(
  MashinCoreBridgeApi coreBridge,
  List<MediaItem> mediaItems, {
  String? providerInstance,
  int? startItem,
}) async {
  if (startItem != null && (startItem < 0 || startItem >= mediaItems.length)) {
    throw RangeError.index(startItem, mediaItems, 'startItem');
  }
  final payloads = <Map<String, dynamic>>[];
  int? resolvedStartItem;
  for (final (index, item) in mediaItems.indexed) {
    final offset = payloads.length;
    final payload = await createSelectedProviderMediaItemPayload(
      item,
      providerInstance: providerInstance,
    );
    if (item is! Album && item is! Artist) {
      payloads.add(payload);
      if (index == startItem) resolvedStartItem = offset;
      continue;
    }
    final itemId = payload['item_id'] as String;
    final provider = payload['provider'] as String;
    final tracks = item is Album
        ? await coreBridge.getAlbumTracks(itemId, provider)
        : await coreBridge.getArtistTopTracks(itemId, provider);
    for (final track in tracks) {
      if (!track.canPlay) continue;
      payloads.add(
        await createSelectedProviderMediaItemPayload(
          track,
          providerInstance: provider,
        ),
      );
    }
    if (index == startItem) {
      if (payloads.length == offset) {
        throw StateError('The selected start item has no playable tracks');
      }
      resolvedStartItem = offset;
    }
  }
  return (mediaItems: payloads, startItem: resolvedStartItem);
}

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
typedef _NativeLogFn =
    ffi.Int32 Function(ffi.Pointer<Utf8>, ffi.Pointer<Utf8>, ffi.Pointer<Utf8>);
typedef _DartLogFn =
    int Function(ffi.Pointer<Utf8>, ffi.Pointer<Utf8>, ffi.Pointer<Utf8>);
typedef _NativeQueueMoveFn = ffi.Int32 Function(ffi.Pointer<Utf8>, ffi.Int32);
typedef _DartQueueMoveFn = int Function(ffi.Pointer<Utf8>, int);
typedef _NativeGetStringFn = ffi.Pointer<Utf8> Function();
typedef _DartGetStringFn = ffi.Pointer<Utf8> Function();
typedef _NativeStartMusicAssistantQueryFn =
    ffi.Int32 Function(ffi.Pointer<Utf8>);
typedef _DartStartMusicAssistantQueryFn = int Function(ffi.Pointer<Utf8>);
typedef _NativePlaylistMutationFn =
    ffi.Int32 Function(ffi.Pointer<Utf8>, ffi.Pointer<Utf8>);
typedef _DartPlaylistMutationFn =
    int Function(ffi.Pointer<Utf8>, ffi.Pointer<Utf8>);
typedef _NativeMediaFavoriteFn =
    ffi.Int32 Function(ffi.Pointer<Utf8>, ffi.Int32);
typedef _DartMediaFavoriteFn = int Function(ffi.Pointer<Utf8>, int);

typedef _NativeFreeStringFn = ffi.Void Function(ffi.Pointer<Utf8>);
typedef _DartFreeStringFn = void Function(ffi.Pointer<Utf8>);

typedef _NativeEventCallbackFn =
    ffi.Void Function(ffi.Pointer<Utf8>, ffi.Pointer<ffi.Uint8>, ffi.UintPtr);
typedef _NativeFreeBytesFn =
    ffi.Void Function(ffi.Pointer<ffi.Uint8>, ffi.UintPtr);
typedef _DartFreeBytesFn = void Function(ffi.Pointer<ffi.Uint8>, int);
typedef _NativeRegisterEventCallbackFn =
    ffi.Int32 Function(ffi.Pointer<ffi.NativeFunction<_NativeEventCallbackFn>>);
typedef _DartRegisterEventCallbackFn =
    int Function(ffi.Pointer<ffi.NativeFunction<_NativeEventCallbackFn>>);
typedef _NativeJsonCommandFn = ffi.Int32 Function(ffi.Pointer<Utf8>);
typedef _DartJsonCommandFn = int Function(ffi.Pointer<Utf8>);

// Display delivery never releases the terminal operation's native cancellation owner.
class CatalogQueryDelivery<T> {
  CatalogQueryDelivery(
    this.kind,
    this.arguments,
    this.decoder, {
    this.decodeUpdate,
  });

  final String kind;
  final Map<String, Object?> arguments;
  final T Function(Object?) decoder;
  final T Function(CatalogUpdate)? decodeUpdate;
  final _display = Completer<T>();
  Object? _latest;
  bool _hasUpdate = false;
  Map<String, dynamic>? _latestEvent;
  T? _decodedLatest;
  bool _hasDecodedLatest = false;

  bool get canCompleteFromCatalog =>
      const {
        'providerManifests',
        'recentlyPlayedTracks',
        'searchMedia',
        'artist',
        'album',
        'albumTracks',
        'playlist',
        'playlistTracks',
        'artistAlbums',
        'artistTopTracks',
        'similarArtists',
        'similarTracks',
        'trackVersions',
      }.contains(kind) &&
      arguments['forceRefresh'] != true &&
      arguments['allowDynamicTracks'] != true &&
      (arguments['limit'] == null ||
          (arguments['limit'] is int &&
              (arguments['limit'] as int) >= 1 &&
              (arguments['limit'] as int) <= 100));

  String get _wireKind => switch (kind) {
    'recentlyPlayedTracks' => 'RecentlyPlayed',
    'searchMedia' => 'Search',
    _ => '${kind[0].toUpperCase()}${kind.substring(1)}',
  };

  T decodeLatest(Object? terminal) {
    final isCatalogCompletion =
        terminal is Map && terminal['catalogResultDelivered'] == true;
    if (isCatalogCompletion && (!canCompleteFromCatalog || !_hasUpdate)) {
      throw StateError('Catalog completion has no matching observed value.');
    }
    if (!canCompleteFromCatalog) return decoder(terminal);
    if (!_hasUpdate) return decoder(terminal);
    if (!_hasDecodedLatest) {
      _decodedLatest = decodeUpdate == null
          ? decoder(_latest)
          : decodeUpdate!(_decodeCatalogUpdate(_latestEvent!));
      _hasDecodedLatest = true;
    }
    return _decodedLatest as T;
  }

  // Query identity includes options that can change the returned inventory.
  void onEvent(Map<String, dynamic> event) {
    if (!canCompleteFromCatalog) return;
    final query = event['query'];
    final identity = query is Map
        ? query[_wireKind]
        : query == _wireKind
        ? <String, dynamic>{}
        : null;
    if (identity is! Map ||
        identity['item_id'] != arguments['itemId'] ||
        identity['provider'] != arguments['provider'] ||
        identity['query'] != arguments['query'] ||
        identity['limit'] != arguments['limit'] ||
        identity['allow_dynamic_tracks'] != arguments['allowDynamicTracks'] ||
        identity['allow_lookup'] != arguments['allowLookup'] ||
        (kind == 'searchMedia' &&
            identity['media_type'] != arguments['mediaType'])) {
      return;
    }
    if (event['type'] == 'catalog_updated') {
      _latest = (event['value'] as Map).values.single;
      _latestEvent = event;
      _hasUpdate = true;
      _hasDecodedLatest = false;
    } else if (event['type'] == 'catalog_read_changed' &&
        arguments['allowDynamicTracks'] != true &&
        event['snapshot'] is Map &&
        (event['snapshot'] as Map)['is_server_complete'] == true &&
        _hasUpdate &&
        !_display.isCompleted) {
      try {
        _display.complete(decodeLatest(null));
      } catch (error, stack) {
        _display.completeError(error, stack);
      }
    }
  }

  Future<T> resolve(Future<T> terminal) =>
      Future.any([terminal, _display.future]);
}

class _PendingImageRead {
  final local = Completer<ImageReadSnapshot>();
  late final Future<ImageReadSnapshot> completed;
  void Function()? unregister;
}

final _catalogEventStreams = Expando<_CatalogEventStreams>();
final _decodedCatalogUpdates = Expando<CatalogUpdate>();

CatalogUpdate _decodeCatalogUpdate(Map<String, dynamic> event) =>
    _decodedCatalogUpdates[event] ??= CatalogUpdate.fromJson(event);

class _CatalogEventStreams {
  _CatalogEventStreams(Stream<Map<String, dynamic>> events)
    : updates = _share(events, 'catalog_updated', _decodeCatalogUpdate),
      readChanges = _share(
        events,
        'catalog_read_changed',
        CatalogReadChange.fromJson,
      );

  final Stream<CatalogUpdate> updates;
  final Stream<CatalogReadChange> readChanges;

  // Decode once per active bridge stream; the last consumer releases the raw subscription.
  static Stream<Value> _share<Value>(
    Stream<Map<String, dynamic>> events,
    String type,
    Value Function(Map<String, dynamic>) decode,
  ) {
    StreamSubscription<Value>? subscription;
    late final StreamController<Value> controller;
    controller = StreamController<Value>.broadcast(
      sync: true,
      onListen: () {
        subscription = events
            .where((event) => event['type'] == type)
            .map(decode)
            .listen(
              controller.add,
              onError: controller.addError,
              onDone: controller.close,
            );
      },
      onCancel: () {
        unawaited(subscription?.cancel());
        subscription = null;
      },
    );
    return controller.stream;
  }
}

extension CatalogUpdates on MashinCoreBridgeApi {
  _CatalogEventStreams get _catalogStreams =>
      _catalogEventStreams[this] ??= _CatalogEventStreams(events);

  Stream<void> get mediaReadInvalidations => events
      .where(
        (event) =>
            (event['type'] == 'cache_cleared' &&
                event['cache'] == 'database') ||
            event['type'] == 'provider_instances_changed',
      )
      .map<void>((_) {});

  Stream<CatalogReadChange> get catalogReadChanges =>
      _catalogStreams.readChanges;

  Stream<CatalogUpdate> get catalogUpdates => _catalogStreams.updates;
}

abstract class MashinCoreBridgeApi {
  Stream<Queue> get queueChanges;
  Stream<PlayerState> get playerStateChanges;
  Stream<Map<String, dynamic>> get events;

  Future<void> login({
    required String musicAssistantUrl,
    required String username,
    required String password,
  });
  Future<void> logout();
  Future<bool> restorePersistedSession();
  Future<SessionSettings> getSessionSettings();
  Future<void> selectLocalOutput();
  Future<void> selectRemotePlayer(String playerId);
  Future<void> refreshQueue();
  Future<void> refreshPlayerState();
  Future<RecommendationSnapshot> getRecommendations({
    required RecommendationView view,
    bool refresh = true,
  });
  Future<List<ProviderManifest>> getProviderManifests();
  Future<String?> getProviderIconData(String provider);
  Future<MediaItemImage?> getMediaArtwork(MediaItem item);
  Future<CacheUsage> getCacheUsage();
  Future<void> clearDatabaseCache();
  Future<void> clearImageCache();
  Future<ImageReadSnapshot> readImage(
    ImageRequest request, {
    bool refresh = false,
  });
  Future<List<MediaItem>> getRecentlyPlayedTracks({int limit = 40});
  Future<SearchResults> searchMedia(
    String query,
    MediaType mediaType, {
    int limit = 50,
  });
  Future<Artist> getArtist(String itemId, String provider);
  Future<Album> getAlbum(String itemId, String provider);
  Future<List<Track>> getAlbumTracks(String itemId, String provider);
  Future<Playlist> getPlaylist(String itemId, String provider);
  Future<List<Track>> getPlaylistTracks(
    String itemId,
    String provider, {
    bool forceRefresh = false,
    bool allowDynamicTracks = false,
  });
  Future<List<Album>> getArtistAlbums(
    String itemId,
    String provider, {
    bool isBackground = false,
  });
  Future<List<Track>> getArtistTopTracks(String itemId, String provider);
  Future<List<Artist>> getSimilarArtists(
    String itemId,
    String provider, {
    int limit = 25,
  });
  Future<List<Track>> getSimilarTracks(
    String itemId,
    String provider, {
    int limit = 50,
    bool allowLookup = true,
  });
  Future<List<Track>> getTrackVersions(String itemId, String provider);
  Future<void> activateEngine();
  Future<void> deactivateEngine();
  Future<void> shutdown();
  Future<void> transportPlay();
  Future<void> transportPause();
  Future<void> transportStop();
  Future<void> transportNext();
  Future<void> transportPrevious();
  Future<void> transportSeek(double seconds);
  Future<void> transportRepeatMode(RepeatMode mode);
  Future<void> transportShuffle(bool enabled);
  Future<void> setAutoplay(bool enabled);
  Future<void> setVolume(int volume);
  Future<void> setMuted(bool muted);
  Future<void> queuePlayMedia(
    List<MediaItem> mediaItems, {
    QueueOption option = QueueOption.replace,
    int? startItem,
    bool shuffle = false,
    String? providerInstance,
  });
  Future<void> startRadio(List<MediaItem> mediaItems);
  Future<void> moveQueueItem(String itemId, int positionShift);
  Future<void> moveQueueItemToEnd(String itemId);
  Future<void> deleteQueueItems(List<String> itemIds);
  Future<void> selectQueueItem(String itemId);
  Future<void> playQueueIndex(int index);
  Future<void> setAudioProfile(String audioProfile);
  Future<void> saveSessionSettings(SessionSettingsUpdate settings);
  Future<UpdateChannel> getUpdateChannel({
    required UpdateChannel defaultChannel,
  });
  Future<void> saveUpdateChannel(UpdateChannel channel);
  Future<void> setPreferredProvider(PreferredProvider? preferredProvider);
  void log(LogLevelPreference level, String category, String message);
  Future<void> addPlaylistTracks(String playlistId, List<String> uris);
  Future<void> removePlaylistTracks(
    String playlistId,
    List<int> positionsToRemove,
  );
  Future<void> startSessionDataLoad();
  Future<Map<String, dynamic>> loadUserData();
  Future<Map<String, dynamic>> getUserData();
  Future<void> setUserDataFavorites(List<MediaItem> items, bool favorite);
  Future<void> createPrivatePlaylist(String name);
  Future<void> renamePrivatePlaylist(String playlistId, String name);
  Future<void> refreshPrivatePlaylistCover(String playlistId);
  Future<void> removePrivatePlaylist(String playlistId);
  Future<void> addPrivatePlaylistTracks(String playlistId, List<Track> tracks);
  Future<void> removePrivatePlaylistTracks(
    String playlistId,
    List<String> uris,
  );
  Future<void> dispose();
}

class MashinCoreBridge implements MashinCoreBridgeApi {
  MashinCoreBridge() : _bindings = _RustBindings(_openLibrary()) {
    _instance = this;
    _setRustLogLevel(
      const String.fromEnvironment('MASHIN_LOG_LEVEL', defaultValue: 'info'),
    );
    _nativeEventCallback = ffi.NativeCallable<_NativeEventCallbackFn>.listener(
      _handleNativeEvent,
    );

    final code = _bindings.setEventCallback(
      _nativeEventCallback.nativeFunction,
    );
    _throwIfError(code, 'registerEventCallback');
  }

  static const bool _hasRuntimeLogLevelOverride = bool.hasEnvironment(
    'MASHIN_LOG_LEVEL',
  );
  static const String _runtimeLogLevel = String.fromEnvironment(
    'MASHIN_LOG_LEVEL',
    defaultValue: 'info',
  );

  // Core session restoration can change the Rust logger, so the runtime
  // override is reapplied after every persisted log-level operation.
  void _setRustLogLevel(String logLevel) {
    final pointer = logLevel.toNativeUtf8();
    try {
      _throwIfError(_bindings.setLogLevel(pointer), 'setLogLevel');
    } finally {
      calloc.free(pointer);
    }
  }

  void _reapplyRuntimeLogLevelOverride() {
    if (_hasRuntimeLogLevelOverride) {
      _setRustLogLevel(_runtimeLogLevel);
    }
  }

  // The native callback can dispatch only to the currently active bridge.
  static MashinCoreBridge? _instance;

  /// Non-owning access to the bridge initialized by the application session.
  static MashinCoreBridge get instance =>
      _instance ?? (throw StateError('Mashin core bridge is not initialized.'));

  final _RustBindings _bindings;
  final AndroidBridge? _androidBridge = Platform.isAndroid
      ? AndroidBridge()
      : null;
  late final ffi.NativeCallable<_NativeEventCallbackFn> _nativeEventCallback;
  final StreamController<Queue> _queueController =
      StreamController<Queue>.broadcast();
  final StreamController<PlayerState> _playerStateController =
      StreamController<PlayerState>.broadcast();
  final StreamController<Map<String, dynamic>> _eventController =
      StreamController<Map<String, dynamic>>.broadcast();
  final Map<int, Completer<Object?>> _pendingMusicAssistantQueries =
      <int, Completer<Object?>>{};
  static int _nextMusicAssistantQueryId = 0;
  static int _nextImageObservationId = 0;
  final _imageObservations = <(CoreQueryScope, String), _PendingImageRead>{};
  bool _disposed = false;
  late final _nativeEventDelivery = NativeCoreEventDelivery(
    onEvent: _processNativeEvent,
    onDecoded: (type, characters, queueMs, decodeMs) {
      if (type != 'log' &&
          (characters >= 64 * 1024 || queueMs >= 100 || decodeMs >= 100)) {
        log(
          LogLevelPreference.trace,
          'core.event',
          'type=$type characters=$characters queue_ms=$queueMs decode_ms=$decodeMs',
        );
      }
    },
    onError: (error, stack) => FlutterError.reportError(
      FlutterErrorDetails(exception: error, stack: stack),
    ),
  );

  @override
  Stream<Queue> get queueChanges => _queueController.stream;

  @override
  Stream<PlayerState> get playerStateChanges => _playerStateController.stream;

  @override
  Stream<Map<String, dynamic>> get events => _eventController.stream;

  bool get isServiceOwnedPlayback => _androidBridge != null;

  // Session authentication, restoration, and output configuration.
  @override
  Future<void> login({
    required String musicAssistantUrl,
    required String username,
    required String password,
  }) => _submitOperation<void>('login', <String, Object?>{
    'musicAssistantUrl': musicAssistantUrl.trim(),
    'username': username.trim(),
    'password': password,
  }, (_) {});

  @override
  Future<void> logout() => _submitVoidOperation('logout');

  @override
  Future<bool> restorePersistedSession() async {
    final bool restored = await _submitOperation<bool>(
      'restorePersistedSession',
      const <String, Object?>{},
      (Object? result) => result as bool,
    );
    _reapplyRuntimeLogLevelOverride();
    return restored;
  }

  @override
  Future<SessionSettings> getSessionSettings() =>
      _submitOperation<SessionSettings>(
        'getSessionSettings',
        const <String, Object?>{},
        (Object? result) =>
            SessionSettings.fromJson(result! as Map<String, dynamic>),
      );

  @override
  Future<void> selectLocalOutput() => _submitOperation<void>(
    'selectLocalOutput',
    const <String, Object?>{},
    (_) {},
  );

  @override
  Future<void> selectRemotePlayer(String playerId) => _submitOperation<void>(
    'selectRemotePlayer',
    <String, Object?>{'playerId': playerId},
    (_) {},
  );

  // Playback state reads and named queries.
  @override
  Future<void> refreshQueue() => _submitOperation<void>(
    'refreshQueue',
    const <String, Object?>{},
    (Object? result) =>
        _queueController.add(Queue.fromJson(result! as Map<String, dynamic>)),
  );

  @override
  Future<void> refreshPlayerState() => _submitOperation<void>(
    'refreshPlayerState',
    const <String, Object?>{},
    (Object? result) => _playerStateController.add(
      PlayerState.fromJson(result! as Map<String, dynamic>),
    ),
  );

  @override
  Future<RecommendationSnapshot> getRecommendations({
    required RecommendationView view,
    bool refresh = true,
  }) => _startMusicAssistantQuery<RecommendationSnapshot>(
    'recommendations',
    <String, Object?>{'view': view.name, 'refresh': refresh},
    (Object? decoded) {
      if (decoded is! Map<String, dynamic>) {
        throw const FormatException('recommendations must be a snapshot');
      }
      return RecommendationSnapshot.fromJson(decoded);
    },
  );

  @override
  Future<List<ProviderManifest>> getProviderManifests() =>
      _startMusicAssistantQuery<List<ProviderManifest>>(
        'providerManifests',
        const <String, Object?>{},
        (Object? decoded) {
          if (decoded is! List) {
            throw const FormatException('provider manifests must be a list');
          }
          return decoded
              .whereType<Map<String, dynamic>>()
              .map(ProviderManifest.fromJson)
              .toList(growable: false);
        },
      );

  @override
  Future<String?> getProviderIconData(String provider) =>
      _startMusicAssistantQuery<String?>(
        'providerIconData',
        <String, Object?>{'provider': provider},
        (Object? decoded) {
          if (decoded == null) {
            return null;
          }
          if (decoded is! String) {
            throw const FormatException('provider icon data must be a string');
          }
          final iconData = decoded.trim();
          return iconData.isEmpty ? null : iconData;
        },
      );

  @override
  Future<CacheUsage> getCacheUsage() => _startMusicAssistantQuery<CacheUsage>(
    'getCacheUsage',
    const <String, Object?>{},
    (decoded) => CacheUsage.fromJson(decoded as Map<String, dynamic>),
  );

  @override
  Future<void> clearDatabaseCache() =>
      _submitVoidOperation('clearDatabaseCache');

  @override
  Future<void> clearImageCache() => _submitVoidOperation('clearImageCache');

  @override
  Future<ImageReadSnapshot> readImage(
    ImageRequest request, {
    bool refresh = false,
  }) async {
    final scope = CoreQueryScope.current;
    if (scope == null) {
      return _startMusicAssistantQuery<ImageReadSnapshot>(
        'readImage',
        {'request': request.toJson(), 'refresh': refresh},
        (decoded) =>
            ImageReadSnapshot.fromJson(decoded as Map<String, dynamic>),
      );
    }
    final key = (scope, jsonEncode(request.toJson()));
    var observation = _imageObservations[key];
    if (observation == null) {
      final pending = _PendingImageRead();
      observation = pending;
      _imageObservations[key] = pending;
      final observationId = ++_nextImageObservationId;
      final subscription = events.listen((event) {
        if (event['type'] == 'image_read_updated' &&
            event['observation_id'] == observationId &&
            !pending.local.isCompleted) {
          pending.local.complete(
            ImageReadSnapshot.fromJson(
              event['snapshot'] as Map<String, dynamic>,
            ),
          );
        }
      });
      pending.unregister = scope.registerCancellation(() {
        _imageObservations.remove(key);
        if (!pending.local.isCompleted) {
          pending.local.completeError(const CoreQueryCanceled());
        }
      });
      pending.completed = _submitOperation<ImageReadSnapshot>(
        'observeImage',
        {
          'request': request.toJson(),
          'observationId': observationId,
          'isBackground': request.isBackground,
        },
        (decoded) =>
            ImageReadSnapshot.fromJson(decoded as Map<String, dynamic>),
        queryScope: scope,
      ).whenComplete(subscription.cancel);
      unawaited(
        pending.completed.then<void>(
          (value) {
            if (!pending.local.isCompleted) pending.local.complete(value);
          },
          onError: (Object error, StackTrace stack) {
            if (!pending.local.isCompleted) {
              pending.local.completeError(error, stack);
            }
          },
        ),
      );
    }
    try {
      final snapshot = await (refresh
          ? observation.completed
          : observation.local.future);
      if (refresh || !snapshot.hasRefresh) {
        _imageObservations.remove(key);
        observation.unregister?.call();
      }
      return snapshot;
    } catch (_) {
      _imageObservations.remove(key);
      observation.unregister?.call();
      rethrow;
    }
  }

  @override
  Future<MediaItemImage?> getMediaArtwork(MediaItem item) =>
      _startMusicAssistantQuery<MediaItemImage?>(
        'mediaArtwork',
        <String, Object?>{'item': createMusicAssistantMediaItemPayload(item)},
        (Object? decoded) => decoded == null
            ? null
            : MediaItemImage.fromJson(decoded as Map<String, dynamic>),
      );

  @override
  Future<List<MediaItem>> getRecentlyPlayedTracks({int limit = 40}) =>
      _startMusicAssistantQuery<List<MediaItem>>(
        'recentlyPlayedTracks',
        <String, Object?>{'limit': limit.clamp(1, 100)},
        tracksFromJson,
      );

  @override
  Future<SearchResults> searchMedia(
    String query,
    MediaType mediaType, {
    int limit = 50,
  }) => _startMusicAssistantQuery<SearchResults>(
    'searchMedia',
    <String, Object?>{
      'query': query,
      'mediaType': mediaType.name,
      'limit': limit.clamp(1, 100),
    },
    (Object? decoded) {
      if (decoded is! Map<String, dynamic>) {
        throw const FormatException('search results must be an object');
      }
      return SearchResults.fromJson(decoded);
    },
  );

  @override
  Future<Artist> getArtist(String itemId, String provider) {
    return _startMusicAssistantQuery<Artist>(
      'artist',
      <String, Object?>{'itemId': itemId, 'provider': provider},
      (Object? decoded) {
        if (decoded is! Map<String, dynamic>) {
          throw const FormatException('artist must be an object');
        }
        return Artist.fromJson(decoded);
      },
    );
  }

  @override
  Future<Album> getAlbum(String itemId, String provider) {
    return _startMusicAssistantQuery<Album>(
      'album',
      <String, Object?>{'itemId': itemId, 'provider': provider},
      (Object? decoded) {
        if (decoded is! Map<String, dynamic>) {
          throw const FormatException('album must be an object');
        }
        return Album.fromJson(decoded);
      },
    );
  }

  @override
  Future<List<Track>> getAlbumTracks(String itemId, String provider) {
    return _startMusicAssistantQuery<List<Track>>(
      'albumTracks',
      <String, Object?>{'itemId': itemId, 'provider': provider},
      tracksFromJson,
    );
  }

  @override
  Future<Playlist> getPlaylist(String itemId, String provider) {
    return _startMusicAssistantQuery<Playlist>(
      'playlist',
      <String, Object?>{'itemId': itemId, 'provider': provider},
      (Object? decoded) {
        if (decoded is! Map<String, dynamic>) {
          throw const FormatException('playlist must be an object');
        }
        return Playlist.fromJson(decoded);
      },
    );
  }

  @override
  Future<List<Track>> getPlaylistTracks(
    String itemId,
    String provider, {
    bool forceRefresh = false,
    bool allowDynamicTracks = false,
  }) => _startMusicAssistantQuery<List<Track>>(
    'playlistTracks',
    <String, Object?>{
      'itemId': itemId,
      'provider': provider,
      'forceRefresh': forceRefresh,
      'allowDynamicTracks': allowDynamicTracks,
    },
    tracksFromJson,
  );

  @override
  Future<List<Album>> getArtistAlbums(
    String itemId,
    String provider, {
    bool isBackground = false,
  }) {
    return _startMusicAssistantQuery<List<Album>>(
      'artistAlbums',
      <String, Object?>{
        'itemId': itemId,
        'provider': provider,
        'isBackground': isBackground,
      },
      _decodeAlbums,
    );
  }

  @override
  Future<List<Track>> getArtistTopTracks(String itemId, String provider) {
    return _startMusicAssistantQuery<List<Track>>(
      'artistTopTracks',
      <String, Object?>{'itemId': itemId, 'provider': provider},
      tracksFromJson,
    );
  }

  @override
  Future<List<Artist>> getSimilarArtists(
    String itemId,
    String provider, {
    int limit = 25,
  }) {
    return _startMusicAssistantQuery<List<Artist>>(
      'similarArtists',
      <String, Object?>{
        'itemId': itemId,
        'provider': provider,
        'limit': limit.clamp(1, 100),
      },
      _decodeArtists,
    );
  }

  @override
  Future<List<Track>> getSimilarTracks(
    String itemId,
    String provider, {
    int limit = 50,
    bool allowLookup = true,
  }) {
    return _startMusicAssistantQuery<List<Track>>(
      'similarTracks',
      <String, Object?>{
        'itemId': itemId,
        'provider': provider,
        'limit': limit.clamp(1, 100),
        'allowLookup': allowLookup,
      },
      tracksFromJson,
    );
  }

  @override
  Future<List<Track>> getTrackVersions(String itemId, String provider) {
    return _startMusicAssistantQuery<List<Track>>(
      'trackVersions',
      <String, Object?>{'itemId': itemId, 'provider': provider},
      tracksFromJson,
    );
  }

  Future<T> _startMusicAssistantQuery<T>(
    String kind,
    Map<String, Object?> arguments,
    T Function(Object? decoded) decoder,
  ) async {
    final delivery = CatalogQueryDelivery<T>(
      kind,
      arguments,
      decoder,
      decodeUpdate: (update) => switch (kind) {
        'artist' || 'album' || 'playlist' => update.item as T,
        'artistAlbums' => update.albums as T,
        'similarArtists' => update.artists as T,
        'searchMedia' => update.searchResults as T,
        'providerManifests' => update.providerManifests as T,
        _ => update.tracks as T,
      },
    );
    final subscription = events.listen(delivery.onEvent);
    try {
      return await delivery.resolve(
        _submitOperation<T>(
          kind,
          arguments,
          delivery.decodeLatest,
          queryScope: CoreQueryScope.current ?? CoreQueryScope(),
          isCatalogObserved: delivery.canCompleteFromCatalog,
        ),
      );
    } finally {
      await subscription.cancel();
    }
  }

  Future<T> _submitOperation<T>(
    String kind,
    Map<String, Object?> arguments,
    T Function(Object? result) decoder, {
    CoreQueryScope? queryScope,
    bool isCatalogObserved = false,
  }) async {
    if (_disposed) throw StateError('Mashin core bridge was disposed.');
    if (queryScope?.isCanceled ?? false) throw const CoreQueryCanceled();
    final androidBridge = _androidBridge;
    if (androidBridge != null) {
      final commands = AndroidCoreCommand.values.where(
        (command) => command.name == kind,
      );
      if (commands.isNotEmpty) {
        return decoder(await androidBridge.execute(commands.single, arguments));
      }
    }
    final int requestId = ++_nextMusicAssistantQueryId;
    final Completer<Object?> completer = Completer<Object?>();
    _pendingMusicAssistantQueries[requestId] = completer;
    final timer = Stopwatch()..start();
    log(
      LogLevelPreference.trace,
      'core.operation',
      'started request_id=$requestId kind=$kind pending=${_pendingMusicAssistantQueries.length}',
    );
    final String request = jsonEncode(<String, Object?>{
      'requestId': requestId,
      'kind': kind,
      if (isCatalogObserved) 'isCatalogObserved': true,
      ...arguments,
    });
    final pointer = request.toNativeUtf8();
    try {
      _throwIfError(
        _bindings.startMusicAssistantQuery(pointer),
        'startOperation',
      );
    } catch (error) {
      _pendingMusicAssistantQueries.remove(requestId);
      log(
        LogLevelPreference.debug,
        'core.operation',
        'rejected request_id=$requestId kind=$kind elapsed_ms=${timer.elapsedMilliseconds} error=$error',
      );
      rethrow;
    } finally {
      calloc.free(pointer);
    }
    try {
      final response = queryScope == null
          ? await completer.future
          : await queryScope.execute(
              () => completer.future,
              onCancel: () {
                final pending = _pendingMusicAssistantQueries.remove(requestId);
                if (pending == null) return;
                _cancelNativeQuery(requestId);
                pending.completeError(const CoreQueryCanceled());
              },
            );
      final result = decoder(response);
      log(
        LogLevelPreference.trace,
        'core.operation',
        'completed request_id=$requestId kind=$kind elapsed_ms=${timer.elapsedMilliseconds} pending=${_pendingMusicAssistantQueries.length}',
      );
      return result;
    } catch (error) {
      log(
        error is CoreQueryCanceled
            ? LogLevelPreference.trace
            : LogLevelPreference.debug,
        'core.operation',
        'failed request_id=$requestId kind=$kind elapsed_ms=${timer.elapsedMilliseconds} error=$error',
      );
      rethrow;
    }
  }

  // Cancellation uses the same FFI boundary but has no asynchronous reply.
  void _cancelNativeQuery(int requestId) {
    final pointer = jsonEncode({
      'kind': 'cancelQuery',
      'requestId': requestId,
    }).toNativeUtf8();
    try {
      _throwIfError(_bindings.startMusicAssistantQuery(pointer), 'cancelQuery');
      log(
        LogLevelPreference.trace,
        'core.operation',
        'canceled request_id=$requestId',
      );
    } catch (error) {
      log(
        LogLevelPreference.warn,
        'core.operation',
        'cancel failed request_id=$requestId error=$error',
      );
    } finally {
      calloc.free(pointer);
    }
  }

  List<Artist> _decodeArtists(Object? decoded) {
    if (decoded is! List) {
      throw const FormatException('artists must be a list');
    }
    return decoded
        .whereType<Map<String, dynamic>>()
        .map(Artist.fromJson)
        .toList(growable: false);
  }

  List<Album> _decodeAlbums(Object? decoded) {
    if (decoded is! List) {
      throw const FormatException('albums must be a list');
    }
    return decoded
        .whereType<Map<String, dynamic>>()
        .map(Album.fromJson)
        .toList(growable: false);
  }

  // Engine lifecycle.
  @override
  Future<void> activateEngine() => _submitVoidOperation('activateEngine');

  @override
  Future<void> deactivateEngine() => _submitVoidOperation('deactivateEngine');

  @override
  Future<void> shutdown() => _submitVoidOperation('shutdown');

  // Playback transport commands.
  @override
  Future<void> transportPlay() => _submitVoidOperation('transportPlay');

  @override
  Future<void> transportPause() => _submitVoidOperation('transportPause');

  @override
  Future<void> transportStop() => _submitVoidOperation('transportStop');

  @override
  Future<void> transportNext() => _submitVoidOperation('transportNext');

  @override
  Future<void> transportPrevious() => _submitVoidOperation('transportPrevious');

  @override
  Future<void> transportSeek(double seconds) => _submitVoidOperation(
    'transportSeek',
    <String, Object?>{'seconds': seconds},
  );

  @override
  Future<void> transportRepeatMode(RepeatMode mode) {
    final int value = switch (mode) {
      RepeatMode.off => 0,
      RepeatMode.one => 1,
      RepeatMode.all => 2,
    };

    return _submitVoidOperation('transportRepeatMode', <String, Object?>{
      'mode': value,
    });
  }

  @override
  Future<void> transportShuffle(bool enabled) => _submitVoidOperation(
    'transportShuffle',
    <String, Object?>{'enabled': enabled},
  );

  // Playback preferences.
  @override
  Future<void> setAutoplay(bool enabled) => _submitVoidOperation(
    'setAutoplay',
    <String, Object?>{'enabled': enabled},
  );

  @override
  Future<void> setVolume(int volume) {
    final clamped = volume.clamp(0, 100);
    return _submitVoidOperation('setVolume', <String, Object?>{
      'volume': clamped,
    });
  }

  @override
  Future<void> setMuted(bool muted) =>
      _submitVoidOperation('setMuted', <String, Object?>{'muted': muted});

  // Queue commands expand selected sources before mutating the canonical queue.
  @override
  Future<void> queuePlayMedia(
    List<MediaItem> mediaItems, {
    QueueOption option = QueueOption.replace,
    int? startItem,
    bool shuffle = false,
    String? providerInstance,
  }) async {
    final resolved = await resolveQueuePlayMedia(
      this,
      mediaItems,
      providerInstance: providerInstance,
      startItem: startItem,
    );
    if (mediaItems.isNotEmpty && resolved.mediaItems.isEmpty) return;
    await _submitVoidOperation('queuePlayMedia', <String, Object?>{
      'request': <String, Object?>{
        'mediaItems': resolved.mediaItems,
        'option': option.name.replaceAllMapped(
          RegExp('[A-Z]'),
          (match) => '_${match.group(0)!.toLowerCase()}',
        ),
        'startItem': resolved.startItem,
        'shuffle': shuffle,
      },
    });
  }

  @override
  Future<void> startRadio(List<MediaItem> mediaItems) =>
      _submitMediaOperation('startRadio', mediaItems);

  Future<void> _submitMediaOperation(
    String kind,
    List<MediaItem> mediaItems,
  ) async => _submitVoidOperation(kind, <String, Object?>{
    'mediaItems': await Future.wait(
      mediaItems.map(createSelectedProviderMediaItemPayload),
    ),
  });
  // Queue item mutations.

  @override
  Future<void> moveQueueItem(String itemId, int positionShift) =>
      _submitVoidOperation('moveQueueItem', <String, Object?>{
        'itemId': itemId,
        'positionShift': positionShift,
      });

  @override
  Future<void> moveQueueItemToEnd(String itemId) => _submitVoidOperation(
    'moveQueueItemToEnd',
    <String, Object?>{'itemId': itemId},
  );

  @override
  Future<void> deleteQueueItems(List<String> itemIds) => _submitVoidOperation(
    'deleteQueueItems',
    <String, Object?>{'itemIds': itemIds},
  );

  @override
  Future<void> selectQueueItem(String itemId) => _submitVoidOperation(
    'selectQueueItem',
    <String, Object?>{'itemId': itemId},
  );

  @override
  Future<void> playQueueIndex(int index) =>
      _submitVoidOperation('playQueueIndex', <String, Object?>{'index': index});

  // Mashin profile selection.
  @override
  Future<void> setAudioProfile(String audioProfile) => _submitVoidOperation(
    'setAudioProfile',
    <String, Object?>{'audioProfile': audioProfile},
  );

  @override
  Future<void> saveSessionSettings(SessionSettingsUpdate settings) async {
    await _submitVoidOperation('saveSessionSettings', settings.toJson());
    _reapplyRuntimeLogLevelOverride();
  }

  // App-wide settings persist in Rust independently of authentication.
  @override
  Future<UpdateChannel> getUpdateChannel({
    required UpdateChannel defaultChannel,
  }) => _submitOperation<UpdateChannel>('getUpdateChannel', <String, Object?>{
    'defaultChannel': defaultChannel.name,
  }, (Object? result) => UpdateChannel.values.byName(result as String));

  @override
  Future<void> saveUpdateChannel(UpdateChannel channel) => _submitVoidOperation(
    'saveUpdateChannel',
    <String, Object?>{'channel': channel.name},
  );

  @override
  Future<void> setPreferredProvider(PreferredProvider? preferredProvider) =>
      _submitVoidOperation('setPreferredProvider', <String, Object?>{
        'preferredProvider': preferredProvider?.toJson(),
      });

  @override
  void log(LogLevelPreference level, String category, String message) {
    final levelPointer = level.name.toNativeUtf8();
    final categoryPointer = category.toNativeUtf8();
    final messagePointer = message.toNativeUtf8();
    try {
      _bindings.log(levelPointer, categoryPointer, messagePointer);
    } finally {
      calloc
        ..free(levelPointer)
        ..free(categoryPointer)
        ..free(messagePointer);
    }
  }

  // Playlist mutations.
  @override
  Future<void> addPlaylistTracks(String playlistId, List<String> uris) =>
      _submitVoidOperation('addPlaylistTracks', <String, Object?>{
        'playlistId': playlistId,
        'uris': uris,
      });

  @override
  Future<void> removePlaylistTracks(
    String playlistId,
    List<int> positionsToRemove,
  ) => _submitVoidOperation('removePlaylistTracks', <String, Object?>{
    'playlistId': playlistId,
    'positions': positionsToRemove,
  });

  @override
  Future<void> startSessionDataLoad() =>
      _submitVoidOperation('startSessionDataLoad', const <String, Object?>{});

  @override
  Future<Map<String, dynamic>> loadUserData() =>
      _submitOperation<Map<String, dynamic>>(
        'loadUserData',
        const <String, Object?>{},
        (Object? result) => result! as Map<String, dynamic>,
      );

  @override
  Future<Map<String, dynamic>> getUserData() =>
      _submitOperation<Map<String, dynamic>>(
        'getUserData',
        const <String, Object?>{},
        (Object? result) => result! as Map<String, dynamic>,
      );

  @override
  Future<void> setUserDataFavorites(
    List<MediaItem> items,
    bool favorite,
  ) async => _submitVoidOperation('setUserDataFavorites', <String, Object?>{
    'mediaItems': await Future.wait(
      items.map(createSelectedProviderMediaItemPayload),
    ),
    'favorite': favorite,
  });

  @override
  Future<void> createPrivatePlaylist(String name) => _submitVoidOperation(
    'createPrivatePlaylist',
    <String, Object?>{'name': name},
  );

  @override
  Future<void> renamePrivatePlaylist(String playlistId, String name) =>
      _submitVoidOperation('renamePrivatePlaylist', <String, Object?>{
        'playlistId': playlistId,
        'name': name,
      });

  @override
  Future<void> refreshPrivatePlaylistCover(String playlistId) =>
      _submitVoidOperation('refreshPrivatePlaylistCover', <String, Object?>{
        'playlistId': playlistId,
      });

  @override
  Future<void> removePrivatePlaylist(String playlistId) => _submitVoidOperation(
    'removePrivatePlaylist',
    <String, Object?>{'playlistId': playlistId},
  );

  @override
  Future<void> addPrivatePlaylistTracks(
    String playlistId,
    List<Track> tracks,
  ) async => _submitVoidOperation('addPrivatePlaylistTracks', <String, Object?>{
    'playlistId': playlistId,
    'tracks': await Future.wait(
      tracks.map(createSelectedProviderMediaItemPayload),
    ),
  });

  @override
  Future<void> removePrivatePlaylistTracks(
    String playlistId,
    List<String> uris,
  ) => _submitVoidOperation('removePrivatePlaylistTracks', <String, Object?>{
    'playlistId': playlistId,
    'uris': uris,
  });

  Future<void> _submitVoidOperation(
    String kind, [
    Map<String, Object?> arguments = const <String, Object?>{},
  ]) => _submitOperation<void>(kind, arguments, (_) {});

  // Bridge lifecycle.
  @override
  Future<void> dispose() async {
    _nativeEventDelivery.close();
    if (_disposed) {
      return;
    }

    _disposed = true;
    try {
      await _androidBridge?.dispose();
    } finally {
      if (identical(_instance, this)) _instance = null;
      for (final entry in _pendingMusicAssistantQueries.entries.toList()) {
        _cancelNativeQuery(entry.key);
        entry.value.completeError(
          StateError('Mashin core bridge was disposed.'),
        );
      }
      _pendingMusicAssistantQueries.clear();
      for (final observation in _imageObservations.values) {
        observation.unregister?.call();
      }
      _imageObservations.clear();
      _bindings.setEventCallback(ffi.nullptr);
      _nativeEventCallback.close();
      await _eventController.close();
      await _queueController.close();
      await _playerStateController.close();
    }
  }

  // Event processing releases every native pointer exactly once before parsing.
  void _handleNativeEvent(
    ffi.Pointer<Utf8> pointer,
    ffi.Pointer<ffi.Uint8> bytes,
    int length,
  ) {
    String jsonText;
    Uint8List? imageBytes;
    try {
      jsonText = pointer == ffi.nullptr ? '' : pointer.toDartString();
      if (bytes != ffi.nullptr) {
        imageBytes = Uint8List.fromList(bytes.asTypedList(length));
      }
    } finally {
      _bindings.freeOwnedString(pointer);
      _bindings.freeOwnedBytes(bytes, length);
    }
    if (_disposed || jsonText.trim().isEmpty) return;
    unawaited(_nativeEventDelivery.add(jsonText, imageBytes));
  }

  void _processNativeEvent(Map<String, dynamic> statePayload) {
    try {
      if (_recordRustLogEvent(statePayload)) {
        return;
      }

      if (_completePendingOperation(statePayload)) {
        return;
      }

      _eventController.add(statePayload);

      _publishMediaSessionEvent(statePayload);
    } catch (_) {
      return;
    }
  }

  bool _recordRustLogEvent(Map<String, dynamic> payload) {
    if (payload['type'] != 'log') {
      return false;
    }

    final String? line = payload['line'] as String?;
    if (line != null) {
      AppLogBuffer.shared.add(line);
    }
    return true;
  }

  bool _completePendingOperation(Map<String, dynamic> payload) {
    final String? type = payload['type'] as String?;
    if (type != 'query_completed' &&
        type != 'query_failed' &&
        type != 'operation_completed' &&
        type != 'operation_failed') {
      return false;
    }

    final int? requestId = (payload['requestId'] as num?)?.toInt();
    if (requestId == null) {
      return true;
    }
    final Completer<Object?>? request = _pendingMusicAssistantQueries.remove(
      requestId,
    );
    if (request == null) {
      return true;
    }

    if (type == 'query_completed' || type == 'operation_completed') {
      request.complete(payload['result']);
      return true;
    }

    final Object? rawError = payload['error'];
    final String message = rawError is Map<String, dynamic>
        ? (rawError['message'] as String? ?? 'Unknown playlist track error')
        : 'Unknown playlist track error';
    request.completeError(StateError(message));
    return true;
  }

  void _publishMediaSessionEvent(Map<String, dynamic> payload) {
    switch (payload['type']) {
      case 'queue_changed':
        final queue = payload['queue'];
        if (queue is Map<String, dynamic>) {
          _queueController.add(Queue.fromJson(queue));
        }
      case 'player_state_changed':
        final playerState = payload['player_state'];
        if (playerState is Map<String, dynamic>) {
          _playerStateController.add(PlayerState.fromJson(playerState));
        }
    }
  }

  // FFI error mapping.
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
      _bindings.freeOwnedString(messagePointer);
    }
  }

  // Native library loading.
  static ffi.DynamicLibrary _openLibrary() {
    final fileName = _libraryFileNameForPlatform();
    final candidates = <String>[];

    final envOverride = Platform.environment['MASHIN_CORE_PATH']?.trim();
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
      _joinPath(cwd, 'rust', 'mashin_engine', 'target', 'debug', fileName),
    );
    candidates.add(
      _joinPath(cwd, 'rust', 'mashin_engine', 'target', 'release', fileName),
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
      'Mashin-Core-Bibliothek konnte nicht geladen werden. '
      'Geprüfte Pfade: ${candidates.join(' | ')}. '
      'Letzter Fehler: $lastError',
    );
  }

  static String _libraryFileNameForPlatform() {
    if (Platform.isWindows) {
      return 'mashin_engine.dll';
    }

    if (Platform.isMacOS || Platform.isIOS) {
      return 'libmashin_engine.dylib';
    }

    if (Platform.isAndroid || Platform.isLinux) {
      return 'libmashin_engine.so';
    }

    throw UnsupportedError('Mashin core is not supported on this platform.');
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

// Native symbol bindings.
class _RustBindings {
  _RustBindings(ffi.DynamicLibrary library)
    : login = library.lookupFunction<_NativeJsonCommandFn, _DartJsonCommandFn>(
        'mashin_core_login',
      ),
      restorePersistedSession = library
          .lookupFunction<_NativeVoidFn, _DartVoidFn>(
            'mashin_core_restore_persisted_session',
          ),
      selectLocalOutput = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_core_select_local_output',
      ),
      selectRemotePlayer = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_select_remote_player',
      ),
      setLogLevel = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_set_log_level',
      ),
      log = library.lookupFunction<_NativeLogFn, _DartLogFn>('mashin_core_log'),
      activateEngine = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_core_activate_engine',
      ),
      deactivateEngine = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_core_deactivate_engine',
      ),
      shutdown = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_core_shutdown',
      ),
      transportPlay = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_core_transport_play',
      ),
      transportPause = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_core_transport_pause',
      ),
      transportStop = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_core_transport_stop',
      ),
      transportNext = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_core_transport_next',
      ),
      transportPrevious = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_core_transport_previous',
      ),
      transportSeek = library.lookupFunction<_NativeSeekFn, _DartSeekFn>(
        'mashin_core_transport_seek',
      ),
      setRepeatMode = library.lookupFunction<_NativeVolumeFn, _DartVolumeFn>(
        'mashin_core_set_repeat_mode',
      ),
      setShuffle = library.lookupFunction<_NativeMutedFn, _DartMutedFn>(
        'mashin_core_set_shuffle',
      ),
      setAutoplay = library.lookupFunction<_NativeMutedFn, _DartMutedFn>(
        'mashin_core_set_autoplay',
      ),
      setVolume = library.lookupFunction<_NativeVolumeFn, _DartVolumeFn>(
        'mashin_core_set_volume',
      ),
      setMuted = library.lookupFunction<_NativeMutedFn, _DartMutedFn>(
        'mashin_core_set_muted',
      ),
      queuePlayMedia = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_queue_play_media',
      ),
      startRadio = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_start_radio',
      ),
      moveQueueItem = library
          .lookupFunction<_NativeQueueMoveFn, _DartQueueMoveFn>(
            'mashin_core_move_queue_item',
          ),
      moveQueueItemToEnd = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_move_queue_item_to_end',
      ),
      deleteQueueItems = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_delete_queue_items',
      ),
      queueSelect = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_select_queue_item',
      ),
      queuePlay = library.lookupFunction<_NativeVolumeFn, _DartVolumeFn>(
        'mashin_core_queue_play',
      ),
      setAudioProfile = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_set_audio_profile',
      ),
      setEventCallback = library
          .lookupFunction<
            _NativeRegisterEventCallbackFn,
            _DartRegisterEventCallbackFn
          >('mashin_core_set_event_callback'),
      freeOwnedBytes = library
          .lookupFunction<_NativeFreeBytesFn, _DartFreeBytesFn>(
            'mashin_core_free_owned_bytes',
          ),
      getQueue = library.lookupFunction<_NativeGetStringFn, _DartGetStringFn>(
        'mashin_core_get_queue',
      ),
      getPlayerState = library
          .lookupFunction<_NativeGetStringFn, _DartGetStringFn>(
            'mashin_core_get_player_state',
          ),
      getSessionSettings = library
          .lookupFunction<_NativeGetStringFn, _DartGetStringFn>(
            'mashin_core_get_session_settings',
          ),
      loadUserData = library
          .lookupFunction<_NativeGetStringFn, _DartGetStringFn>(
            'mashin_core_load_user_data',
          ),
      getUserData = library
          .lookupFunction<_NativeGetStringFn, _DartGetStringFn>(
            'mashin_core_get_user_data',
          ),
      setUserDataFavorites = library
          .lookupFunction<_NativeMediaFavoriteFn, _DartMediaFavoriteFn>(
            'mashin_core_set_user_data_favorites',
          ),
      createPrivatePlaylist = library
          .lookupFunction<_NativeCodecFn, _DartCodecFn>(
            'mashin_core_create_private_playlist',
          ),
      renamePrivatePlaylist = library
          .lookupFunction<_NativePlaylistMutationFn, _DartPlaylistMutationFn>(
            'mashin_core_rename_private_playlist',
          ),
      removePrivatePlaylist = library
          .lookupFunction<_NativeCodecFn, _DartCodecFn>(
            'mashin_core_remove_private_playlist',
          ),
      addPrivatePlaylistTracks = library
          .lookupFunction<_NativePlaylistMutationFn, _DartPlaylistMutationFn>(
            'mashin_core_add_private_playlist_tracks',
          ),
      removePrivatePlaylistTracks = library
          .lookupFunction<_NativePlaylistMutationFn, _DartPlaylistMutationFn>(
            'mashin_core_remove_private_playlist_tracks',
          ),
      startMusicAssistantQuery = library
          .lookupFunction<
            _NativeStartMusicAssistantQueryFn,
            _DartStartMusicAssistantQueryFn
          >('mashin_core_start_operation'),
      addPlaylistTracks = library
          .lookupFunction<_NativePlaylistMutationFn, _DartPlaylistMutationFn>(
            'mashin_core_add_playlist_tracks',
          ),
      removePlaylistTracks = library
          .lookupFunction<_NativePlaylistMutationFn, _DartPlaylistMutationFn>(
            'mashin_core_remove_playlist_tracks',
          ),
      getLastError = library
          .lookupFunction<_NativeGetStringFn, _DartGetStringFn>(
            'mashin_core_get_last_error',
          ),
      freeOwnedString = library
          .lookupFunction<_NativeFreeStringFn, _DartFreeStringFn>(
            'mashin_core_free_owned_string',
          );

  final _DartJsonCommandFn login;
  final _DartVoidFn restorePersistedSession;
  final _DartVoidFn selectLocalOutput;
  final _DartCodecFn selectRemotePlayer;
  final _DartCodecFn setLogLevel;
  final _DartLogFn log;
  final _DartVoidFn activateEngine;
  final _DartVoidFn deactivateEngine;
  final _DartVoidFn shutdown;
  final _DartVoidFn transportPlay;
  final _DartVoidFn transportPause;
  final _DartVoidFn transportStop;
  final _DartVoidFn transportNext;
  final _DartVoidFn transportPrevious;
  final _DartSeekFn transportSeek;
  final _DartVolumeFn setRepeatMode;
  final _DartMutedFn setShuffle;
  final _DartMutedFn setAutoplay;
  final _DartVolumeFn setVolume;
  final _DartMutedFn setMuted;
  final _DartCodecFn queuePlayMedia;
  final _DartCodecFn startRadio;
  final _DartQueueMoveFn moveQueueItem;
  final _DartCodecFn moveQueueItemToEnd;
  final _DartCodecFn deleteQueueItems;
  final _DartCodecFn queueSelect;
  final _DartVolumeFn queuePlay;
  final _DartCodecFn setAudioProfile;
  final _DartRegisterEventCallbackFn setEventCallback;
  final _DartFreeBytesFn freeOwnedBytes;
  final _DartGetStringFn getQueue;
  final _DartGetStringFn getPlayerState;
  final _DartGetStringFn getSessionSettings;
  final _DartGetStringFn loadUserData;
  final _DartGetStringFn getUserData;
  final _DartMediaFavoriteFn setUserDataFavorites;
  final _DartCodecFn createPrivatePlaylist;
  final _DartPlaylistMutationFn renamePrivatePlaylist;
  final _DartCodecFn removePrivatePlaylist;
  final _DartPlaylistMutationFn addPrivatePlaylistTracks;
  final _DartPlaylistMutationFn removePrivatePlaylistTracks;
  final _DartStartMusicAssistantQueryFn startMusicAssistantQuery;
  final _DartPlaylistMutationFn addPlaylistTracks;
  final _DartPlaylistMutationFn removePlaylistTracks;
  final _DartGetStringFn getLastError;
  final _DartFreeStringFn freeOwnedString;
}
