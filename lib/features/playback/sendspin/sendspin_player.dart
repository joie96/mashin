import 'dart:async';
import 'dart:math' as math;

import '../../../data/music_assistant/ma_models.dart';
import '../../../data/music_assistant/music_assistant_client.dart';
import '../playback_models.dart';
import 'sendspin_rust_backend.dart';

class SendspinPlayer {
  SendspinPlayer({
    required this.transportBackend,
    required this.clientResolver,
  });

  final SendspinRustBackend transportBackend;
  final MusicAssistantClient? Function() clientResolver;

  final StreamController<PlaybackQueueSnapshot> _queueStateController =
      StreamController<PlaybackQueueSnapshot>.broadcast();
  PlaybackQueueSnapshot _queueState = const PlaybackQueueSnapshot();

  PlaybackOutputMode get outputMode => transportBackend.outputMode;

  Stream<PlaybackState> get states => transportBackend.states;

  Stream<PlaybackQueueSnapshot> get queueStates => _queueStateController.stream;

  Future<void> activate({String? targetPlayerId}) =>
      transportBackend.activate(targetPlayerId: targetPlayerId);

  Future<void> deactivate() => transportBackend.deactivate();

  Future<void> connect() => transportBackend.connect();

  Future<void> reconnect() => transportBackend.reconnect();

  Future<void> disconnect({String reason = 'client_disconnect'}) =>
      transportBackend.disconnect(reason: reason);

  Future<void> terminate() async {
    if (!_queueStateController.isClosed) {
      await _queueStateController.close();
    }
    await transportBackend.terminate();
  }

  Future<void> transportPlay() => transportBackend.transportPlay();

  Future<void> transportPause() => transportBackend.transportPause();

  Future<void> transportNext() => transportBackend.transportNext();

  Future<void> transportPrevious() => transportBackend.transportPrevious();

  Future<void> transportSeek(double seconds) =>
      transportBackend.transportSeek(seconds);

  Future<void> setVolume(int volume) => transportBackend.setVolume(volume);

  Future<void> setMuted(bool muted) => transportBackend.setMuted(muted);

  Future<void> playMedia(
    List<Map<String, dynamic>> mediaItems, {
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

  Future<void> moveQueueItem(String queueItemId, {int posShift = 0}) async {
    final client = clientResolver();
    if (client == null || queueItemId.trim().isEmpty) {
      return;
    }

    final String queueId = await _resolveQueueId(client);
    await client.moveQueueItem(queueId, queueItemId.trim(), posShift: posShift);
    _projectMoveQueueItem(queueId, queueItemId.trim(), posShift);
  }

  Future<void> deleteQueueItem(String queueItemId) async {
    final client = clientResolver();
    if (client == null || queueItemId.trim().isEmpty) {
      return;
    }

    final String queueId = await _resolveQueueId(client);
    await client.deleteQueueItemById(queueId, queueItemId.trim());
    _projectDeleteQueueItem(queueId, queueItemId.trim());
  }

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

  Future<PlaybackState> getState() => transportBackend.getState();

  Future<PlaybackQueueSnapshot> getQueueState() async {
    return _queueState;
  }

  Future<String> _resolveQueueId(MusicAssistantClient client) async {
    if ((_queueState.queueId ?? '').trim().isNotEmpty) {
      return _queueState.queueId!.trim();
    }

    final List<MaPlayerQueue> queues = await client.getPlayerQueues();
    final List<MaPlayerQueue> valid =
        queues.where((q) => q.queueId.trim().isNotEmpty).toList(growable: false);
    if (valid.isEmpty) {
      throw StateError('Keine Player Queue verfuegbar.');
    }

    final MaPlayerQueue? active =
        valid.where((q) => q.active).cast<MaPlayerQueue?>().firstWhere(
              (q) => q != null,
              orElse: () => null,
            );
    final MaPlayerQueue? available =
        valid.where((q) => q.available).cast<MaPlayerQueue?>().firstWhere(
              (q) => q != null,
              orElse: () => null,
            );

    final String queueId = (active ?? available ?? valid.first).queueId.trim();
    _queueState = _queueState.copyWith(queueId: queueId);
    _emitQueueState();
    return queueId;
  }

  MaQueueOption _toMaQueueOption(PlaybackQueueInsertOption option) {
    switch (option) {
      case PlaybackQueueInsertOption.replace:
        return MaQueueOption.replace;
      case PlaybackQueueInsertOption.next:
        return MaQueueOption.next;
      case PlaybackQueueInsertOption.replaceNext:
        return MaQueueOption.replaceNext;
      case PlaybackQueueInsertOption.last:
        return MaQueueOption.add;
    }
  }

  MaRepeatMode _toMaRepeatMode(String raw) {
    switch (raw.trim().toLowerCase()) {
      case 'all':
        return MaRepeatMode.all;
      case 'one':
        return MaRepeatMode.one;
      default:
        return MaRepeatMode.off;
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
    List<Map<String, dynamic>> mediaItems, {
    required PlaybackQueueInsertOption option,
  }) {
    final List<Map<String, dynamic>> current = _queueState.items
        .map((e) => Map<String, dynamic>.from(e))
        .toList(growable: true);
    final List<Map<String, dynamic>> payload =
        mediaItems.map((e) => Map<String, dynamic>.from(e)).toList(growable: false);
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

  void _emitQueueState() {
    if (_queueStateController.isClosed) {
      return;
    }
    _queueStateController.add(_queueState);
  }
}
