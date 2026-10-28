import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/features/core/core_models.dart';

void main() {
  test('only canonical local and remote mode names are accepted', () {
    expect(
      PlayerState.fromJson({'activeMode': 'local'}).activeMode,
      PlaybackMode.local,
    );
    expect(
      PlayerState.fromJson({'activeMode': 'remote'}).activeMode,
      PlaybackMode.remote,
    );
    for (final removed in ['mashin', 'sendspin', 'offline']) {
      expect(PlayerState.fromJson({'activeMode': removed}).activeMode, isNull);
    }
  });

  test(
    'projects the Mashin session profile and serializes only client settings',
    () {
      final configured = SessionSettings.fromJson(<String, dynamic>{
        'audioProfile': 'opus_192k',
      });
      expect(configured.audioProfile, 'opus_192k');
      expect(
        const SessionSettingsUpdate(
          localCacheSizeBytes: 262144000,
          isDarkTheme: true,
          logLevel: LogLevelPreference.info,
        ).toJson(),
        {
          'localCacheSizeBytes': 262144000,
          'theme': 'dark',
          'logLevel': 'info',
          'shouldShowMultiDeviceButton': true,
        },
      );
      expect(
        PlayerState.fromJson({'activeMode': 'local'}).activeMode,
        PlaybackMode.local,
      );
    },
  );

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
