import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/features/core/core_models.dart';

void main() {
  test('projects the Rust player-state payload', () {
    final playerState = PlayerState.fromJson(<String, dynamic>{
      'activeEngineKind': 'remote',
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

    expect(playerState.activeOutputMode, CoreOutputMode.remote);
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