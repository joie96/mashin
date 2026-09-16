import 'dart:async';
import 'dart:convert';
import 'dart:ffi' as ffi;
import 'dart:io';

import 'package:ffi/ffi.dart';

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

    final code = _bindings.setEventCallback(
      _nativeEventCallback.nativeFunction,
    );
    _throwIfError(code, 'registerEventCallback');
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
  }) async {
    final config = <String, Object?>{
      'musicAssistantUrl': musicAssistantUrl.trim(),
      'username': username.trim(),
      'password': password,
    };

    final pointer = jsonEncode(config).toNativeUtf8();
    try {
      final code = _bindings.login(pointer);
      _throwIfError(code, 'login');
    } finally {
      calloc.free(pointer);
    }
    await refreshQueue();
    await refreshPlayerState();
  }

  @override
  Future<bool> restorePersistedSession() async {
    final code = _bindings.restorePersistedSession();
    if (code == 1) {
      return false;
    }
    _throwIfError(code, 'restorePersistedSession');
    await refreshQueue();
    await refreshPlayerState();
    return true;
  }

  @override
  Future<SessionSettings> getSessionSettings() async {
    final settings = _decodeNativeObject(
      _bindings.getSessionSettings(),
      'getSessionSettings',
    );
    return SessionSettings.fromJson(settings);
  }

  @override
  Future<void> selectDefaultSendspinOutput() async {
    _throwIfError(
      _bindings.selectDefaultSendspinOutput(),
      'selectDefaultSendspinOutput',
    );
    await refreshQueue();
    await refreshPlayerState();
  }

  @override
  Future<void> selectOfflineLibrary(String libraryRoot) async {
    await _selectOutputValue(
      _bindings.selectOfflineLibrary,
      libraryRoot,
      'selectOfflineLibrary',
    );
  }

  @override
  Future<void> selectRemotePlayer(String playerId) async {
    await _selectOutputValue(
      _bindings.selectRemotePlayer,
      playerId,
      'selectRemotePlayer',
    );
  }

  Future<void> _selectOutputValue(
    _DartCodecFn command,
    String value,
    String operation,
  ) async {
    final pointer = value.toNativeUtf8();
    try {
      _throwIfError(command(pointer), operation);
    } finally {
      calloc.free(pointer);
    }
    await refreshQueue();
    await refreshPlayerState();
  }

  // Playback state reads and named queries.
  @override
  Future<void> refreshQueue() async {
    final queue = _decodeNativeObject(_bindings.getQueue(), 'getQueue');
    _queueController.add(Queue.fromJson(queue));
  }

  @override
  Future<void> refreshPlayerState() async {
    final playerState = _decodeNativeObject(
      _bindings.getPlayerState(),
      'getPlayerState',
    );
    _playerStateController.add(PlayerState.fromJson(playerState));
  }

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
        'startMusicAssistantQuery',
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
  Future<void> activateEngine() async {
    final code = _bindings.activateEngine();
    _throwIfError(code, 'activateEngine');
  }

  @override
  Future<void> deactivateEngine() async {
    final code = _bindings.deactivateEngine();
    _throwIfError(code, 'deactivateEngine');
  }

  @override
  Future<void> shutdown() async {
    final code = _bindings.shutdown();
    _throwIfError(code, 'shutdown');
  }

  // Playback transport commands.
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

  // Playback preferences.
  @override
  Future<void> setAutoplay(bool enabled) async {
    final code = _bindings.setAutoplay(enabled ? 1 : 0);
    _throwIfError(code, 'setAutoplay');
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

  // Queue commands resolve media in Rust before mutating the canonical queue.
  @override
  Future<void> playMedia(List<MediaItem> mediaItems) =>
      _playMediaJson(_encodeMediaItems(mediaItems));

  @override
  Future<void> playShuffledMedia(List<MediaItem> mediaItems) =>
      _playShuffledMediaJson(_encodeMediaItems(mediaItems));

  @override
  Future<void> playSendspinRadioNext(List<MediaItem> mediaItems) =>
      _playSendspinRadioNextJson(_encodeMediaItems(mediaItems));

  @override
  Future<void> playMediaNext(List<MediaItem> mediaItems) =>
      _playMediaNextJson(_encodeMediaItems(mediaItems));

  @override
  Future<void> playMediaReplaceNext(List<MediaItem> mediaItems) =>
      _playMediaReplaceNextJson(_encodeMediaItems(mediaItems));

  @override
  Future<void> playMediaLast(List<MediaItem> mediaItems) =>
      _playMediaLastJson(_encodeMediaItems(mediaItems));

  Future<void> _playMediaJson(String mediaItemsJson) async {
    final pointer = mediaItemsJson.toNativeUtf8();
    try {
      final code = _bindings.queueReplace(pointer);
      _throwIfError(code, 'playMedia');
    } finally {
      calloc.free(pointer);
    }
  }
  // Queue item mutations.

  Future<void> _playShuffledMediaJson(String mediaItemsJson) => _sendMediaItems(
    _bindings.queueReplaceShuffled,
    mediaItemsJson,
    'queueReplaceShuffled',
  );

  Future<void> _playMediaNextJson(String mediaItemsJson) => _sendMediaItems(
    _bindings.queueInsertNext,
    mediaItemsJson,
    'queueInsertNext',
  );

  Future<void> _playMediaReplaceNextJson(String mediaItemsJson) =>
      _sendMediaItems(
        _bindings.queueReplaceNext,
        mediaItemsJson,
        'queueReplaceNext',
      );

  Future<void> _playMediaLastJson(String mediaItemsJson) =>
      _sendMediaItems(_bindings.queueAppend, mediaItemsJson, 'queueAppend');

  Future<void> _playSendspinRadioNextJson(String mediaItemsJson) =>
      _sendMediaItems(
        _bindings.queueRadioNext,
        mediaItemsJson,
        'queueSendspinRadioNext',
      );

  @override
  Future<void> clearQueue() async {
    _throwIfError(_bindings.queueClear(), 'queueClear');
  }

  @override
  Future<void> moveQueueItem(String itemId, int shift) async {
    final pointer = itemId.toNativeUtf8();
    try {
      _throwIfError(_bindings.queueMove(pointer, shift), 'queueMove');
    } finally {
      calloc.free(pointer);
    }
  }

  @override
  Future<void> deleteQueueItem(String itemId) async {
    final pointer = itemId.toNativeUtf8();
    try {
      _throwIfError(_bindings.queueRemove(pointer), 'queueRemove');
    } finally {
      calloc.free(pointer);
    }
  }

  @override
  Future<void> playQueueIndex(int index) async {
    _throwIfError(_bindings.queuePlay(index), 'queuePlay');
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

  String _encodeMediaItems(List<MediaItem> mediaItems) => jsonEncode(
    mediaItems
        .map(createMusicAssistantMediaItemPayload)
        .toList(growable: false),
  );

  // Output codec selection.
  @override
  Future<void> requestCodec(String codec) async {
    final pointer = codec.toNativeUtf8();
    try {
      _throwIfError(_bindings.setCodec(pointer), 'requestCodec');
    } finally {
      calloc.free(pointer);
    }
  }

  // Playlist mutations.
  @override
  Future<void> addPlaylistTracks(String playlistId, List<String> uris) {
    return _executePlaylistMutation(
      _bindings.addPlaylistTracks,
      playlistId,
      jsonEncode(uris),
      'addPlaylistTracks',
    );
  }

  @override
  Future<void> removePlaylistTracks(
    String playlistId,
    List<int> positionsToRemove,
  ) {
    return _executePlaylistMutation(
      _bindings.removePlaylistTracks,
      playlistId,
      jsonEncode(positionsToRemove),
      'removePlaylistTracks',
    );
  }

  @override
  Future<Map<String, dynamic>> loadUserData() async =>
      _decodeNativeObject(_bindings.loadUserData(), 'loadUserData');

  @override
  Future<Map<String, dynamic>> getUserData() async =>
      _decodeNativeObject(_bindings.getUserData(), 'getUserData');

  @override
  Future<void> setUserDataFavorites(List<MediaItem> items, bool favorite) =>
      _executeJsonBooleanCommand(
        _bindings.setUserDataFavorites,
        _encodeMediaItems(items),
        favorite,
        'setUserDataFavorites',
      );

  @override
  Future<void> createLocalPlaylist(String name) => _executeStringCommand(
    _bindings.createLocalPlaylist,
    name,
    'createLocalPlaylist',
  );

  @override
  Future<void> renameLocalPlaylist(String playlistId, String name) =>
      _executeTwoStringCommand(
        _bindings.renameLocalPlaylist,
        playlistId,
        name,
        'renameLocalPlaylist',
      );

  @override
  Future<void> removeLocalPlaylist(String playlistId) => _executeStringCommand(
    _bindings.removeLocalPlaylist,
    playlistId,
    'removeLocalPlaylist',
  );

  @override
  Future<void> addLocalPlaylistTracks(String playlistId, List<Track> tracks) =>
      _executeTwoStringCommand(
        _bindings.addLocalPlaylistTracks,
        playlistId,
        jsonEncode(tracks.map(createMusicAssistantMediaItemPayload).toList()),
        'addLocalPlaylistTracks',
      );

  @override
  Future<void> removeLocalPlaylistTracks(
    String playlistId,
    List<String> uris,
  ) => _executeTwoStringCommand(
    _bindings.removeLocalPlaylistTracks,
    playlistId,
    jsonEncode(uris),
    'removeLocalPlaylistTracks',
  );

  Future<void> _executeStringCommand(
    _DartCodecFn command,
    String value,
    String operation,
  ) async {
    final pointer = value.toNativeUtf8();
    try {
      _throwIfError(command(pointer), operation);
    } finally {
      calloc.free(pointer);
    }
  }

  Future<void> _executeTwoStringCommand(
    _DartPlaylistMutationFn command,
    String first,
    String second,
    String operation,
  ) async {
    final firstPointer = first.toNativeUtf8();
    final secondPointer = second.toNativeUtf8();
    try {
      _throwIfError(command(firstPointer, secondPointer), operation);
    } finally {
      calloc.free(secondPointer);
      calloc.free(firstPointer);
    }
  }

  Future<void> _executeJsonBooleanCommand(
    _DartMediaFavoriteFn command,
    String json,
    bool value,
    String operation,
  ) async {
    final jsonPointer = json.toNativeUtf8();
    try {
      _throwIfError(command(jsonPointer, value ? 1 : 0), operation);
    } finally {
      calloc.free(jsonPointer);
    }
  }

  Future<void> _executePlaylistMutation(
    _DartPlaylistMutationFn command,
    String playlistId,
    String valuesJson,
    String operation,
  ) async {
    final playlistPointer = playlistId.toNativeUtf8();
    final valuesPointer = valuesJson.toNativeUtf8();
    try {
      _throwIfError(command(playlistPointer, valuesPointer), operation);
    } finally {
      calloc.free(valuesPointer);
      calloc.free(playlistPointer);
    }
  }

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

      if (_completeMusicAssistantQuery(statePayload)) {
        return;
      }

      _eventController.add(statePayload);

      _publishMediaSessionEvent(statePayload);
    } catch (_) {
      return;
    }
  }

  bool _completeMusicAssistantQuery(Map<String, dynamic> payload) {
    final String? type = payload['type'] as String?;
    if (type != 'query_completed' && type != 'query_failed') {
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

    if (type == 'query_completed') {
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

  Map<String, dynamic> _decodeNativeObject(
    ffi.Pointer<Utf8> pointer,
    String operation,
  ) {
    if (pointer == ffi.nullptr) {
      _throwIfError(-1, operation);
    }
    try {
      final decoded = jsonDecode(pointer.toDartString());
      if (decoded is! Map<String, dynamic>) {
        throw FormatException('$operation must return an object');
      }
      return decoded;
    } finally {
      _bindings.freeOwnedString(pointer);
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
          >('mashin_core_start_music_assistant_query'),
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
