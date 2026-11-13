import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/explore/explore_controller.dart';
import 'package:mashin/features/home/home_controller.dart';

void main() {
  test(
    'home refreshes history after thirty seconds of playback and track end',
    () async {
      var now = DateTime(2026);
      final bridge = _RecommendationBridge();
      final controller = HomeController(coreBridge: bridge, now: () => now);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      bridge.emitPlayback('first');
      expect(bridge.recentReadCount, 0);
      await controller.load();
      bridge.result.complete(const RecommendationSnapshot());
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 1);
      bridge.emitPlayback('first');
      now = now.add(const Duration(seconds: 29));
      bridge.emitPlayback('first', positionMillis: 29000);
      expect(bridge.recentReadCount, 1);
      now = now.add(const Duration(seconds: 1));
      bridge.emitPlayback('second');
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 2);
      expect(controller.recentListens.single.name, 'History 2');
      now = now.add(const Duration(seconds: 1));
      bridge.emitPlayback('third');
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 2);
      now = now.add(const Duration(seconds: 30));
      bridge.emitPlayback('third', playbackState: PlayState.idle);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 3);
    },
  );

  test(
    'home ignores paused time and throttles after another history refresh',
    () async {
      var now = DateTime(2026);
      final bridge = _RecommendationBridge();
      final controller = HomeController(coreBridge: bridge, now: () => now);
      addTearDown(bridge.eventsController.close);
      await controller.load();
      bridge.result.complete(const RecommendationSnapshot());
      await Future<void>.delayed(Duration.zero);
      bridge.emitPlayback('first');
      now = now.add(const Duration(seconds: 10));
      bridge.emitPlayback('first', playbackState: PlayState.paused);
      now = now.add(const Duration(seconds: 60));
      bridge.emitPlayback('first');
      now = now.add(const Duration(seconds: 19));
      bridge.emitPlayback('second');
      expect(bridge.recentReadCount, 1);
      now = now.add(const Duration(seconds: 30));
      bridge.emitPlayback('second', positionMillis: 30000);
      expect(bridge.recentReadCount, 1);
      bridge.eventsController.add({'type': 'provider_instances_changed'});
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 2);
      now = now.add(const Duration(seconds: 1));
      bridge.emitPlayback('third');
      expect(bridge.recentReadCount, 2);
      now = now.add(const Duration(seconds: 30));
      bridge.emitPlayback('third', playbackState: PlayState.idle);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 3);
      controller.dispose();
      now = now.add(const Duration(seconds: 60));
      bridge.emitPlayback('fourth');
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 3);
    },
  );

  test(
    'home refreshes history for repeat one without refreshing every position',
    () async {
      var now = DateTime(2026);
      final bridge = _RecommendationBridge();
      final controller = HomeController(coreBridge: bridge, now: () => now);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      await controller.load();
      bridge.result.complete(const RecommendationSnapshot());
      await Future<void>.delayed(Duration.zero);
      bridge.emitPlayback('repeated', durationMillis: 60000);
      for (var repeat = 0; repeat < 2; repeat++) {
        now = now.add(const Duration(seconds: 59));
        bridge.emitPlayback(
          'repeated',
          positionMillis: 59000,
          durationMillis: 60000,
        );
        expect(bridge.recentReadCount, repeat + 1);
        now = now.add(const Duration(seconds: 1));
        bridge.emitPlayback('repeated', durationMillis: 60000);
        await Future<void>.delayed(Duration.zero);
        expect(bridge.recentReadCount, repeat + 2);
        bridge.emitPlayback('repeated', durationMillis: 60000);
        expect(bridge.recentReadCount, repeat + 2);
      }
    },
  );

  test(
    'home coalesces track ends during a pending history read and retains failed data',
    () async {
      var now = DateTime(2026);
      final pending = Completer<List<MediaItem>>();
      final bridge = _RecommendationBridge(
        recentLoader: (count) {
          if (count == 1) return pending.future;
          return Future.error(StateError('offline'));
        },
      );
      final controller = HomeController(coreBridge: bridge, now: () => now);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      await controller.load();
      bridge.result.complete(const RecommendationSnapshot());
      bridge.emitPlayback('first');
      now = now.add(const Duration(seconds: 30));
      bridge.emitPlayback('second');
      now = now.add(const Duration(seconds: 30));
      bridge.emitPlayback('third');
      expect(bridge.recentReadCount, 1);
      pending.complete([
        Track.fromJson({
          'provider': 'source',
          'item_id': 'cached',
          'name': 'Retained',
        }),
      ]);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 2);
      expect(controller.recentListens.single.name, 'Retained');
      expect(controller.isLoadingRecentListens, isFalse);
    },
  );

  test(
    'home retains early cached recent tracks when the pending read fails',
    () async {
      for (final hasTracks in [true, false]) {
        final recent = Completer<List<MediaItem>>();
        final bridge = _RecommendationBridge(recentResult: recent);
        final controller = HomeController(coreBridge: bridge);
        addTearDown(() async {
          controller.dispose();
          await bridge.eventsController.close();
        });
        await controller.load();
        await bridge.requested.future;
        bridge.eventsController.add({
          'type': 'catalog_updated',
          'query': {
            'RecentlyPlayed': {'limit': 40},
          },
          'value': {
            'Tracks': [
              if (hasTracks)
                {
                  'provider': 'source',
                  'item_id': 'recent',
                  'name': 'Cached recent',
                },
            ],
          },
        });
        final cached = controller.recentListens;
        if (hasTracks) {
          expect(cached.single.name, 'Cached recent');
        } else {
          expect(cached, isEmpty);
        }
        expect(controller.isLoadingRecentListens, isFalse);
        expect(recent.isCompleted, isFalse);
        recent.completeError(StateError('offline'));
        bridge.result.complete(const RecommendationSnapshot());
        await Future<void>.delayed(Duration.zero);
        expect(identical(controller.recentListens, cached), isTrue);
        expect(controller.isLoadingRecentListens, isFalse);
      }
    },
  );

  test(
    'home retains unknown or failed sections and accepts authoritative empty',
    () async {
      final bridge = _RecommendationBridge();
      final controller = HomeController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      await controller.load();
      await bridge.requested.future;
      bridge.emit(RecommendationView.home);
      final tracks = controller.topTracks;
      bridge.emitEmpty(RecommendationView.home, isLoading: true);
      expect(identical(controller.topTracks, tracks), isTrue);
      bridge.emitEmpty(RecommendationView.home, hasLoadError: true);
      expect(identical(controller.topTracks, tracks), isTrue);
      bridge.emitEmpty(RecommendationView.home);
      expect(controller.topTracks, isEmpty);
      bridge.result.complete(const RecommendationSnapshot());
      await Future<void>.delayed(Duration.zero);
    },
  );

  test(
    'explore retains unknown or failed sections and accepts authoritative empty',
    () async {
      final bridge = _RecommendationBridge();
      final controller = ExploreController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      final loading = controller.load();
      await bridge.requested.future;
      bridge.emit(RecommendationView.explore);
      final tracks = controller.weeklyTracks;
      bridge.emitEmpty(RecommendationView.explore, isLoading: true);
      expect(identical(controller.weeklyTracks, tracks), isTrue);
      bridge.emitEmpty(RecommendationView.explore, hasLoadError: true);
      expect(identical(controller.weeklyTracks, tracks), isTrue);
      bridge.emitEmpty(RecommendationView.explore);
      expect(controller.weeklyTracks, isEmpty);
      bridge.result.complete(const RecommendationSnapshot());
      await loading;
    },
  );

  test(
    'home publishes early sections and retains identical content and failed refresh',
    () async {
      final bridge = _RecommendationBridge();
      final controller = HomeController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      await controller.load();
      await bridge.requested.future;
      bridge.emit(RecommendationView.home);
      final tracks = controller.topTracks;
      final artists = controller.topArtists;
      expect(tracks.single.name, 'Cached');
      expect(bridge.result.isCompleted, isFalse);
      var tracksChanges = 0;
      var artistsChanges = 0;
      controller.topTracksListenable.addListener(() => tracksChanges++);
      controller.topArtistsListenable.addListener(() => artistsChanges++);
      bridge.emit(RecommendationView.home);
      expect(tracksChanges, 0);
      expect(artistsChanges, 0);
      expect(identical(controller.topTracks, tracks), isTrue);
      bridge.emit(RecommendationView.explore, name: 'Wrong view');
      expect(identical(controller.topTracks, tracks), isTrue);
      bridge.emit(RecommendationView.home, name: 'Server');
      expect(tracksChanges, 1);
      expect(artistsChanges, 0);
      expect(identical(controller.topArtists, artists), isTrue);
      final updated = controller.topTracks;
      bridge.result.completeError(StateError('offline'));
      await Future<void>.delayed(Duration.zero);
      expect(controller.hasLoadError, isTrue);
      expect(identical(controller.topTracks, updated), isTrue);
    },
  );

  test(
    'explore accepts early snapshots, suppresses duplicates and rejects disposed events',
    () async {
      final bridge = _RecommendationBridge();
      final controller = ExploreController(coreBridge: bridge);
      addTearDown(bridge.eventsController.close);
      final loading = controller.load();
      await bridge.requested.future;
      bridge.emit(RecommendationView.explore);
      final tracks = controller.weeklyTracks;
      final artists = controller.weeklyArtists;
      expect(tracks.single.name, 'Cached');
      expect(bridge.result.isCompleted, isFalse);
      var changes = 0;
      controller.addListener(() => changes++);
      bridge.emit(RecommendationView.explore);
      expect(changes, 0);
      expect(identical(controller.weeklyTracks, tracks), isTrue);
      bridge.emit(RecommendationView.home, name: 'Wrong view');
      expect(changes, 0);
      bridge.emit(RecommendationView.explore, name: 'Server');
      expect(changes, 1);
      expect(identical(controller.weeklyArtists, artists), isTrue);
      final updated = controller.weeklyTracks;
      controller.dispose();
      bridge.emit(RecommendationView.explore, name: 'Late');
      bridge.result.complete(const RecommendationSnapshot());
      await loading;
      expect(identical(controller.weeklyTracks, updated), isTrue);
    },
  );
}

class _RecommendationBridge implements MashinCoreBridgeApi {
  _RecommendationBridge({this.recentResult, this.recentLoader});

  final Completer<List<MediaItem>>? recentResult;
  final Future<List<MediaItem>> Function(int)? recentLoader;
  final eventsController = StreamController<Map<String, dynamic>>.broadcast(
    sync: true,
  );
  final result = Completer<RecommendationSnapshot>();
  final requested = Completer<void>();
  int recentReadCount = 0;

  @override
  Stream<PlayerState> get playerStateChanges => eventsController.stream
      .where((event) => event['type'] == 'player_state_changed')
      .map(
        (event) =>
            PlayerState.fromJson(event['player_state'] as Map<String, dynamic>),
      );

  void emitPlayback(
    String itemId, {
    PlayState playbackState = PlayState.playing,
    int positionMillis = 0,
    int durationMillis = 180000,
  }) {
    eventsController.add({
      'type': 'player_state_changed',
      'player_state': {
        'connectionState': 'online',
        'playbackState': playbackState.name,
        'currentQueueItemId': itemId,
        'positionMillis': positionMillis,
        'durationMillis': durationMillis,
      },
    });
  }

  @override
  Stream<Map<String, dynamic>> get events => eventsController.stream;

  void emitEmpty(
    RecommendationView view, {
    bool isLoading = false,
    bool hasLoadError = false,
  }) {
    eventsController.add({
      'type': 'recommendations_updated',
      'view': view.name,
      'snapshot': {
        'is_loading': isLoading,
        'has_load_error': hasLoadError,
        'folders': [],
      },
    });
  }

  // Exercise the production snapshot decoder through the native event contract.
  void emit(RecommendationView view, {String name = 'Cached'}) {
    eventsController.add({
      'type': 'recommendations_updated',
      'view': view.name,
      'snapshot': {
        'is_loading': true,
        'has_load_error': false,
        'folders': [
          {
            'item_id': view == RecommendationView.home
                ? 'user_top_tracks_monthly'
                : 'sitewide_top_tracks_weekly',
            'provider': 'source',
            'title': 'Tracks',
            'is_loading': false,
            'has_load_error': false,
            'items': [
              {
                'media_type': 'track',
                'provider': 'source',
                'item_id': 'track',
                'name': name,
              },
            ],
          },
          {
            'item_id': view == RecommendationView.home
                ? 'user_top_artists_monthly'
                : 'sitewide_top_artists_weekly',
            'provider': 'source',
            'title': 'Artists',
            'is_loading': false,
            'has_load_error': false,
            'items': [
              {
                'media_type': 'artist',
                'provider': 'source',
                'item_id': 'artist',
                'name': 'Unchanged',
              },
            ],
          },
        ],
      },
    });
  }

  @override
  Future<RecommendationSnapshot> getRecommendations({
    required RecommendationView view,
    bool refresh = true,
  }) {
    requested.complete();
    return result.future;
  }

  @override
  Future<List<MediaItem>> getRecentlyPlayedTracks({int limit = 40}) {
    recentReadCount++;
    return recentLoader?.call(recentReadCount) ??
        recentResult?.future ??
        Future.value([
          Track.fromJson({
            'provider': 'source',
            'item_id': 'history-$recentReadCount',
            'name': 'History $recentReadCount',
          }),
        ]);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
