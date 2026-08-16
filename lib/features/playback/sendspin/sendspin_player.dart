import 'dart:async';
import 'dart:developer' as developer;
import 'dart:math' as math;

import '../../../app/config/app_config.dart';
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

  final SendspinRustBackend transportBackend;
  final MusicAssistantClient? Function() clientResolver;
  final MusicAssistantEventHub? Function() eventHubResolver;

  final StreamController<PlaybackQueueSnapshot> _queueStateController =
      StreamController<PlaybackQueueSnapshot>.broadcast();
  PlaybackQueueSnapshot _queueState = const PlaybackQueueSnapshot();
  StreamSubscription<MusicAssistantQueueEvent>? _queueEventSubscription;

  @override
  PlaybackOutputMode get outputMode => transportBackend.outputMode;

  @override
  Stream<PlaybackState> get states => transportBackend.states;

  @override
  Stream<PlaybackQueueSnapshot> get queueStates => _queueStateController.stream;

  // Connection and Activation
  @override
  Future<void> activate({String? targetPlayerId}) async {
    await transportBackend.activate(targetPlayerId: targetPlayerId);
    await _ensureQueueEventSubscription();
    await _refreshQueueFromServer();
  }

  @override
  Future<void> deactivate() async {
    await _queueEventSubscription?.cancel();
    _queueEventSubscription = null;
    await transportBackend.deactivate();
  }

  @override
  Future<void> connect() async {
    await transportBackend.connect();
    await _ensureQueueEventSubscription();
    await _refreshQueueFromServer();
  }

  Future<void> reconnect() => transportBackend.reconnect();

  Future<void> disconnect({String reason = 'client_disconnect'}) =>
      transportBackend.disconnect(reason: reason);

  @override
  Future<void> terminate() async {
    await _queueEventSubscription?.cancel();
    _queueEventSubscription = null;
    if (!_queueStateController.isClosed) {
      await _queueStateController.close();
    }
    await transportBackend.terminate();
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
  Future<void> transportSeek(double seconds) =>
      transportBackend.transportSeek(seconds);

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
  Future<PlaybackState> getState() => transportBackend.getState();

  @override
  Future<PlaybackQueueSnapshot> getQueueState() async {
    return _queueState;
  }

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

    developer.log(
      'Sendspin queue id resolution failed: queueState.queueId and AppConfig.maAppId are empty.',
      name: 'SendspinPlayer',
      level: 1000,
    );
    throw StateError('Keine Sendspin QueueId verfuegbar.');
  }

  // Subscribes to Music Assistant queue events.
  Future<void> _ensureQueueEventSubscription() async {
    final hub = eventHubResolver();
    if (hub == null) {
      return;
    }

    if (!hub.isConnected) {
      await hub.connect();
    }

    await _queueEventSubscription?.cancel();
    _queueEventSubscription = hub.queueEvents.listen((event) {
      unawaited(_onQueueEvent(event));
    });
  }

  // Applies queue changes from MA event stream.
  // Strategy:
  // - ignore unrelated queue ids
  // - apply lightweight fields directly from event payload
  // - refresh full item list only when queue_items_updated arrives
  Future<void> _onQueueEvent(MusicAssistantQueueEvent event) async {
    final String expectedQueueId = (_queueState.queueId ?? AppConfig.maAppId).trim();
    final String eventQueueId = (event.queueId ?? event.queue?.queueId ?? '').trim();

    if (expectedQueueId.isNotEmpty && eventQueueId.isNotEmpty && eventQueueId != expectedQueueId) {
      return;
    }

    switch (event.event) {
      case 'queue_added':
      case 'queue_updated':
        final PlayerQueue? queue = event.queue;
        if (queue != null) {
          _queueState = _queueState.copyWith(
            queueId: queue.queueId.isNotEmpty
                ? queue.queueId
                : (eventQueueId.isNotEmpty ? eventQueueId : _queueState.queueId),
            currentIndex: queue.currentIndex,
            clearCurrentIndex: queue.currentIndex == null,
            shuffleEnabled: queue.shuffleEnabled,
            repeatMode: queue.repeatMode?.name,
            dontStopTheMusicEnabled: queue.dontStopTheMusicEnabled,
          );
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
        final raw = event.rawData ?? const <String, dynamic>{};
        _queueState = _queueState.copyWith(
          queueId: eventQueueId.isNotEmpty ? eventQueueId : _queueState.queueId,
          shuffleEnabled: raw['shuffle_enabled'] as bool? ?? _queueState.shuffleEnabled,
          repeatMode: (raw['repeat_mode'] as String?)?.trim() ?? _queueState.repeatMode,
          dontStopTheMusicEnabled: raw['dont_stop_the_music_enabled'] as bool? ??
              _queueState.dontStopTheMusicEnabled,
        );
        _emitQueueState();
        return;
      case 'queue_removed':
        _queueState = _queueState.copyWith(
          queueId: eventQueueId.isNotEmpty ? eventQueueId : _queueState.queueId,
          items: const <QueueItem>[],
          clearCurrentIndex: true,
        );
        _emitQueueState();
        return;
      case 'queue_time_updated':
        // Position/progress is handled by playback state stream.
      default:
        return;
    }
  }

  // Initial/full queue refresh used during activate/connect.
  Future<void> _refreshQueueFromServer() async {
    final client = clientResolver();
    if (client == null) {
      return;
    }

    final String queueId = await _resolveQueueId();
    final PlayerQueue? queue = await client.getPlayerQueue(queueId);
    if (queue == null) {
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
      repeatMode: queue.repeatMode?.name,
      dontStopTheMusicEnabled: queue.dontStopTheMusicEnabled,
    );
    _emitQueueState();
  }

  // Targeted item refresh for queue_items_updated.
  Future<void> _refreshQueueItemsFromServer(String queueId) async {
    final client = clientResolver();
    if (client == null) {
      return;
    }

    final String normalizedQueueId = queueId.trim();
    if (normalizedQueueId.isEmpty) {
      return;
    }

    final int itemCount = _queueState.items.length;
    final int limit = itemCount <= 0 ? 500 : math.max(100, itemCount);
    final List<QueueItem> items = await client.getQueueItems(normalizedQueueId, limit: limit);

    final String activeQueueId = (_queueState.queueId ?? AppConfig.maAppId).trim();
    if (activeQueueId.isNotEmpty && normalizedQueueId != activeQueueId) {
      return;
    }

    _queueState = _queueState.copyWith(
      queueId: normalizedQueueId,
      items: items,
    );
    _emitQueueState();
  }

  // Emits the current queue snapshot to the coordinator.
  void _emitQueueState() {
    if (_queueStateController.isClosed) {
      return;
    }
    _queueStateController.add(_queueState);
  }
}
