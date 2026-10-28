import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/features/core/core_models.dart';

void main() {
  List<QueueItem> queueItems(List<int?> sortIndices) =>
      List<QueueItem>.generate(
        sortIndices.length,
        (index) => QueueItem(
          queueItemId: 'queue-$index',
          sortIndex: sortIndices[index],
          mediaItem: const Track(
            itemId: 'same-track',
            provider: 'test',
            name: 'Track',
            duration: 120,
            artists: <Artist>[],
          ),
        ),
      );

  test('queue numbers rank sparse sort indices without reordering items', () {
    final items = queueItems(<int?>[0, 12, 4, 9]);
    expect(items.normalizedItemNumbers, <int>[1, 4, 2, 3]);
    expect(items.map((item) => item.sortIndex), <int>[0, 12, 4, 9]);
    expect(items.map((item) => item.queueItemId), <String>[
      'queue-0',
      'queue-1',
      'queue-2',
      'queue-3',
    ]);
  });

  test('queue numbers follow item order only when sort indices are absent', () {
    expect(queueItems(<int?>[null, null, null]).normalizedItemNumbers, <int>[
      1,
      2,
      3,
    ]);
    expect(queueItems(<int?>[8, null, 2]).normalizedItemNumbers, <int>[
      3,
      1,
      2,
    ]);
  });

  test(
    'equal queue sort indices keep snapshot order including duplicate tracks',
    () {
      expect(queueItems(<int?>[8, 2, 8, 2]).normalizedItemNumbers, <int>[
        3,
        1,
        4,
        2,
      ]);
    },
  );

  test(
    'queue numbers are dense after removal and independent of index scale',
    () {
      final items = queueItems(<int?>[0, 12, 4, 9]);
      expect(
        <QueueItem>[items[0], items[1], items[3]].normalizedItemNumbers,
        <int>[1, 3, 2],
      );
      expect(
        queueItems(<int?>[100, 900, 200, 500]).normalizedItemNumbers,
        <int>[1, 4, 2, 3],
      );
    },
  );

  test('queue numbering handles empty and single-item snapshots', () {
    expect(queueItems(<int?>[]).normalizedItemNumbers, isEmpty);
    expect(queueItems(<int?>[42]).normalizedItemNumbers, <int>[1]);
  });

  test('projects the Rust player-state payload', () {
    final playerState = PlayerState.fromJson(<String, dynamic>{
      'activeMode': 'remote',
      'connectionState': 'online',
      'playbackState': 'playing',
      'currentQueueItemId': 'queue-item-7',
      'currentIndex': 0,
      'positionMillis': 12345,
      'durationMillis': 180000,
      'bufferedPositionMillis': 18000,
      'volume': 64,
      'muted': false,
    });

    expect(playerState.activeMode, PlaybackMode.remote);
    expect(playerState.connectionState, CoreConnectionState.online);
    expect(playerState.playbackState, PlayState.playing);
    expect(playerState.currentQueueItemId, 'queue-item-7');
    expect(playerState.positionMillis, 12345);
    expect(playerState.durationMillis, 180000);
    expect(playerState.bufferedPositionMillis, 18000);
    expect(playerState.volume, 64);
  });

  test('projects the Rust queue payload without player-state fields', () {
    final queue = Queue.fromJson(<String, dynamic>{
      'queueId': 'queue-42',
      'items': const <Map<String, dynamic>>[],
      'currentIndex': 0,
      'shuffleEnabled': true,
      'repeatMode': 'all',
      'autoplayEnabled': true,
    });

    expect(queue.queueId, 'queue-42');
    expect(queue.items, isEmpty);
    expect(queue.currentIndex, 0);
    expect(queue.shuffleEnabled, isTrue);
    expect(queue.repeatMode, RepeatMode.all);
    expect(queue.autoplayEnabled, isTrue);
  });
}
