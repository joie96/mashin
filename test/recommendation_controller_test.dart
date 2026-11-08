import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/explore/explore_controller.dart';
import 'package:mashin/features/home/home_controller.dart';

void main() {
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
  final eventsController = StreamController<Map<String, dynamic>>.broadcast(
    sync: true,
  );
  final result = Completer<RecommendationSnapshot>();
  final requested = Completer<void>();

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
  Future<List<MediaItem>> getRecentlyPlayedTracks({int limit = 40}) async =>
      const [];

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
