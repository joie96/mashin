import 'dart:async';
import 'dart:convert';
import 'dart:ffi' as ffi;
import 'dart:io';

import 'package:ffi/ffi.dart';

import '../../app/logging/app_log_buffer.dart';
import 'core_models.dart';

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

typedef _NativeEventCallbackFn = ffi.Void Function(ffi.Pointer<Utf8>);
typedef _NativeRegisterEventCallbackFn =
    ffi.Int32 Function(ffi.Pointer<ffi.NativeFunction<_NativeEventCallbackFn>>);
typedef _DartRegisterEventCallbackFn =
    int Function(ffi.Pointer<ffi.NativeFunction<_NativeEventCallbackFn>>);
typedef _NativeJsonCommandFn = ffi.Int32 Function(ffi.Pointer<Utf8>);
typedef _DartJsonCommandFn = int Function(ffi.Pointer<Utf8>);

abstract class MashinCoreBridgeApi {
  Stream<Queue> get queueChanges;
  Stream<PlayerState> get playerStateChanges;
  Stream<Map<String, dynamic>> get events;

  Future<void> login({
    required String musicAssistantUrl,
    required String username,
    required String password,
  });
  Future<bool> restorePersistedSession();
  Future<SessionSettings> getSessionSettings();
  Future<void> selectDefaultSendspinOutput();
  Future<void> selectOfflineLibrary(String libraryRoot);
  Future<void> selectRemotePlayer(String playerId);
  Future<void> refreshQueue();
  Future<void> refreshPlayerState();
  Future<List<RecommendationFolder>> getRecommendations();
  Future<List<ProviderManifest>> getProviderManifests();
  Future<String?> getProviderIconData(String provider);
  Future<List<MediaItem>> getRecentlyPlayedTracks({int limit = 40});
  Future<SearchResults> searchMusic(String query, {int limit = 50});
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
  Future<List<Album>> getArtistAlbums(String itemId, String provider);
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
  Future<void> playMedia(List<MediaItem> mediaItems);
  Future<void> playShuffledMedia(List<MediaItem> mediaItems);
  Future<void> playSendspinRadioNext(List<MediaItem> mediaItems);
  Future<void> playMediaNext(List<MediaItem> mediaItems);
  Future<void> playMediaReplaceNext(List<MediaItem> mediaItems);
  Future<void> playMediaLast(List<MediaItem> mediaItems);
  Future<void> clearQueue();
  Future<void> moveQueueItem(String itemId, int shift);
  Future<void> deleteQueueItem(String itemId);
  Future<void> playQueueIndex(int index);
  Future<void> requestCodec(String codec);
  Future<void> setLocalCacheSizeBytes(int cacheSizeBytes);
  Future<void> setTheme(bool isDarkTheme);
  Future<void> setLogLevel(LogLevelPreference logLevel);
  Future<String> saveLogSnapshot();
  void log(LogLevelPreference level, String category, String message);
  Future<void> addPlaylistTracks(String playlistId, List<String> uris);
  Future<void> removePlaylistTracks(
    String playlistId,
    List<int> positionsToRemove,
  );
  Future<Map<String, dynamic>> loadUserData();
  Future<Map<String, dynamic>> getUserData();
  Future<void> setUserDataFavorites(List<MediaItem> items, bool favorite);
  Future<void> createLocalPlaylist(String name);
  Future<void> renameLocalPlaylist(String playlistId, String name);
  Future<void> removeLocalPlaylist(String playlistId);
  Future<void> addLocalPlaylistTracks(String playlistId, List<Track> tracks);
  Future<void> removeLocalPlaylistTracks(String playlistId, List<String> uris);
  Future<void> dispose();
}

class MashinCoreBridge implements MashinCoreBridgeApi {
  MashinCoreBridge() : _bindings = _RustBindings(_openLibrary()) {
    _instance = this;
    _setRustLogLevel(
      const String.fromEnvironment('MASHIN_LOG_LEVEL', defaultValue: 'info'),
    );
    _nativeEventCallback = ffi.NativeCallable<_NativeEventCallbackFn>.listener(
      _dispatchNativeEvent,
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

  final _RustBindings _bindings;
  late final ffi.NativeCallable<_NativeEventCallbackFn> _nativeEventCallback;
  final StreamController<Queue> _queueController =
      StreamController<Queue>.broadcast();
  final StreamController<PlayerState> _playerStateController =
      StreamController<PlayerState>.broadcast();
  final StreamController<Map<String, dynamic>> _eventController =
      StreamController<Map<String, dynamic>>.broadcast();
  final Map<int, Completer<Object?>> _pendingMusicAssistantQueries =
      <int, Completer<Object?>>{};
  int _nextMusicAssistantQueryId = 0;
  bool _disposed = false;

  @override
  Stream<Queue> get queueChanges => _queueController.stream;

  @override
  Stream<PlayerState> get playerStateChanges => _playerStateController.stream;

  @override
  Stream<Map<String, dynamic>> get events => _eventController.stream;

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
  Future<void> selectDefaultSendspinOutput() => _submitOperation<void>(
    'selectDefaultSendspinOutput',
    const <String, Object?>{},
    (_) {},
  );

  @override
  Future<void> selectOfflineLibrary(String libraryRoot) =>
      _submitOperation<void>('selectOfflineLibrary', <String, Object?>{
        'libraryRoot': libraryRoot,
      }, (_) {});

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
  Future<List<RecommendationFolder>> getRecommendations() =>
      _startMusicAssistantQuery<List<RecommendationFolder>>(
        'recommendations',
        const <String, Object?>{},
        (Object? decoded) {
          if (decoded is! List) {
            throw const FormatException('recommendations must be a list');
          }
          return decoded
              .whereType<Map<String, dynamic>>()
              .map(RecommendationFolder.fromJson)
              .toList(growable: false);
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
  Future<List<MediaItem>> getRecentlyPlayedTracks({int limit = 40}) =>
      _startMusicAssistantQuery<List<MediaItem>>(
        'recentlyPlayedTracks',
        <String, Object?>{'limit': limit.clamp(1, 100)},
        (Object? decoded) {
          if (decoded is! List) {
            throw const FormatException('recently played media must be a list');
          }
          return decoded
              .whereType<Map<String, dynamic>>()
              .map(mediaItemFromJson)
              .toList(growable: false);
        },
      );

  @override
  Future<SearchResults> searchMusic(String query, {int limit = 50}) =>
      _startMusicAssistantQuery<SearchResults>(
        'searchMusic',
        <String, Object?>{'query': query, 'limit': limit.clamp(1, 100)},
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
      _decodeTracks,
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
    _decodeTracks,
  );

  @override
  Future<List<Album>> getArtistAlbums(String itemId, String provider) {
    return _startMusicAssistantQuery<List<Album>>(
      'artistAlbums',
      <String, Object?>{'itemId': itemId, 'provider': provider},
      _decodeAlbums,
    );
  }

  @override
  Future<List<Track>> getArtistTopTracks(String itemId, String provider) {
    return _startMusicAssistantQuery<List<Track>>(
      'artistTopTracks',
      <String, Object?>{'itemId': itemId, 'provider': provider},
      _decodeTracks,
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
      _decodeTracks,
    );
  }

  @override
  Future<List<Track>> getTrackVersions(String itemId, String provider) {
    return _startMusicAssistantQuery<List<Track>>(
      'trackVersions',
      <String, Object?>{'itemId': itemId, 'provider': provider},
      _decodeTracks,
    );
  }

  Future<T> _startMusicAssistantQuery<T>(
    String kind,
    Map<String, Object?> arguments,
    T Function(Object? decoded) decoder,
  ) => _submitOperation<T>(kind, arguments, decoder);

  Future<T> _submitOperation<T>(
    String kind,
    Map<String, Object?> arguments,
    T Function(Object? result) decoder,
  ) async {
    final int requestId = ++_nextMusicAssistantQueryId;
    final Completer<Object?> completer = Completer<Object?>();
    _pendingMusicAssistantQueries[requestId] = completer;
    final String request = jsonEncode(<String, Object?>{
      'requestId': requestId,
      'kind': kind,
      ...arguments,
    });
    final pointer = request.toNativeUtf8();
    try {
      _throwIfError(
        _bindings.startMusicAssistantQuery(pointer),
        'startOperation',
      );
    } catch (_) {
      _pendingMusicAssistantQueries.remove(requestId);
      rethrow;
    } finally {
      calloc.free(pointer);
    }
    return decoder(await completer.future);
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

  List<Track> _decodeTracks(Object? decoded) {
    if (decoded is! List) {
      throw const FormatException('tracks must be a list');
    }
    return decoded
        .whereType<Map<String, dynamic>>()
        .map(Track.fromJson)
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

  // Queue commands resolve media in Rust before mutating the canonical queue.
  @override
  Future<void> playMedia(List<MediaItem> mediaItems) =>
      _submitMediaOperation('playMedia', mediaItems);

  @override
  Future<void> playShuffledMedia(List<MediaItem> mediaItems) =>
      _submitMediaOperation('playShuffledMedia', mediaItems);

  @override
  Future<void> playSendspinRadioNext(List<MediaItem> mediaItems) =>
      _submitMediaOperation('playSendspinRadioNext', mediaItems);

  @override
  Future<void> playMediaNext(List<MediaItem> mediaItems) =>
      _submitMediaOperation('playMediaNext', mediaItems);

  @override
  Future<void> playMediaReplaceNext(List<MediaItem> mediaItems) =>
      _submitMediaOperation('playMediaReplaceNext', mediaItems);

  @override
  Future<void> playMediaLast(List<MediaItem> mediaItems) =>
      _submitMediaOperation('playMediaLast', mediaItems);

  Future<void> _submitMediaOperation(String kind, List<MediaItem> mediaItems) =>
      _submitVoidOperation(kind, <String, Object?>{
        'mediaItems': mediaItems
            .map(createMusicAssistantMediaItemPayload)
            .toList(),
      });
  // Queue item mutations.

  @override
  Future<void> clearQueue() => _submitVoidOperation('clearQueue');

  @override
  Future<void> moveQueueItem(String itemId, int shift) => _submitVoidOperation(
    'moveQueueItem',
    <String, Object?>{'itemId': itemId, 'shift': shift},
  );

  @override
  Future<void> deleteQueueItem(String itemId) => _submitVoidOperation(
    'deleteQueueItem',
    <String, Object?>{'itemId': itemId},
  );

  @override
  Future<void> playQueueIndex(int index) =>
      _submitVoidOperation('playQueueIndex', <String, Object?>{'index': index});

  // Output codec selection.
  @override
  Future<void> requestCodec(String codec) =>
      _submitVoidOperation('requestCodec', <String, Object?>{'codec': codec});

  @override
  Future<void> setLocalCacheSizeBytes(int cacheSizeBytes) =>
      _submitVoidOperation('setLocalCacheSizeBytes', <String, Object?>{
        'cacheSizeBytes': cacheSizeBytes,
      });

  @override
  Future<void> setTheme(bool isDarkTheme) => _submitVoidOperation(
    'setTheme',
    <String, Object?>{'theme': isDarkTheme ? 'dark' : 'light'},
  );

  @override
  Future<void> setLogLevel(LogLevelPreference logLevel) async {
    await _submitVoidOperation('setLogLevel', <String, Object?>{
      'logLevel': logLevel.name,
    });
    _reapplyRuntimeLogLevelOverride();
  }

  @override
  Future<String> saveLogSnapshot() => _submitOperation<String>(
    'saveLogSnapshot',
    const <String, Object?>{},
    (Object? result) => result! as String,
  );

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
  Future<void> setUserDataFavorites(List<MediaItem> items, bool favorite) =>
      _submitVoidOperation('setUserDataFavorites', <String, Object?>{
        'mediaItems': items.map(createMusicAssistantMediaItemPayload).toList(),
        'favorite': favorite,
      });

  @override
  Future<void> createLocalPlaylist(String name) => _submitVoidOperation(
    'createLocalPlaylist',
    <String, Object?>{'name': name},
  );

  @override
  Future<void> renameLocalPlaylist(String playlistId, String name) =>
      _submitVoidOperation('renameLocalPlaylist', <String, Object?>{
        'playlistId': playlistId,
        'name': name,
      });

  @override
  Future<void> removeLocalPlaylist(String playlistId) => _submitVoidOperation(
    'removeLocalPlaylist',
    <String, Object?>{'playlistId': playlistId},
  );

  @override
  Future<void> addLocalPlaylistTracks(String playlistId, List<Track> tracks) =>
      _submitVoidOperation('addLocalPlaylistTracks', <String, Object?>{
        'playlistId': playlistId,
        'tracks': tracks.map(createMusicAssistantMediaItemPayload).toList(),
      });

  @override
  Future<void> removeLocalPlaylistTracks(
    String playlistId,
    List<String> uris,
  ) => _submitVoidOperation('removeLocalPlaylistTracks', <String, Object?>{
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
    if (_disposed) {
      return;
    }

    _disposed = true;
    _instance = null;
    for (final Completer<Object?> request
        in _pendingMusicAssistantQueries.values) {
      request.completeError(StateError('Mashin core bridge was disposed.'));
    }
    _pendingMusicAssistantQueries.clear();
    _bindings.setEventCallback(ffi.nullptr);
    _nativeEventCallback.close();
    await _eventController.close();
    await _queueController.close();
    await _playerStateController.close();
  }

  // The native callback transfers ownership of its string pointer to this bridge.
  static void _dispatchNativeEvent(ffi.Pointer<Utf8> pointer) {
    final bridge = _instance;
    if (bridge == null) {
      return;
    }

    bridge._handleNativeEvent(pointer);
  }

  // Event processing releases every native pointer exactly once before parsing.
  void _handleNativeEvent(ffi.Pointer<Utf8> pointer) {
    if (pointer == ffi.nullptr) {
      return;
    }

    final jsonText = pointer.toDartString();
    _bindings.freeOwnedString(pointer);

    if (jsonText.trim().isEmpty) {
      return;
    }

    try {
      final dynamic decoded = jsonDecode(jsonText);
      if (decoded is! Map<String, dynamic>) {
        return;
      }

      final statePayload = decoded;

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
      'Mashin-Core-Bibliothek konnte nicht geladen werden. '
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
      selectDefaultSendspinOutput = library
          .lookupFunction<_NativeVoidFn, _DartVoidFn>(
            'mashin_core_select_default_sendspin_output',
          ),
      selectOfflineLibrary = library
          .lookupFunction<_NativeCodecFn, _DartCodecFn>(
            'mashin_core_select_offline_library',
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
      queueReplace = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_queue_replace',
      ),
      queueReplaceShuffled = library
          .lookupFunction<_NativeCodecFn, _DartCodecFn>(
            'mashin_core_queue_replace_shuffled',
          ),
      queueInsertNext = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_queue_insert_next',
      ),
      queueReplaceNext = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_queue_replace_next',
      ),
      queueAppend = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_queue_append',
      ),
      queueRadioNext = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_sendspin_queue_radio_next',
      ),
      queueClear = library.lookupFunction<_NativeVoidFn, _DartVoidFn>(
        'mashin_core_queue_clear',
      ),
      queueMove = library.lookupFunction<_NativeQueueMoveFn, _DartQueueMoveFn>(
        'mashin_core_queue_move',
      ),
      queueRemove = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_queue_remove',
      ),
      queuePlay = library.lookupFunction<_NativeVolumeFn, _DartVolumeFn>(
        'mashin_core_queue_play',
      ),
      setCodec = library.lookupFunction<_NativeCodecFn, _DartCodecFn>(
        'mashin_core_set_codec',
      ),
      setEventCallback = library
          .lookupFunction<
            _NativeRegisterEventCallbackFn,
            _DartRegisterEventCallbackFn
          >('mashin_core_set_event_callback'),
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
      createLocalPlaylist = library
          .lookupFunction<_NativeCodecFn, _DartCodecFn>(
            'mashin_core_create_local_playlist',
          ),
      renameLocalPlaylist = library
          .lookupFunction<_NativePlaylistMutationFn, _DartPlaylistMutationFn>(
            'mashin_core_rename_local_playlist',
          ),
      removeLocalPlaylist = library
          .lookupFunction<_NativeCodecFn, _DartCodecFn>(
            'mashin_core_remove_local_playlist',
          ),
      addLocalPlaylistTracks = library
          .lookupFunction<_NativePlaylistMutationFn, _DartPlaylistMutationFn>(
            'mashin_core_add_local_playlist_tracks',
          ),
      removeLocalPlaylistTracks = library
          .lookupFunction<_NativePlaylistMutationFn, _DartPlaylistMutationFn>(
            'mashin_core_remove_local_playlist_tracks',
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
  final _DartVoidFn selectDefaultSendspinOutput;
  final _DartCodecFn selectOfflineLibrary;
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
  final _DartCodecFn queueReplace;
  final _DartCodecFn queueReplaceShuffled;
  final _DartCodecFn queueInsertNext;
  final _DartCodecFn queueReplaceNext;
  final _DartCodecFn queueAppend;
  final _DartCodecFn queueRadioNext;
  final _DartVoidFn queueClear;
  final _DartQueueMoveFn queueMove;
  final _DartCodecFn queueRemove;
  final _DartVolumeFn queuePlay;
  final _DartCodecFn setCodec;
  final _DartRegisterEventCallbackFn setEventCallback;
  final _DartGetStringFn getQueue;
  final _DartGetStringFn getPlayerState;
  final _DartGetStringFn getSessionSettings;
  final _DartGetStringFn loadUserData;
  final _DartGetStringFn getUserData;
  final _DartMediaFavoriteFn setUserDataFavorites;
  final _DartCodecFn createLocalPlaylist;
  final _DartPlaylistMutationFn renameLocalPlaylist;
  final _DartCodecFn removeLocalPlaylist;
  final _DartPlaylistMutationFn addLocalPlaylistTracks;
  final _DartPlaylistMutationFn removeLocalPlaylistTracks;
  final _DartStartMusicAssistantQueryFn startMusicAssistantQuery;
  final _DartPlaylistMutationFn addPlaylistTracks;
  final _DartPlaylistMutationFn removePlaylistTracks;
  final _DartGetStringFn getLastError;
  final _DartFreeStringFn freeOwnedString;
}
