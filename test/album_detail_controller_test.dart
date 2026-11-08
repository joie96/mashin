import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/session/preferred_provider_controller.dart';
import 'package:mashin/features/album/album_detail_controller.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_models.dart';

void main() {
  setUp(() {
    PreferredProviderController.instance.initialize(
      preferredProvider: null,
      persist: (_) async {},
    );
  });
  tearDown(PreferredProviderController.instance.reset);

  test(
    'identical album and track responses retain content and notify only changed sections',
    () async {
      final bridge = _AlbumBridge(shouldDelayTracks: true);
      final controller = AlbumDetailController(
        coreBridge: bridge,
        itemId: '44',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadAlbum();
      bridge.emit('Header');
      await Future<void>.delayed(Duration.zero);
      bridge.emitTracks('library', '44', 'Track');
      final header = controller.album!;
      final tracks = controller.tracks;
      var metadataChanges = 0;
      var trackChanges = 0;
      var pageChanges = 0;
      var statusChanges = 0;
      controller.metadataListenable.addListener(() => metadataChanges++);
      controller.tracksListenable.addListener(() => trackChanges++);
      controller.addListener(() => pageChanges++);
      controller.readStatusListenable.addListener(() => statusChanges++);
      bridge.emit('Header');
      bridge.emitTracks('library', '44', 'Track');
      bridge.emitPhase('refreshing');
      bridge.emitPhase('ready', kind: 'AlbumTracks');
      expect(metadataChanges, 0);
      expect(trackChanges, 0);
      expect(pageChanges, 0);
      expect(statusChanges, 2);
      expect(identical(controller.album, header), isTrue);
      expect(identical(controller.tracks, tracks), isTrue);
      bridge.emitTracks('library', '44', 'Changed');
      expect(trackChanges, 1);
      expect(metadataChanges, 0);
      bridge.pendingTracks[('44', 'library')]!.complete(controller.tracks);
      bridge.metadata.complete(header);
      await loading;
      expect(metadataChanges, 0);
      expect(trackChanges, 1);
    },
  );

  test(
    'album tracks start before metadata refresh, preserve context and survive failures',
    () async {
      final bridge = _AlbumBridge(shouldDelayTracks: true);
      final controller = AlbumDetailController(
        coreBridge: bridge,
        itemId: '44',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadAlbum();
      bridge.emit('Cached');
      await Future<void>.delayed(Duration.zero);
      expect(bridge.metadata.isCompleted, isFalse);
      expect(bridge.trackRequestCount, 1);
      bridge.emitTracks('library', '44', 'Cached track');
      bridge.emitPhase('refreshing', kind: 'AlbumTracks');
      expect(controller.isLoadingTracks, isFalse);
      expect(controller.tracks.single.album?.provider, 'source-a');
      expect(controller.tracks.single.album?.itemId, '007');
      expect(controller.tracks.single.discNumber, 2);
      expect(controller.tracks.single.trackNumber, 9);
      bridge.emitPhase('failed', kind: 'AlbumTracks');
      bridge.pendingTracks[('44', 'library')]!.completeError(
        StateError('offline tracks'),
      );
      bridge.metadata.completeError(StateError('offline metadata'));
      await loading;
      expect(controller.album?.name, 'Cached');
      expect(controller.tracks.single.name, 'Cached track');
      expect(controller.tracksReadState?.phase, CatalogLoadPhase.failed);
      final reload = controller.loadAlbum();
      await Future<void>.delayed(Duration.zero);
      expect(controller.tracks.single.name, 'Cached track');
      bridge.pendingTracks[('44', 'library')]!.completeError(
        StateError('still offline'),
      );
      await reload;
      expect(controller.tracks.single.name, 'Cached track');
    },
  );

  test(
    'album provider changes keep the original header and reject old track events',
    () async {
      final bridge = _AlbumBridge(shouldDelayTracks: true);
      final controller = AlbumDetailController(
        coreBridge: bridge,
        itemId: '44',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadAlbum();
      bridge.emit('Library header', hasMappings: true);
      await Future<void>.delayed(Duration.zero);
      bridge.emitTracks('source-a', '007', 'A');
      bridge.pendingTracks[('007', 'source-a')]!.complete(controller.tracks);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.metadata.isCompleted, isFalse);
      final selected = controller.selectProvider(
        controller.sectionProvider.mappings.firstWhere(
          (mapping) => mapping.providerInstance == 'source-b',
        ),
      );
      await Future<void>.delayed(Duration.zero);
      expect(controller.album?.provider, 'library');
      expect(controller.album?.name, 'Library header');
      expect(controller.tracks, isEmpty);
      bridge.emitTracks('source-a', '007', 'Late A');
      expect(controller.tracks, isEmpty);
      bridge.emitTracks('source-b', '008', 'B');
      bridge.pendingTracks[('008', 'source-b')]!.complete(controller.tracks);
      await selected;
      expect(controller.tracks.single.name, 'B');
      expect(controller.album?.name, 'Library header');
      bridge.metadata.complete(controller.album!);
      await loading;
    },
  );

  test(
    'disposed album controller ignores late track and metadata completions',
    () async {
      final bridge = _AlbumBridge(shouldDelayTracks: true);
      final controller = AlbumDetailController(
        coreBridge: bridge,
        itemId: '44',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(bridge.updates.close);
      final loading = controller.loadAlbum();
      bridge.emit('Cached');
      await Future<void>.delayed(Duration.zero);
      bridge.emitTracks('library', '44', 'Cached track');
      final tracks = controller.tracks;
      final album = controller.album!;
      controller.dispose();
      bridge.emitTracks('library', '44', 'Late event');
      bridge.pendingTracks[('44', 'library')]!.complete([]);
      bridge.metadata.complete(
        Album.fromJson({
          'provider': 'library',
          'item_id': '44',
          'name': 'Late metadata',
        }),
      );
      await loading;
      expect(controller.tracks, tracks);
      expect(controller.album, album);
    },
  );

  test(
    'album track invalidation clears data and ignores late futures',
    () async {
      final bridge = _AlbumBridge(shouldDelayTracks: true);
      final controller = AlbumDetailController(
        coreBridge: bridge,
        itemId: '44',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadAlbum();
      bridge.emit('Cached');
      await Future<void>.delayed(Duration.zero);
      bridge.emitTracks('library', '44', 'Cached track');
      final tracks = controller.tracks;
      bridge.emitPhase('invalidated', kind: 'AlbumTracks');
      expect(controller.album, isNull);
      expect(controller.tracks, isEmpty);
      bridge.emitTracks('library', '44', 'Late event');
      bridge.pendingTracks[('44', 'library')]!.complete(tracks);
      bridge.metadata.complete(
        Album.fromJson({
          'provider': 'library',
          'item_id': '44',
          'name': 'Late metadata',
        }),
      );
      await loading;
      expect(controller.album, isNull);
      expect(controller.tracks, isEmpty);
      expect(controller.isLoadingSections, isFalse);
    },
  );

  test(
    'cached album is visible before refresh and survives its failure',
    () async {
      final bridge = _AlbumBridge();
      final controller = AlbumDetailController(
        coreBridge: bridge,
        itemId: '44',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadAlbum();
      bridge.emit('Cached');
      expect(bridge.metadata.isCompleted, isFalse);
      expect(controller.album?.name, 'Cached');
      expect(controller.isLoadingMetadata, isFalse);
      bridge.emitPhase('failed');
      bridge.metadata.completeError(StateError('offline'));
      await loading;
      expect(controller.album?.name, 'Cached');
      expect(controller.album?.provider, 'library');
      expect(controller.description, 'Cached description');
      expect(controller.metadataReadState?.phase, CatalogLoadPhase.failed);
      expect(
        controller.metadataReadState?.knowledge,
        CatalogReadKnowledge.partial,
      );
      await controller.setPageVisible(false);
      bridge.emit('Hidden');
      expect(controller.album?.name, 'Cached');
    },
  );

  test(
    'album invalidation clears the header and ignores old completions',
    () async {
      final bridge = _AlbumBridge();
      final controller = AlbumDetailController(
        coreBridge: bridge,
        itemId: '44',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadAlbum();
      bridge.emit('Cached');
      bridge.emitPhase('invalidated');
      expect(controller.album, isNull);
      expect(controller.tracks, isEmpty);
      expect(controller.isLoadingSections, isFalse);
      expect(controller.metadataReadState?.phase, CatalogLoadPhase.invalidated);
      bridge.emit('Late event');
      bridge.metadata.complete(
        Album.fromJson({
          'provider': 'library',
          'item_id': '44',
          'name': 'Late response',
        }),
      );
      await loading;
      expect(controller.album, isNull);
      expect(bridge.trackRequestCount, 0);
    },
  );
}

class _AlbumBridge implements MashinCoreBridgeApi {
  _AlbumBridge({this.shouldDelayTracks = false});
  final bool shouldDelayTracks;
  final updates = StreamController<Map<String, dynamic>>.broadcast(sync: true);
  final metadata = Completer<Album>();
  final pendingTracks = <(String, String), Completer<List<Track>>>{};
  int trackRequestCount = 0;

  @override
  Stream<Map<String, dynamic>> get events => updates.stream;

  // Synthetic observations exercise the production catalog event decoder.
  void emit(String name, {bool hasMappings = false}) {
    updates.add({
      'type': 'catalog_updated',
      'query': {
        'Album': {'item_id': '44', 'provider': 'library'},
      },
      'value': {
        'Album': {
          'item_id': '44',
          'provider': 'library',
          'name': name,
          'metadata': {'description': 'Cached description'},
          if (hasMappings)
            'provider_mappings': [
              {
                'provider_domain': 'source',
                'provider_instance': 'source-a',
                'item_id': '007',
                'available': true,
              },
              {
                'provider_domain': 'source',
                'provider_instance': 'source-b',
                'item_id': '008',
                'available': true,
              },
            ],
        },
      },
    });
  }

  void emitTracks(String provider, String itemId, String name) {
    updates.add({
      'type': 'catalog_updated',
      'query': {
        'AlbumTracks': {'provider': provider, 'item_id': itemId},
      },
      'value': {
        'Tracks': [
          {
            'provider': provider,
            'item_id': 'track',
            'name': name,
            'album': {
              'provider': 'source-a',
              'item_id': '007',
              'name': 'Provider album',
            },
            'disc_number': 2,
            'track_number': 9,
          },
        ],
      },
    });
  }

  void emitPhase(String phase, {String kind = 'Album'}) {
    updates.add({
      'type': 'catalog_read_changed',
      'query': {
        kind: {'item_id': '44', 'provider': 'library'},
      },
      'snapshot': {
        'phase': phase,
        'knowledge': phase == 'invalidated' ? 'unknown' : 'partial',
        'freshness': 'unknown',
        'observed_at': null,
        'error': 'unavailable',
      },
    });
  }

  // Metadata remains pending independently of section reads.
  @override
  Future<Album> getAlbum(String itemId, String provider) => metadata.future;

  @override
  Future<List<Track>> getAlbumTracks(String itemId, String provider) {
    trackRequestCount++;
    if (shouldDelayTracks) {
      final pending = Completer<List<Track>>();
      pendingTracks[(itemId, provider)] = pending;
      return pending.future;
    }
    return Future.value([]);
  }

  @override
  void log(LogLevelPreference level, String category, String message) {}

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
