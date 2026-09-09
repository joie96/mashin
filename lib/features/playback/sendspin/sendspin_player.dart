import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import '../../../app/config/app_config.dart';
import '../../../data/music_assistant/ma_client.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../playback_models.dart';
import '../playback_player.dart';
import 'sendspin_models.dart';
import 'sendspin_rust_bridge.dart';

class SendspinPlayer implements PlaybackPlayer {
  SendspinPlayer({
    required this.transportBackend,
    required this.clientResolver,
  });

  final SendspinRustBridge transportBackend;
  final MusicAssistantClient? Function() clientResolver;
  final StreamController<PlaybackState> _playbackStateController =
      StreamController<PlaybackState>.broadcast();
  final StreamController<PlaybackQueue> _queueStateController =
      StreamController<PlaybackQueue>.broadcast();

  static const int _autoplayRefillThreshold = 5;
  static const int _autoplayBatchSize = 25;
  static const int _autoplayMaxSeedItems = 10;
  static const int _radioMaxTrackDurationSeconds = 20 * 60;
  static const int _radioBaseSampleSize = 5;

  StreamSubscription<SendspinPlayerState>? _stateSubscription;
  PlaybackState _playbackState = const PlaybackState();
  PlaybackQueue _queueState = const PlaybackQueue();
  final List<MediaItem> _autoplaySeedItems = <MediaItem>[];
  bool _autoplayEnabled = false;
  bool _autoplayRefillInProgress = false;

  @override
  PlaybackOutputMode get outputMode => transportBackend.outputMode;

  @override
  Stream<PlaybackState> get playbackStates => _playbackStateController.stream;

  @override
  Stream<PlaybackQueue> get queueStates => _queueStateController.stream;

  @override
  Future<void> activate({String? targetPlayerId}) async {
    await _ensureStateSubscription();
    await transportBackend.activate(targetPlayerId: targetPlayerId);
  }

  @override
  Future<void> deactivate() => transportBackend.deactivate();

  @override
  Future<void> connect() => transportBackend.connect();

  @override
  Future<void> terminate() async {
    await _stateSubscription?.cancel();
    _stateSubscription = null;
    await _playbackStateController.close();
    await _queueStateController.close();
    await transportBackend.terminate();
  }

  @override
  Future<void> transportPlay() => transportBackend.transportPlay();

  @override
  Future<void> transportPause() => transportBackend.transportPause();

  @override
  Future<void> transportNext() => transportBackend.transportNext();

  @override
  Future<void> transportPrevious() => transportBackend.transportPrevious();

  @override
  Future<void> transportSeek(double seconds) =>
      transportBackend.transportSeek(seconds);

  @override
  Future<void> setVolume(int volume) => transportBackend.setVolume(volume);

  @override
  Future<void> setMuted(bool muted) => transportBackend.setMuted(muted);

  @override
  Future<void> playMedia(List<MediaItem> mediaItems) async {
    _resetAutoplaySeeds(mediaItems);
    await transportBackend.playMediaJson(await _mediaItemsJson(mediaItems));
  }

  @override
  Future<void> playMediaNext(List<MediaItem> mediaItems) async {
    _appendAutoplaySeeds(mediaItems);
    await transportBackend.playMediaNextJson(await _mediaItemsJson(mediaItems));
  }

  @override
  Future<void> playMediaReplaceNext(List<MediaItem> mediaItems) async {
    _resetAutoplaySeeds(mediaItems);
    await transportBackend.playMediaReplaceNextJson(
      await _mediaItemsJson(mediaItems),
    );
  }

  @override
  Future<void> playMediaLast(List<MediaItem> mediaItems) async {
    _appendAutoplaySeeds(mediaItems);
    await transportBackend.playMediaLastJson(await _mediaItemsJson(mediaItems));
  }

  @override
  Future<void> playMediaRadioNext(List<MediaItem> mediaItems) async {
    _appendAutoplaySeeds(mediaItems);
    final tracks = await _radioTracksFromSeeds(mediaItems);
    if (tracks.isEmpty) {
      return;
    }
    await transportBackend.playMediaNextJson(await _tracksJson(tracks));
  }

  @override
  Future<void> shufflePlayMedia(List<MediaItem> mediaItems) {
    final shuffled = List<MediaItem>.of(mediaItems)..shuffle();
    return playMedia(shuffled);
  }

  @override
  Future<void> clearQueue({bool skipStop = false}) =>
      transportBackend.clearQueue();

  @override
  Future<void> playQueueIndex(int index) =>
      transportBackend.playQueueIndex(index);

  @override
  Future<void> moveQueueItem(String queueItemId, {int posShift = 0}) =>
      transportBackend.moveQueueItem(queueItemId, posShift);

  @override
  Future<void> deleteQueueItem(String queueItemId) =>
      transportBackend.deleteQueueItem(queueItemId);

  @override
  Future<void> setShuffle(bool enabled) =>
      transportBackend.transportShuffle(enabled);

  @override
  Future<void> setRepeatMode(RepeatMode repeatMode) =>
      transportBackend.transportRepeatMode(repeatMode);

  Future<void> setPreferredAudioCodec(String codec) async {
    await AppConfig.setSendspinPreferredCodec(codec);
    await transportBackend.requestCodec(codec);
  }

  @override
  Future<void> setDontStopTheMusic(bool enabled) async {
    await setAutoplay(enabled);
  }

  Future<void> setAutoplay(bool enabled) async {
    _autoplayEnabled = enabled;
    _queueState = _queueState.copyWith(dontStopTheMusicEnabled: enabled);
    if (!_queueStateController.isClosed) {
      _queueStateController.add(_queueState);
    }
    if (enabled) {
      unawaited(_maybeRefillAutoplay());
    }
  }

  @override
  Future<PlaybackState> getPlaybackState() async {
    await _ensureStateSubscription();
    _handleState(await transportBackend.getPlaybackState());
    return _playbackState;
  }

  @override
  Future<PlaybackQueue> getQueueState() async {
    await _ensureStateSubscription();
    return _queueState;
  }

  Future<String> _mediaItemsJson(List<MediaItem> mediaItems) async {
    final client = clientResolver();
    if (client == null) {
      return jsonEncode(const <Map<String, dynamic>>[]);
    }
    final resolved = await client.resolvePlayableMediaItems(mediaItems);
    return jsonEncode(
      resolved.map(client.createMediaItemPayload).toList(growable: false),
    );
  }

  Future<String> _tracksJson(List<Track> tracks) async {
    final client = clientResolver();
    if (client == null) {
      return jsonEncode(const <Map<String, dynamic>>[]);
    }
    return jsonEncode(
      tracks.map(client.createMediaItemPayload).toList(growable: false),
    );
  }

  Future<List<Track>> _radioTracksFromSeeds(List<MediaItem> seeds) async {
    final client = clientResolver();
    if (client == null) {
      return const <Track>[];
    }
    final seedTrackKeys = (await client.resolvePlayableMediaItems(
      seeds,
    )).whereType<Track>().map(_trackKey).toSet();
    final tracks = <Track>[];
    for (final seed in seeds) {
      final uri = _seedUri(seed);
      if (uri == null) {
        continue;
      }
      tracks.addAll(
        await client.getPlaylistTracks(
          uri,
          'radio_playlist',
          forceRefresh: true,
          allowDynamicTracks: true,
        ),
      );
    }
    return _dedupeTracks(tracks)
        .where((track) => !seedTrackKeys.contains(_trackKey(track)))
        .where((track) => track.duration <= _radioMaxTrackDurationSeconds)
        .toList(growable: false);
  }

  Future<void> _maybeRefillAutoplay() async {
    if (!_autoplayEnabled || _autoplayRefillInProgress) {
      return;
    }
    final currentIndex = _queueState.currentIndex;
    if (currentIndex == null || _autoplaySeedItems.isEmpty) {
      return;
    }
    final upcomingCount = _queueState.items.length - currentIndex - 1;
    if (upcomingCount >= _autoplayRefillThreshold) {
      return;
    }

    _autoplayRefillInProgress = true;
    try {
      final tracks = await _autoplayTracksFromSeeds();
      if (tracks.isEmpty) {
        return;
      }
      await transportBackend.playMediaLastJson(await _tracksJson(tracks));
    } catch (_) {
      return;
    } finally {
      _autoplayRefillInProgress = false;
    }
  }

  Future<List<Track>> _autoplayTracksFromSeeds() async {
    final client = clientResolver();
    if (client == null) {
      return const <Track>[];
    }
    final baseTracks = await client.resolvePlayableMediaItems(
      _autoplaySeedItems,
    );
    if (baseTracks.isEmpty) {
      return const <Track>[];
    }
    final shuffledBase = List<MediaItem>.of(baseTracks)..shuffle();
    final sampledBaseTracks = shuffledBase
        .whereType<Track>()
        .take(math.min(_radioBaseSampleSize, shuffledBase.length))
        .toList(growable: false);
    final candidates = <Track>[];
    for (final baseTrack in sampledBaseTracks) {
      var similar = await client.getSimilarTracks(
        baseTrack.itemId,
        baseTrack.provider,
        limit: _autoplayBatchSize,
        allowLookup: false,
      );
      if (similar.isEmpty) {
        similar = await client.getSimilarTracks(
          baseTrack.itemId,
          baseTrack.provider,
          limit: _autoplayBatchSize,
          allowLookup: true,
        );
      }
      candidates.addAll(similar);
    }
    final excludedUris = <String>{
      for (final item in _queueState.items)
        if (item.mediaItem?.uri case final String uri when uri.isNotEmpty) uri,
      for (final track in sampledBaseTracks)
        if (track.uri case final String uri when uri.isNotEmpty) uri,
    };
    return _dedupeTracks(candidates)
        .where((track) => track.duration <= _radioMaxTrackDurationSeconds)
        .where(
          (track) => track.uri == null || !excludedUris.contains(track.uri),
        )
        .take(_autoplayBatchSize)
        .toList(growable: false);
  }

  void _resetAutoplaySeeds(List<MediaItem> mediaItems) {
    _autoplaySeedItems.clear();
    _appendAutoplaySeeds(mediaItems);
  }

  void _appendAutoplaySeeds(List<MediaItem> mediaItems) {
    for (final item in mediaItems) {
      if (!_canSeedAutoplay(item)) {
        continue;
      }
      _autoplaySeedItems.add(item);
      while (_autoplaySeedItems.length > _autoplayMaxSeedItems) {
        _autoplaySeedItems.removeAt(0);
      }
    }
  }

  bool _canSeedAutoplay(MediaItem item) {
    return switch (item.mediaType) {
      MediaType.track ||
      MediaType.album ||
      MediaType.playlist ||
      MediaType.artist => true,
      _ => false,
    };
  }

  String? _seedUri(MediaItem item) {
    final uri = item.uri?.trim();
    if (uri != null && uri.isNotEmpty) {
      return uri;
    }
    final provider = item.provider.trim();
    final itemId = item.itemId.trim();
    if (provider.isEmpty || itemId.isEmpty) {
      return null;
    }
    return '$provider://${item.mediaType.name}/$itemId';
  }

  List<Track> _dedupeTracks(List<Track> tracks) {
    final seen = <String>{};
    final result = <Track>[];
    for (final track in tracks) {
      final key = _trackKey(track);
      if (seen.add(key)) {
        result.add(track);
      }
    }
    return result;
  }

  String _trackKey(Track track) {
    return (track.uri?.trim().isNotEmpty ?? false)
        ? track.uri!.trim()
        : '${track.provider}:${track.itemId}';
  }

  Future<void> _ensureStateSubscription() async {
    if (_stateSubscription != null) {
      return;
    }
    _stateSubscription = transportBackend.playerStates.listen(_handleState);
  }

  void _handleState(SendspinPlayerState state) {
    final previousCurrentIndex = _queueState.currentIndex;
    _playbackState = state.toPlaybackState();
    _queueState = PlaybackQueue(
      queueId: 'local-sendspin',
      items: state.queue,
      currentIndex: state.currentIndex,
      autoplayEnabled: _autoplayEnabled,
    );
    if (!_playbackStateController.isClosed) {
      _playbackStateController.add(_playbackState);
    }
    if (!_queueStateController.isClosed) {
      _queueStateController.add(_queueState);
    }
    if (state.currentIndex != null &&
        state.currentIndex != previousCurrentIndex) {
      unawaited(_maybeRefillAutoplay());
    }
  }
}
