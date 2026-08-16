import 'dart:async';
import 'dart:math' as math;

import '../../../data/music_assistant/ma_models.dart';
import '../../../data/music_assistant/music_assistant_client.dart';
import '../playback_models.dart';
import '../playback_player.dart';
import 'sendspin_rust_backend.dart';

class SendspinPlayer implements PlaybackPlayer {
  SendspinPlayer({
    required this.transportBackend,
    required this.clientResolver,
  });

  final SendspinRustBackend transportBackend;
  final MusicAssistantClient? Function() clientResolver;

  final StreamController<PlaybackQueueSnapshot> _queueStateController =
      StreamController<PlaybackQueueSnapshot>.broadcast();
  PlaybackQueueSnapshot _queueState = const PlaybackQueueSnapshot();

  @override
  PlaybackOutputMode get outputMode => transportBackend.outputMode;

  @override
  Stream<PlaybackState> get states => transportBackend.states;

  @override
  Stream<PlaybackQueueSnapshot> get queueStates => _queueStateController.stream;

  // Connection and Activation
  @override
  Future<void> activate({String? targetPlayerId}) =>
      transportBackend.activate(targetPlayerId: targetPlayerId);

  @override
  Future<void> deactivate() => transportBackend.deactivate();

  @override
  Future<void> connect() => transportBackend.connect();

  Future<void> reconnect() => transportBackend.reconnect();

  Future<void> disconnect({String reason = 'client_disconnect'}) =>
      transportBackend.disconnect(reason: reason);

  @override
  Future<void> terminate() async {
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
  Future<void> playMedia(
    List<MediaItem> mediaItems, {
    PlaybackQueueInsertOption option = PlaybackQueueInsertOption.replace,
    bool radioMode = false,
    dynamic startItem,
    String sortBy = 'original',
  }) async {
    final client = clientResolver();
    if (client == null || mediaItems.isEmpty) {
      return;
    }

    final String queueId = await _resolveQueueId(client);
    await client.playMedia(
      queueId,
      mediaItems,
      option: _toMaQueueOption(option),
      radioMode: radioMode,
      startItem: startItem,
      sortBy: sortBy,
    );

    _projectPlayMedia(queueId, mediaItems, option: option);
  }

  @override
  Future<void> clearQueue({bool skipStop = false}) async {
    final client = clientResolver();
    if (client == null) {
      return;
    }

    final String queueId = await _resolveQueueId(client);
    await client.clearQueue(queueId, skipStop: skipStop);
    _queueState = _queueState.copyWith(
      queueId: queueId,
      items: const <Map<String, dynamic>>[],
      clearCurrentIndex: true,
    );
    _emitQueueState();
  }

  @override
  Future<void> playQueueIndex(int index) async {
    final client = clientResolver();
    if (client == null) {
      return;
    }

    final String queueId = await _resolveQueueId(client);
    await client.playQueueIndex(queueId, index);
    if (_queueState.items.isEmpty) {
      _queueState = _queueState.copyWith(queueId: queueId, clearCurrentIndex: true);
    } else {
      _queueState = _queueState.copyWith(
        queueId: queueId,
        currentIndex: math.max(0, math.min(index, _queueState.items.length - 1)),
      );
    }
    _emitQueueState();
  }

  @override
  Future<void> moveQueueItem(String queueItemId, {int posShift = 0}) async {
    final client = clientResolver();
    if (client == null || queueItemId.trim().isEmpty) {
      return;
    }

    final String queueId = await _resolveQueueId(client);
    await client.moveQueueItem(queueId, queueItemId.trim(), posShift: posShift);
    _projectMoveQueueItem(queueId, queueItemId.trim(), posShift);
  }

  @override
  Future<void> deleteQueueItem(String queueItemId) async {
    final client = clientResolver();
    if (client == null || queueItemId.trim().isEmpty) {
      return;
    }

    final String queueId = await _resolveQueueId(client);
    await client.deleteQueueItemById(queueId, queueItemId.trim());
    _projectDeleteQueueItem(queueId, queueItemId.trim());
  }

  @override
  Future<void> setShuffle(bool enabled) async {
    final client = clientResolver();
    if (client == null) {
      return;
    }

    final String queueId = await _resolveQueueId(client);
    await client.setShuffle(queueId, enabled);
    _queueState = _queueState.copyWith(
      queueId: queueId,
      shuffleEnabled: enabled,
    );
    _emitQueueState();
  }

  @override
  Future<void> setRepeatMode(String repeatMode) async {
    final client = clientResolver();
    if (client == null) {
      return;
    }

    final String queueId = await _resolveQueueId(client);
    final String normalized = _normalizeRepeatMode(repeatMode);
    await client.setRepeat(queueId, _toMaRepeatMode(normalized));
    _queueState = _queueState.copyWith(
      queueId: queueId,
      repeatMode: normalized,
    );
    _emitQueueState();
  }

  @override
  Future<void> setDontStopTheMusic(bool enabled) async {
    final client = clientResolver();
    if (client == null) {
      return;
    }

    final String queueId = await _resolveQueueId(client);
    await client.setDontStopTheMusic(queueId, enabled);
    _queueState = _queueState.copyWith(
      queueId: queueId,
      dontStopTheMusicEnabled: enabled,
    );
    _emitQueueState();
  }

  @override
  Future<PlaybackState> getState() => transportBackend.getState();

  @override
  Future<PlaybackQueueSnapshot> getQueueState() async {
    return _queueState;
  }

  Future<String> _resolveQueueId(MusicAssistantClient client) async {
    if ((_queueState.queueId ?? '').trim().isNotEmpty) {
      return _queueState.queueId!.trim();
    }

    final List<PlayerQueue> queues = await client.getPlayerQueues();
    final List<PlayerQueue> valid =
        queues.where((q) => q.queueId.trim().isNotEmpty).toList(growable: false);
    if (valid.isEmpty) {
      throw StateError('Keine Player Queue verfuegbar.');
    }

    final PlayerQueue? active =
        valid.where((q) => q.active).cast<PlayerQueue?>().firstWhere(
              (q) => q != null,
              orElse: () => null,
            );
    final PlayerQueue? available =
        valid.where((q) => q.available).cast<PlayerQueue?>().firstWhere(
              (q) => q != null,
              orElse: () => null,
            );

    final String queueId = (active ?? available ?? valid.first).queueId.trim();
    _queueState = _queueState.copyWith(queueId: queueId);
    _emitQueueState();
    return queueId;
  }

  QueueOption _toMaQueueOption(PlaybackQueueInsertOption option) {
    switch (option) {
      case PlaybackQueueInsertOption.replace:
        return QueueOption.replace;
      case PlaybackQueueInsertOption.next:
        return QueueOption.next;
      case PlaybackQueueInsertOption.replaceNext:
        return QueueOption.replaceNext;
      case PlaybackQueueInsertOption.last:
        return QueueOption.add;
    }
  }

  RepeatMode _toMaRepeatMode(String raw) {
    switch (raw.trim().toLowerCase()) {
      case 'all':
        return RepeatMode.all;
      case 'one':
        return RepeatMode.one;
      default:
        return RepeatMode.off;
    }
  }

  String _normalizeRepeatMode(String raw) {
    switch (raw.trim().toLowerCase()) {
      case 'all':
        return 'all';
      case 'one':
        return 'one';
      default:
        return 'off';
    }
  }

  void _projectPlayMedia(
    String queueId,
    List<MediaItem> mediaItems, {
    required PlaybackQueueInsertOption option,
  }) {
    final List<Map<String, dynamic>> current = _queueState.items
        .map((e) => Map<String, dynamic>.from(e))
        .toList(growable: true);
    final List<Map<String, dynamic>> payload =
        mediaItems.map(_toQueueProjection).toList(growable: false);
    int? currentIndex = _queueState.currentIndex;

    switch (option) {
      case PlaybackQueueInsertOption.replace:
        current
          ..clear()
          ..addAll(payload);
        currentIndex = current.isEmpty ? null : 0;
        break;
      case PlaybackQueueInsertOption.next:
        final int insertIndex =
            currentIndex == null ? current.length : math.min(currentIndex + 1, current.length);
        current.insertAll(insertIndex, payload);
        currentIndex ??= current.isEmpty ? null : 0;
        break;
      case PlaybackQueueInsertOption.replaceNext:
        if (currentIndex == null) {
          current
            ..clear()
            ..addAll(payload);
          currentIndex = current.isEmpty ? null : 0;
        } else {
          final int keepCount = math.min(currentIndex + 1, current.length);
          final List<Map<String, dynamic>> prefix =
              current.take(keepCount).toList(growable: false);
          current
            ..clear()
            ..addAll(prefix)
            ..addAll(payload);
        }
        break;
      case PlaybackQueueInsertOption.last:
        current.addAll(payload);
        currentIndex ??= current.isEmpty ? null : 0;
        break;
    }

    _queueState = _queueState.copyWith(
      queueId: queueId,
      items: current,
      currentIndex: currentIndex,
      clearCurrentIndex: currentIndex == null,
    );
    _emitQueueState();
  }

  void _projectMoveQueueItem(String queueId, String queueItemId, int posShift) {
    final List<Map<String, dynamic>> current = _queueState.items
        .map((e) => Map<String, dynamic>.from(e))
        .toList(growable: true);
    final int fromIndex = _indexOfQueueItem(current, queueItemId);
    if (fromIndex < 0 || posShift == 0) {
      return;
    }

    final int toIndex = math.max(0, math.min(fromIndex + posShift, current.length - 1));
    if (fromIndex == toIndex) {
      return;
    }

    final moved = current.removeAt(fromIndex);
    current.insert(toIndex, moved);

    int? currentIndex = _queueState.currentIndex;
    if (currentIndex != null) {
      if (currentIndex == fromIndex) {
        currentIndex = toIndex;
      } else if (fromIndex < currentIndex && toIndex >= currentIndex) {
        currentIndex = currentIndex - 1;
      } else if (fromIndex > currentIndex && toIndex <= currentIndex) {
        currentIndex = currentIndex + 1;
      }
    }

    _queueState = _queueState.copyWith(
      queueId: queueId,
      items: current,
      currentIndex: currentIndex,
      clearCurrentIndex: currentIndex == null,
    );
    _emitQueueState();
  }

  void _projectDeleteQueueItem(String queueId, String queueItemId) {
    final List<Map<String, dynamic>> current = _queueState.items
        .map((e) => Map<String, dynamic>.from(e))
        .toList(growable: true);
    final int index = _indexOfQueueItem(current, queueItemId);
    if (index < 0) {
      return;
    }

    current.removeAt(index);
    int? currentIndex = _queueState.currentIndex;
    if (current.isEmpty) {
      currentIndex = null;
    } else if (currentIndex != null) {
      if (index < currentIndex) {
        currentIndex = currentIndex - 1;
      } else if (index == currentIndex) {
        currentIndex = math.min(currentIndex, current.length - 1);
      }
    }

    _queueState = _queueState.copyWith(
      queueId: queueId,
      items: current,
      currentIndex: currentIndex,
      clearCurrentIndex: currentIndex == null,
    );
    _emitQueueState();
  }

  int _indexOfQueueItem(List<Map<String, dynamic>> items, String queueItemId) {
    final String needle = queueItemId.trim();
    for (int i = 0; i < items.length; i++) {
      final dynamic value = items[i]['queue_item_id'] ?? items[i]['item_id'];
      if (value is String && value.trim() == needle) {
        return i;
      }
    }
    return -1;
  }

  Map<String, dynamic> _toQueueProjection(MediaItem item) {
    return <String, dynamic>{
      'item_id': item.itemId,
      'provider': item.provider,
      'name': item.name,
      'media_type': mediaTypeToJson(item.mediaType),
      'uri': item.uri,
    };
  }

  void _emitQueueState() {
    if (_queueStateController.isClosed) {
      return;
    }
    _queueStateController.add(_queueState);
  }
}
