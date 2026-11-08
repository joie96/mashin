import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/session/preferred_provider_controller.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/playlist/playlist_detail_controller.dart';

void main() {
  setUp(() {
    PreferredProviderController.instance.initialize(
      preferredProvider: null,
      persist: (_) async {},
    );
  });
  tearDown(PreferredProviderController.instance.reset);

  test(
    'playlist cache is visible early, unchanged content survives a failed refresh',
    () async {
      final bridge = _PlaylistBridge();
      final controller = PlaylistDetailController(
        coreBridge: bridge,
        itemId: '44',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadPlaylist();
      await bridge.requested.future;
      bridge.emitTracks();
      final tracks = controller.tracks;
      final header = controller.playlist;
      expect(bridge.tracks.isCompleted, isFalse);
      expect(controller.isLoadingTracks, isFalse);
      expect(tracks.map((track) => track.position), [1, 4]);
      var contentChanges = 0;
      var statusChanges = 0;
      controller.addListener(() => contentChanges++);
      controller.readStatusListenable.addListener(() => statusChanges++);
      bridge.emitTracks();
      bridge.emitPhase('refreshing');
      bridge.emitPhase('failed');
      expect(contentChanges, 0);
      expect(statusChanges, 2);
      expect(identical(controller.tracks, tracks), isTrue);
      expect(identical(controller.playlist, header), isTrue);
      bridge.tracks.completeError(StateError('offline'));
      await loading;
      expect(identical(controller.tracks, tracks), isTrue);
      expect(controller.tracksReadState?.phase, CatalogLoadPhase.failed);
      await controller.setPageVisible(false);
      bridge.emitTracks(name: 'Hidden');
      expect(identical(controller.tracks, tracks), isTrue);
    },
  );

  test(
    'playlist invalidation rejects pending completions and late events',
    () async {
      final bridge = _PlaylistBridge();
      final controller = PlaylistDetailController(
        coreBridge: bridge,
        itemId: '44',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadPlaylist();
      await bridge.requested.future;
      bridge.emitTracks();
      final tracks = controller.tracks;
      bridge.emitTracks(name: 'Dynamic', allowDynamicTracks: true);
      expect(identical(controller.tracks, tracks), isTrue);
      bridge.emitPhase('invalidated');
      bridge.emitTracks(name: 'Late');
      bridge.tracks.complete(tracks);
      await loading;
      expect(controller.tracks, isEmpty);
      expect(controller.isLoadingTracks, isFalse);
    },
  );
}

class _PlaylistBridge implements MashinCoreBridgeApi {
  final updates = StreamController<Map<String, dynamic>>.broadcast(sync: true);
  final tracks = Completer<List<Track>>();
  final requested = Completer<void>();

  @override
  Stream<Map<String, dynamic>> get events => updates.stream;

  void emitTracks({String name = 'Cached', bool allowDynamicTracks = false}) {
    updates.add({
      'type': 'catalog_updated',
      'query': {
        'PlaylistTracks': {
          'item_id': '44',
          'provider': 'library',
          'allow_dynamic_tracks': allowDynamicTracks,
        },
      },
      'value': {
        'Tracks': [
          for (final position in [1, 4])
            {
              'provider': 'source-a',
              'item_id': 'track',
              'name': name,
              'position': position,
            },
        ],
      },
    });
  }

  void emitPhase(String phase) {
    updates.add({
      'type': 'catalog_read_changed',
      'query': {
        'PlaylistTracks': {
          'item_id': '44',
          'provider': 'library',
          'allow_dynamic_tracks': false,
        },
      },
      'snapshot': {
        'phase': phase,
        'knowledge': 'observed_response',
        'freshness': 'unknown',
      },
    });
  }

  @override
  Future<Playlist> getPlaylist(String itemId, String provider) async =>
      Playlist.fromJson({
        'item_id': itemId,
        'provider': provider,
        'name': 'Library header',
      });

  @override
  Future<List<Track>> getPlaylistTracks(
    String itemId,
    String provider, {
    bool forceRefresh = false,
    bool allowDynamicTracks = false,
  }) {
    requested.complete();
    return tracks.future;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
