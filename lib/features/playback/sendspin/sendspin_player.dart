import 'dart:async';
import 'dart:math' as math;

import '../../../app/config/app_config.dart';
import '../../../app/logging/app_logger.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../../../data/music_assistant/ma_client.dart';
import '../../../data/music_assistant/ma_event_hub.dart';
import '../playback_models.dart';
import '../playback_player.dart';
import 'sendspin_rust_backend.dart';

class SendspinPlayer implements PlaybackPlayer {
  SendspinPlayer({
    required this.transportBackend,
    required this.clientResolver,
    required this.eventHubResolver,
  });

  static const Duration _positionTickInterval = Duration(milliseconds: 250);
  static const String _logName = 'SendspinPlayer';

  final SendspinRustBackend transportBackend;
  final MusicAssistantClient? Function() clientResolver;
  final MusicAssistantEventHub? Function() eventHubResolver;

  final StreamController<PlaybackQueueSnapshot> _queueStateController =
      StreamController<PlaybackQueueSnapshot>.broadcast();
  final StreamController<PlaybackState> _stateController =
      StreamController<PlaybackState>.broadcast();

  PlaybackState _playbackState = const PlaybackState();
  PlaybackQueueSnapshot _queueState = const PlaybackQueueSnapshot();
  StreamSubscription<PlaybackState>? _transportStateSubscription;
  StreamSubscription<MusicAssistantQueueEvent>? _queueEventSubscription;
  Timer? _positionInterpolationTimer;
  int? _positionAnchorMillis;
  DateTime? _positionAnchorUpdatedAt;
  Future<void> _queueEventProcessing = Future<void>.value();
  final AppLogger _logger = AppLogger(_logName);

  @override
  PlaybackOutputMode get outputMode => transportBackend.outputMode;

  @override
  Stream<PlaybackState> get states => _stateController.stream;

  @override
  Stream<PlaybackQueueSnapshot> get queueStates => _queueStateController.stream;

  // Connection and Activation
  @override
  Future<void> activate({String? targetPlayerId}) async {
    _logger.info('Sendspin activate requested. TargetPlayerId=${targetPlayerId ?? ''}');
    await _ensureStateSubscription();
    await transportBackend.activate(targetPlayerId: targetPlayerId);
    await _ensureQueueEventSubscription();
    await getState();
    await _refreshQueueFromServer();
    _logger.info('Sendspin activate completed. QueueId=${_queueState.queueId ?? ''}');
  }

  @override
  Future<void> deactivate() async {
    _logger.info('Sendspin deactivate requested.');
    _positionInterpolationTimer?.cancel();
    _positionInterpolationTimer = null;
    await _transportStateSubscription?.cancel();
    _transportStateSubscription = null;
    await _queueEventSubscription?.cancel();
    _queueEventSubscription = null;
    await transportBackend.deactivate();
    _logger.info('Sendspin deactivate completed.');
  }

  @override
  Future<void> connect() async {
    _logger.info('Sendspin connect requested.');
    await _ensureStateSubscription();
    await transportBackend.connect();
    await _ensureQueueEventSubscription();
    await getState();
    await _refreshQueueFromServer();
    _logger.info('Sendspin connect completed. QueueId=${_queueState.queueId ?? ''}');
  }

  Future<void> reconnect() => transportBackend.reconnect();

  Future<void> disconnect({String reason = 'client_disconnect'}) =>
      transportBackend.disconnect(reason: reason);

  @override
  Future<void> terminate() async {
    _logger.info('Sendspin terminate requested.');
    _positionInterpolationTimer?.cancel();
    _positionInterpolationTimer = null;
    await _transportStateSubscription?.cancel();
    _transportStateSubscription = null;
    await _queueEventSubscription?.cancel();
    _queueEventSubscription = null;
    if (!_stateController.isClosed) {
      await _stateController.close();
    }
    if (!_queueStateController.isClosed) {
      await _queueStateController.close();
    }
    await transportBackend.terminate();
    _logger.info('Sendspin terminate completed.');
  }

  // Transport Controls
  @override
  Future<void> transportPlay() => transportBackend.transportPlay();

  @override
  Future<void> transportPause() => transportBackend.transportPause();

  @override
  Future<void> transportNext() => transportBackend.transportNext();

  @override
  Future<void> transportPrevious() => transportBackend.transportPrevious();

  @override
  Future<void> transportSeek(double seconds) async {
    final client = clientResolver();
    if (client == null) {
      _logger.warn('Sendspin seek skipped because MusicAssistant client is null.');
      return;
    }

    final int position = seconds.isNaN || seconds.isInfinite
        ? 0
        : math.max(0, seconds.round());
    final String playerId = AppConfig.maAppId.trim();
    if (playerId.isEmpty) {
      _logger.warn('Sendspin seek skipped because player id is empty.');
      return;
    }

    _playbackState = _playbackState.copyWith(
      playbackState: PlaybackStateType.seeking,
      positionMillis: position * 1000,
    );
    _emitPlaybackState();

    _logger.info('Sendspin seek requested via MusicAssistant. PlayerId=$playerId, Seconds=$position');
    await client.playerSeek(playerId, position);
  }

  @override
  Future<void> setVolume(int volume) => transportBackend.setVolume(volume);

  @override
  Future<void> setMuted(bool muted) => transportBackend.setMuted(muted);

  // Play Media and Queue Management
  @override
  Future<void> playMedia(List<MediaItem> mediaItems) async {
    final client = clientResolver();
    if (client == null || mediaItems.isEmpty) {
      return;
    }

    final String queueId = await _resolveQueueId();
    await client.playMedia(
      queueId,
      mediaItems,
      option: QueueOption.replace,
      radioMode: false,
      sortBy: 'original',
    );
  }

  @override
  Future<void> playMediaNext(List<MediaItem> mediaItems) async {
    final client = clientResolver();
    if (client == null || mediaItems.isEmpty) {
      return;
    }

    final String queueId = await _resolveQueueId();
    await client.playMedia(
      queueId,
      mediaItems,
      option: QueueOption.next,
      radioMode: false,
      sortBy: 'original',
    );
  }

  @override
  Future<void> playMediaReplaceNext(List<MediaItem> mediaItems) async {
    final client = clientResolver();
    if (client == null || mediaItems.isEmpty) {
      return;
    }

    final String queueId = await _resolveQueueId();
    await client.playMedia(
      queueId,
      mediaItems,
      option: QueueOption.replaceNext,
      radioMode: false,
      sortBy: 'original',
    );
  }

  @override
  Future<void> playMediaLast(List<MediaItem> mediaItems) async {
    final client = clientResolver();
    if (client == null || mediaItems.isEmpty) {
      return;
    }

    final String queueId = await _resolveQueueId();
    await client.playMedia(
      queueId,
      mediaItems,
      option: QueueOption.add,
      radioMode: false,
      sortBy: 'original',
    );
  }

  @override
  Future<void> playMediaRadioNext(List<MediaItem> mediaItems) async {
    if (mediaItems.isEmpty) {
      return;
    }

    final client = clientResolver();
    if (client == null) {
      return;
    }

    final String queueId = await _resolveQueueId();
    await client.playMedia(
      queueId,
      mediaItems,
      option: QueueOption.next,
      radioMode: true,
      startItem: mediaItems.first,
      sortBy: 'original',
    );
  }

  @override
  Future<void> shufflePlayMedia(List<MediaItem> mediaItems) async {
    if (mediaItems.isEmpty) {
      return;
    }

    final client = clientResolver();
    if (client == null) {
      return;
    }

    final shuffled = mediaItems.toList(growable: true);
    for (int i = shuffled.length - 1; i > 0; i--) {
      final int j = math.Random().nextInt(i + 1);
      final MediaItem temp = shuffled[i];
      shuffled[i] = shuffled[j];
      shuffled[j] = temp;
    }

    final String queueId = await _resolveQueueId();
    await client.playMedia(
      queueId,
      shuffled,
      option: QueueOption.replace,
      radioMode: false,
      sortBy: 'original',
    );
  }

  @override
  Future<void> clearQueue({bool skipStop = false}) async {
    final client = clientResolver();
    if (client == null) {
      return;
    }

    final String queueId = await _resolveQueueId();
    await client.clearQueue(queueId, skipStop: skipStop);
  }

  @override
  Future<void> playQueueIndex(int index) async {
    final client = clientResolver();
    if (client == null) {
      return;
    }

    final String queueId = await _resolveQueueId();
    await client.playQueueIndex(queueId, index);
  }

  @override
  Future<void> moveQueueItem(String queueItemId, {int posShift = 0}) async {
    final client = clientResolver();
    if (client == null || queueItemId.trim().isEmpty) {
      return;
    }

    final String queueId = await _resolveQueueId();
    await client.moveQueueItem(queueId, queueItemId.trim(), posShift: posShift);
  }

  @override
  Future<void> deleteQueueItem(String queueItemId) async {
    final client = clientResolver();
    if (client == null || queueItemId.trim().isEmpty) {
      return;
    }

    final String queueId = await _resolveQueueId();
    await client.deleteQueueItemById(queueId, queueItemId.trim());
  }

  @override
  Future<void> setShuffle(bool enabled) async {
    final client = clientResolver();
    if (client == null) {
      return;
    }

    final String queueId = await _resolveQueueId();
    await client.setShuffle(queueId, enabled);
  }

  @override
  Future<void> setRepeatMode(RepeatMode repeatMode) async {
    final client = clientResolver();
    if (client == null) {
      return;
    }

    final String queueId = await _resolveQueueId();
    await client.setRepeat(queueId, repeatMode);
  }

  @override
  Future<void> setDontStopTheMusic(bool enabled) async {
    final client = clientResolver();
    if (client == null) {
      return;
    }

    final String queueId = await _resolveQueueId();
    await client.setDontStopTheMusic(queueId, enabled);
  }

  @override
  Future<PlaybackState> getState() async {
    await _ensureStateSubscription();
    final PlaybackState next = await transportBackend.getState();
    _applyPlaybackState(next, emit: true);
    _logger.debug(
      'Sendspin getState applied. State=${_playbackState.playbackState.name}, PositionMillis=${_playbackState.positionMillis}, DurationMillis=${_playbackState.durationMillis}',
    );
    return _playbackState;
  }

  @override
  Future<PlaybackQueueSnapshot> getQueueState() async {
    return _queueState;
  }

  // Playback state stream subscription.
  Future<void> _ensureStateSubscription() async {
    if (_transportStateSubscription != null) {
      return;
    }

    _logger.debug('Sendspin state subscription initialized.');

    _transportStateSubscription = transportBackend.states.listen(
      (PlaybackState next) {
        _applyPlaybackState(next, emit: true);
      },
      onError: (Object error, StackTrace stackTrace) {
        _logger.error(
          'Sendspin state stream error. Error=$error',
          error: error,
          stackTrace: stackTrace,
        );
      },
    );
  }

  // Apply playback state
  // - update playback flags (state/volume/mute/repeat/shuffle)
  // - keep duration from metadata
  // - do not overwrite position anchor from this stream
  void _applyPlaybackState(PlaybackState next, {required bool emit}) {
    final PlaybackState previous = _playbackState;
    _playbackState = _playbackState.copyWith(
      playbackState: next.playbackState,
      volume: next.volume,
      muted: next.muted,
      repeatMode: next.repeatMode,
      clearRepeatMode: next.repeatMode == null,
      shuffleEnabled: next.shuffleEnabled,
      clearShuffleEnabled: next.shuffleEnabled == null,
      seekMaxMillis: next.seekMaxMillis,
      clearSeekMaxMillis: next.seekMaxMillis == null,
      durationMillis: next.durationMillis,
      clearDurationMillis: next.durationMillis == null,
    );

    if (next.playbackState == PlaybackStateType.playing) {
      // Only interpolate position when playing.
      _positionAnchorMillis ??= _playbackState.positionMillis;
      _positionAnchorUpdatedAt ??= DateTime.now();
      _startPositionInterpolation();
    } else {
      // Reset anchor when not playing.
      _resetPositionAnchor();
      if (_positionInterpolationTimer != null) {
        _logger.debug('Sendspin interpolation stopped. State=${next.playbackState.name}');
      }
      _positionInterpolationTimer?.cancel();
      _positionInterpolationTimer = null;
    }

    if (previous.playbackState != next.playbackState
        || previous.positionMillis != _playbackState.positionMillis
        || previous.durationMillis != next.durationMillis) {
      _logger.debug(
        'Sendspin playback state applied. State=${next.playbackState.name}, PositionMillis=${_playbackState.positionMillis}, DurationMillis=${next.durationMillis}',
      );
    }

    if (emit) {
      _emitPlaybackState();
    }
  }

  void _resetPositionAnchor() {
    _positionAnchorMillis = _playbackState.positionMillis;
    _positionAnchorUpdatedAt = null;
  }

  // Position interpolation.
  // - run only while playing
  // - derive from anchor + elapsed wall time
  // - clamp to track duration if known
  void _startPositionInterpolation() {
    if (_positionInterpolationTimer != null) {
      return;
    }

    _logger.debug(
      'Sendspin interpolation started. TickMs=${_positionTickInterval.inMilliseconds}, AnchorMillis=$_positionAnchorMillis',
    );

    _positionInterpolationTimer = Timer.periodic(_positionTickInterval, (_) {
      if (_playbackState.playbackState != PlaybackStateType.playing) {
        _logger.debug('Sendspin interpolation canceled because state is ${_playbackState.playbackState.name}.');
        _positionInterpolationTimer?.cancel();
        _positionInterpolationTimer = null;
        return;
      }

      final int? anchorMillis = _positionAnchorMillis;
      final DateTime? anchorUpdatedAt = _positionAnchorUpdatedAt;
      if (anchorMillis == null || anchorUpdatedAt == null) {
        return;
      }

      final int elapsedMillis = DateTime.now().difference(anchorUpdatedAt).inMilliseconds;
      var nextPosition = anchorMillis + elapsedMillis;

      final int? durationMillis = _playbackState.durationMillis;
      if (durationMillis != null && durationMillis > 0) {
        nextPosition = nextPosition.clamp(0, durationMillis);
      }

      if (nextPosition == _playbackState.positionMillis) {
        return;
      }

      _playbackState = _playbackState.copyWith(positionMillis: nextPosition);
      _emitPlaybackState();
    });
  }

  // Emit playback state to the coordinator
  void _emitPlaybackState() {
    if (_stateController.isClosed) {
      return;
    }
    _stateController.add(_playbackState);
  }

  // Queue ID Helper
  Future<String> _resolveQueueId() async {
    if ((_queueState.queueId ?? '').trim().isNotEmpty) {
      return _queueState.queueId!.trim();
    }

    final playerId = AppConfig.maAppId.trim();
    if (playerId.isNotEmpty) {
      _queueState = _queueState.copyWith(queueId: playerId);
      _emitQueueState();
      return playerId;
    }

    _logger.error(
      'Sendspin queue id resolution failed. QueueStateQueueId=${_queueState.queueId ?? ''}, AppConfigQueueId=${AppConfig.maAppId}',
    );
    throw StateError('Keine Sendspin QueueId verfuegbar.');
  }

  // Queue event subscription
  Future<void> _ensureQueueEventSubscription() async {
    final hub = eventHubResolver();
    if (hub == null) {
      _logger.warn('Queue event subscription skipped because event hub resolver returned null.');
      return;
    }

    if (!hub.isConnected) {
      _logger.debug('Event hub is disconnected, connecting before queue subscription.');
      await hub.connect();
    }

    await _queueEventSubscription?.cancel();
    _queueEventSubscription = hub.queueEvents.listen((event) {
      _queueEventProcessing = _queueEventProcessing.then((_) => _onQueueEvent(event)).catchError((
        Object error,
        StackTrace stackTrace,
      ) {
        _logger.error(
          'Queue event processing failed. Event=${event.event}, Error=$error',
          error: error,
          stackTrace: stackTrace,
        );
      });
    });
    _logger.debug('Queue event subscription initialized.');
  }

  // Apply queue events.
  // - ignore unrelated queue ids
  // - project queue metadata/settings
  // - take progress anchor from queue timing
  Future<void> _onQueueEvent(MusicAssistantQueueEvent event) async {
    final String expectedQueueId = (_queueState.queueId ?? AppConfig.maAppId).trim();
    final String eventQueueId = (event.queueId ?? event.queue?.queueId ?? '').trim();

    if (expectedQueueId.isNotEmpty && eventQueueId.isNotEmpty && eventQueueId != expectedQueueId) {
      _logger.debug(
        'Queue event ignored due to queue mismatch. Event=${event.event}, EventQueueId=$eventQueueId, ExpectedQueueId=$expectedQueueId',
      );
      return;
    }

    switch (event.event) {
      case 'queue_added':
      case 'queue_updated':
        final PlayerQueue? queue = event.queue;
        if (queue != null) {
          // Set Queue properties.
          _queueState = _queueState.copyWith(
            queueId: queue.queueId.isNotEmpty
                ? queue.queueId
                : (eventQueueId.isNotEmpty ? eventQueueId : _queueState.queueId),
            currentIndex: queue.currentIndex,
            clearCurrentIndex: queue.currentIndex == null,
            shuffleEnabled: queue.shuffleEnabled,
            repeatMode: queue.repeatMode,
            dontStopTheMusicEnabled: queue.dontStopTheMusicEnabled,
          );

          // Set DurationMillis from queue payload.
          _playbackState = _playbackState.copyWith(
            durationMillis: _queueDurationToMillis(queue.currentItem?.duration),
            clearDurationMillis: queue.currentItem?.duration == null,
          );

          // Set positionMillis from queue elapsed time when present.
          if (queue.elapsedTime != null) {
            _applyQueueTimingProgress(
              (queue.elapsedTime! * 1000).round(),
              source: 'queue_updated',
            );
          } else {
            _emitPlaybackState();
          }

          _emitQueueState();
        }
        return;
      case 'queue_items_updated':
        final String queueIdForItems = eventQueueId.isNotEmpty
            ? eventQueueId
            : ((_queueState.queueId ?? AppConfig.maAppId).trim());
        if (queueIdForItems.isNotEmpty) {
          await _refreshQueueItemsFromServer(queueIdForItems);
        }
        return;
      case 'queue_settings_updated':
        final settings = event.queueSettings;
        if (settings == null) {
          return;
        }

        _queueState = _queueState.copyWith(
          queueId: eventQueueId.isNotEmpty ? eventQueueId : _queueState.queueId,
          shuffleEnabled: settings.shuffleEnabled ?? _queueState.shuffleEnabled,
          repeatMode: settings.repeatMode ?? _queueState.repeatMode,
          dontStopTheMusicEnabled: settings.dontStopTheMusicEnabled ?? _queueState.dontStopTheMusicEnabled,
        );
        _emitQueueState();
        return;
      case 'queue_removed':
        _queueState = _queueState.copyWith(
          queueId: eventQueueId.isNotEmpty ? eventQueueId : _queueState.queueId,
          items: const <QueueItem>[],
          clearCurrentIndex: true,
        );
        _resetPositionAnchor();
        _playbackState = _playbackState.copyWith(
          playbackState: PlaybackStateType.idle,
          clearPositionMillis: true,
          clearDurationMillis: true,
        );
        _emitQueueState();
        _emitPlaybackState();
        return;
      case 'queue_time_updated':
        // Set position/progress from queue_time_updated.
        final int? elapsedMillis = _extractElapsedMillis(event);
        if (elapsedMillis != null) {
          _applyQueueTimingProgress(elapsedMillis, source: 'queue_time_updated');
        } else {
          _logger.debug(
            'Queue timing update ignored because elapsed time is missing. QueueId=$eventQueueId, Event=${event.event}',
          );
        }
      default:
        return;
    }
  }


  void _applyQueueTimingProgress(int positionMillis, {required String source}) {
    final int clampedPosition = positionMillis < 0 ? 0 : positionMillis;

    _positionAnchorMillis = clampedPosition;
    _positionAnchorUpdatedAt = DateTime.now();

    _playbackState = _playbackState.copyWith(positionMillis: clampedPosition);

    if (_playbackState.playbackState == PlaybackStateType.playing) {
      _startPositionInterpolation();
    }

    _emitPlaybackState();
    _logger.debug(
      'Queue timing progress applied. Source=$source, PositionMillis=$clampedPosition, DurationMillis=${_playbackState.durationMillis}',
    );
  }


  int? _extractElapsedMillis(MusicAssistantQueueEvent event) {
    final double? eventElapsedSeconds = event.elapsedTimeSeconds;
    if (eventElapsedSeconds != null) {
      return (eventElapsedSeconds * 1000).round();
    }

    return null;
  }

  int? _queueDurationToMillis(int? value) {
    if (value == null) {
      return null;
    }

    if (value <= 0) {
      return 0;
    }

    // Queue duration is usually seconds. Keep large values that already look like milliseconds.
    return value >= 100000 ? value : value * 1000;
  }

  // Full queue refresh
  Future<void> _refreshQueueFromServer() async {
    final client = clientResolver();
    if (client == null) {
      _logger.warn('Queue refresh skipped because MusicAssistant client is null.');
      return;
    }

    final String queueId = await _resolveQueueId();
    final PlayerQueue? queue = await client.getPlayerQueue(queueId);
    if (queue == null) {
      _logger.warn('Queue refresh returned null queue for QueueId=$queueId.');
      return;
    }

    final int limit = queue.itemCount <= 0 ? 500 : math.max(100, queue.itemCount);
    final List<QueueItem> items = await client.getQueueItems(queueId, limit: limit);

    _queueState = _queueState.copyWith(
      queueId: queue.queueId.isNotEmpty ? queue.queueId : queueId,
      items: items,
      currentIndex: queue.currentIndex,
      clearCurrentIndex: queue.currentIndex == null,
      shuffleEnabled: queue.shuffleEnabled,
      repeatMode: queue.repeatMode,
      dontStopTheMusicEnabled: queue.dontStopTheMusicEnabled,
    );

    // Set DurationMillis from queue payload.
    _playbackState = _playbackState.copyWith(
      durationMillis: _queueDurationToMillis(queue.currentItem?.duration),
      clearDurationMillis: queue.currentItem?.duration == null,
    );

    // Set positionMillis from queue elapsed time when present.
    if (queue.elapsedTime != null) {
      _applyQueueTimingProgress(
        (queue.elapsedTime! * 1000).round(),
        source: 'refresh_queue',
      );
    } else {
      _emitPlaybackState();
    }

    _emitQueueState();
    _logger.debug(
      'Queue refresh applied. QueueId=${_queueState.queueId ?? ''}, Items=${_queueState.items.length}, CurrentIndex=${_queueState.currentIndex}',
    );
  }

  // Targeted item refresh
  Future<void> _refreshQueueItemsFromServer(String queueId) async {
    final client = clientResolver();
    if (client == null) {
      _logger.warn('Queue item refresh skipped because MusicAssistant client is null.');
      return;
    }

    final String normalizedQueueId = queueId.trim();
    if (normalizedQueueId.isEmpty) {
      _logger.warn('Queue item refresh skipped because queueId is empty.');
      return;
    }

    final int itemCount = _queueState.items.length;
    final int limit = itemCount <= 0 ? 500 : math.max(100, itemCount);
    final List<QueueItem> items = await client.getQueueItems(normalizedQueueId, limit: limit);

    final String activeQueueId = (_queueState.queueId ?? AppConfig.maAppId).trim();
    if (activeQueueId.isNotEmpty && normalizedQueueId != activeQueueId) {
      _logger.debug(
        'Queue item refresh ignored due to queue mismatch. IncomingQueueId=$normalizedQueueId, ActiveQueueId=$activeQueueId',
      );
      return;
    }

    _queueState = _queueState.copyWith(
      queueId: normalizedQueueId,
      items: items,
    );
    _emitQueueState();
    _logger.debug('Queue items refresh applied. QueueId=$normalizedQueueId, Items=${items.length}');
  }

  // Emits the current queue snapshot to the coordinator.
  void _emitQueueState() {
    if (_queueStateController.isClosed) {
      return;
    }
    _queueStateController.add(_queueState);
  }
}
