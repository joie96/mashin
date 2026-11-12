import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/session/preferred_provider_controller.dart';
import 'package:mashin/features/artist/artist_detail_controller.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_models.dart';

void main() {
  test(
    'late cached albums reproject top track years without waiting for refresh',
    () async {
      final bridge = _ArtistBridge(deferSections: true);
      final controller = ArtistDetailController(
        coreBridge: bridge,
        itemId: '007',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadArtist();
      bridge.emit('007', 'Cached');
      await Future<void>.delayed(Duration.zero);
      bridge.emitSection('ArtistTopTracks', 'Tracks', '007', 'library', [
        {
          'provider': 'library',
          'item_id': 'unknown-year',
          'name': 'Unknown year',
          'album': {'provider': 'library', 'item_id': 'album', 'name': 'Album'},
        },
        {
          'provider': 'library',
          'item_id': 'known-year',
          'name': 'Known year',
          'album': {
            'provider': 'library',
            'item_id': 'album',
            'name': 'Album',
            'year': 1999,
          },
        },
      ]);
      for (final year in [2020, 2024]) {
        bridge.emitSection('ArtistAlbums', 'Albums', '007', 'library', [
          {
            'provider': 'library',
            'item_id': 'album',
            'name': 'Album',
            'year': year,
          },
        ]);
        expect(controller.topTracks.first.album?.year, year);
        expect(controller.topTracks.last.album?.year, 1999);
      }
      bridge.albumRequests['library']!.completeError(
        StateError('offline albums'),
      );
      bridge.trackRequests['library']!.completeError(
        StateError('offline tracks'),
      );
      bridge.similarRequests['library']!.complete([]);
      bridge.metadata.completeError(StateError('offline metadata'));
      await loading;
      expect(controller.topTracks.first.album?.year, 2024);
    },
  );

  test(
    'artist reload starts sections from its existing header before metadata refresh',
    () async {
      final bridge = _ArtistBridge();
      final controller = ArtistDetailController(
        coreBridge: bridge,
        itemId: '007',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadArtist();
      bridge.emit('007', 'Cached');
      bridge.metadata.complete(controller.artist!);
      await loading;
      bridge.metadata = Completer<Artist>();
      final reloading = controller.loadArtist();
      await Future<void>.delayed(Duration.zero);
      expect(bridge.albumRequestCount, 2);
      expect(bridge.metadata.isCompleted, isFalse);
      bridge.metadata.completeError(StateError('offline metadata'));
      await reloading;
    },
  );

  test(
    'identical artist observations retain header identity without content notifications',
    () async {
      PreferredProviderController.instance.initialize(
        preferredProvider: null,
        persist: (_) async {},
      );
      addTearDown(PreferredProviderController.instance.reset);
      final bridge = _ArtistBridge();
      final controller = ArtistDetailController(
        coreBridge: bridge,
        itemId: '007',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadArtist();
      bridge.emit('007', 'Header');
      await Future<void>.delayed(Duration.zero);
      final header = controller.artist!;
      var metadataChanges = 0;
      controller.metadataListenable.addListener(() => metadataChanges++);
      var pageChanges = 0;
      var statusChanges = 0;
      controller.addListener(() => pageChanges++);
      controller.readStatusListenable.addListener(() => statusChanges++);
      bridge.emit('007', 'Header');
      bridge.emitReadState(
        'Artist',
        '007',
        'library',
        phase: 'ready',
        knowledge: 'observed_response',
        freshness: 'fresh',
      );
      expect(metadataChanges, 0);
      expect(pageChanges, 0);
      expect(statusChanges, 1);
      expect(identical(controller.artist, header), isTrue);
      bridge.metadata.complete(header);
      await loading;
      expect(metadataChanges, 0);
    },
  );

  test(
    'section invalidation retires the header and ignores later completions',
    () async {
      PreferredProviderController.instance.initialize(
        preferredProvider: null,
        persist: (_) async {},
      );
      addTearDown(PreferredProviderController.instance.reset);
      final bridge = _ArtistBridge(deferSections: true);
      final controller = ArtistDetailController(
        coreBridge: bridge,
        itemId: '007',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadArtist();
      bridge.emit(
        '007',
        'Cached',
        mappings: [
          {
            'provider_domain': 'source',
            'provider_instance': 'source-a',
            'item_id': 'artist-a',
            'available': true,
          },
        ],
      );
      await Future<void>.delayed(Duration.zero);
      bridge.updates.add({
        'type': 'catalog_read_changed',
        'query': {
          'ArtistAlbums': {'provider': 'source-a', 'item_id': 'artist-a'},
        },
        'snapshot': {
          'phase': 'invalidated',
          'knowledge': 'unknown',
          'freshness': 'unknown',
          'observed_at': null,
          'error': 'server changed',
        },
      });
      expect(controller.artist, isNull);
      expect(controller.albums, isEmpty);
      expect(controller.isLoadingSections, isFalse);
      bridge.emit('007', 'Late');
      bridge.albumRequests['source-a']!.complete([
        Album.fromJson({
          'provider': 'source-a',
          'item_id': 'late',
          'name': 'Late',
        }),
      ]);
      bridge.trackRequests['source-a']!.complete([]);
      bridge.similarRequests['source-a']!.complete([]);
      bridge.metadata.completeError(StateError('old session'));
      await loading;
      expect(controller.artist, isNull);
      expect(controller.albums, isEmpty);
    },
  );

  test(
    'provider changes cancel old sections without replacing the library header',
    () async {
      PreferredProviderController.instance.initialize(
        preferredProvider: null,
        persist: (_) async {},
      );
      addTearDown(PreferredProviderController.instance.reset);
      final bridge = _ArtistBridge(deferSections: true);
      final controller = ArtistDetailController(
        coreBridge: bridge,
        itemId: '007',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final mappings = <Map<String, dynamic>>[
        {
          'provider_domain': 'source',
          'provider_instance': 'source-a',
          'item_id': 'artist-a',
          'available': true,
        },
        {
          'provider_domain': 'source',
          'provider_instance': 'source-b',
          'item_id': 'artist-b',
          'available': true,
        },
      ];
      final loading = controller.loadArtist();
      bridge.emit('007', 'Cached', mappings: mappings);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.albumRequests.containsKey('source-a'), isTrue);
      expect(bridge.metadata.isCompleted, isFalse);
      bridge.emitSection('ArtistAlbums', 'Albums', 'artist-a', 'source-a', []);
      bridge.emitSection(
        'ArtistTopTracks',
        'Tracks',
        'artist-a',
        'source-a',
        [],
      );
      bridge.emitSection(
        'SimilarArtists',
        'Artists',
        'artist-a',
        'source-a',
        [],
        limit: 25,
      );
      final selected = controller.sectionProvider.mappings.firstWhere(
        (mapping) => mapping.providerInstance == 'source-b',
      );
      final switching = controller.selectProvider(selected);
      await Future<void>.delayed(Duration.zero);
      expect(controller.sectionProvider.selectedProvider, 'source-b');
      expect(controller.artist?.provider, 'library');
      bridge.albumRequests['source-a']!.complete([
        Album.fromJson({
          'item_id': 'old',
          'provider': 'source-a',
          'name': 'Old',
        }),
      ]);
      bridge.trackRequests['source-a']!.complete([]);
      bridge.similarRequests['source-a']!.complete([]);
      await Future<void>.delayed(Duration.zero);
      expect(controller.albums, isEmpty);
      expect(controller.isLoadingAlbums, isTrue);
      bridge.emitSection('ArtistAlbums', 'Albums', 'artist-b', 'source-b', [
        {
          'item_id': 'new',
          'provider': 'source-b',
          'name': 'Cached B',
          'year': 2025,
        },
      ]);
      bridge.emitSection(
        'ArtistTopTracks',
        'Tracks',
        'artist-b',
        'source-b',
        [],
      );
      bridge.emitSection(
        'SimilarArtists',
        'Artists',
        'artist-b',
        'source-b',
        [],
        limit: 25,
      );
      expect(controller.isLoadingAlbums, isFalse);
      bridge.albumRequests['source-b']!.completeError(StateError('offline'));
      bridge.trackRequests['source-b']!.completeError(StateError('offline'));
      bridge.similarRequests['source-b']!.completeError(StateError('offline'));
      await switching;
      bridge.metadata.completeError(StateError('offline'));
      await loading;
      expect(controller.albums.single.name, 'Cached B');
      expect(controller.artist?.name, 'Cached');
      expect(controller.artist?.provider, 'library');
    },
  );

  test(
    'cached artist and sections retain independent states after refresh failure',
    () async {
      PreferredProviderController.instance.initialize(
        preferredProvider: null,
        persist: (_) async {},
      );
      addTearDown(PreferredProviderController.instance.reset);
      final bridge = _ArtistBridge(deferSections: true);
      final controller = ArtistDetailController(
        coreBridge: bridge,
        itemId: '007',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadArtist();
      expect(controller.isLoadingMetadata, isTrue);
      bridge.emit('other', 'Wrong');
      expect(controller.artist, isNull);
      bridge.emit(
        '007',
        'Cached',
        mappings: [
          {
            'provider_domain': 'source',
            'provider_instance': 'source-a',
            'item_id': 'artist-a',
            'available': true,
          },
        ],
      );
      bridge.emitReadState(
        'Artist',
        '007',
        'library',
        phase: 'refreshing',
        knowledge: 'partial',
        freshness: 'unknown',
      );
      expect(bridge.metadata.isCompleted, isFalse);
      expect(controller.artist?.name, 'Cached');
      expect(controller.description, 'Cached biography');
      expect(controller.isLoadingMetadata, isFalse);
      expect(
        controller.metadataReadState?.knowledge,
        CatalogReadKnowledge.partial,
      );
      expect(controller.metadataReadState?.observedAt, isNull);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.albumRequests['source-a']!.isCompleted, isFalse);
      expect(bridge.trackRequests['source-a']!.isCompleted, isFalse);
      expect(bridge.similarRequests['source-a']!.isCompleted, isFalse);
      bridge.emitSection('ArtistAlbums', 'Albums', 'artist-a', 'source-a', [
        {'item_id': 'album-a', 'provider': 'source-a', 'name': 'Cached album'},
      ]);
      bridge.emitSection('ArtistTopTracks', 'Tracks', 'artist-a', 'source-a', [
        {'item_id': 'track-a', 'provider': 'source-a', 'name': 'Cached track'},
      ]);
      bridge.emitSection(
        'SimilarArtists',
        'Artists',
        'artist-a',
        'source-a',
        [],
        limit: 25,
      );
      const sections = [
        (CatalogQueryKind.artistAlbums, 'ArtistAlbums'),
        (CatalogQueryKind.artistTopTracks, 'ArtistTopTracks'),
        (CatalogQueryKind.similarArtists, 'SimilarArtists'),
      ];
      for (final (kind, query) in sections) {
        bridge.emitReadState(
          query,
          'artist-a',
          'source-a',
          phase: 'refreshing',
          knowledge: 'observed_response',
          freshness: 'stale',
          observedAt: 1700000000,
          limit: kind == CatalogQueryKind.similarArtists ? 25 : null,
        );
        expect(
          controller.sectionReadState(kind)?.phase,
          CatalogLoadPhase.refreshing,
        );
      }
      expect(controller.albums.single.name, 'Cached album');
      expect(controller.topTracks.single.name, 'Cached track');
      expect(controller.similarArtists, isEmpty);
      expect(controller.hasObservedSimilarArtists, isTrue);
      expect(controller.isLoadingSections, isFalse);
      bridge.emitReadState(
        'SimilarArtists',
        'artist-a',
        'source-a',
        phase: 'failed',
        knowledge: 'unknown',
        freshness: 'unknown',
        error: 'wrong limit',
        limit: 26,
      );
      expect(
        controller.sectionReadState(CatalogQueryKind.similarArtists)?.phase,
        CatalogLoadPhase.refreshing,
      );
      bridge.emitReadState(
        'Artist',
        '007',
        'library',
        phase: 'failed',
        knowledge: 'partial',
        freshness: 'unknown',
        error: 'offline',
      );
      for (final (kind, query) in sections) {
        bridge.emitReadState(
          query,
          'artist-a',
          'source-a',
          phase: 'failed',
          knowledge: 'observed_response',
          freshness: 'stale',
          observedAt: 1700000000,
          error: 'offline',
          limit: kind == CatalogQueryKind.similarArtists ? 25 : null,
        );
      }
      bridge.albumRequests['source-a']!.completeError(StateError('offline'));
      bridge.trackRequests['source-a']!.completeError(StateError('offline'));
      bridge.similarRequests['source-a']!.completeError(StateError('offline'));
      bridge.metadata.completeError(StateError('offline'));
      await loading;
      expect(controller.artist?.name, 'Cached');
      expect(controller.description, 'Cached biography');
      expect(controller.artist?.provider, 'library');
      expect(controller.albums.single.name, 'Cached album');
      expect(controller.topTracks.single.name, 'Cached track');
      expect(controller.hasObservedSimilarArtists, isTrue);
      expect(controller.metadataReadState?.phase, CatalogLoadPhase.failed);
      expect(
        controller.metadataReadState?.knowledge,
        CatalogReadKnowledge.partial,
      );
      expect(controller.metadataReadState?.freshness, CatalogFreshness.unknown);
      for (final (kind, _) in sections) {
        final state = controller.sectionReadState(kind)!;
        expect(state.phase, CatalogLoadPhase.failed);
        expect(state.knowledge, CatalogReadKnowledge.observedResponse);
        expect(state.freshness, CatalogFreshness.stale);
        expect(state.observedAt, 1700000000);
        expect(state.error, 'offline');
      }
      await controller.setPageVisible(false);
      bridge.emit('007', 'Hidden');
      expect(controller.artist?.name, 'Cached');
    },
  );

  test(
    'disposed artist controller ignores late metadata and read events',
    () async {
      final bridge = _ArtistBridge();
      addTearDown(bridge.updates.close);
      final controller = ArtistDetailController(
        coreBridge: bridge,
        itemId: '007',
        providerInstanceIdOrDomain: 'library',
      );
      var notificationCount = 0;
      controller.addListener(() => notificationCount++);
      final loading = controller.loadArtist();
      controller.dispose();
      final notificationsAtDispose = notificationCount;
      bridge.emit('007', 'Late');
      bridge.emitReadState(
        'Artist',
        '007',
        'library',
        phase: 'ready',
        knowledge: 'observed_response',
        freshness: 'fresh',
        observedAt: 1700000000,
      );
      bridge.metadata.complete(
        Artist.fromJson({
          'item_id': '007',
          'provider': 'library',
          'name': 'Late response',
        }),
      );
      await loading;
      expect(controller.artist, isNull);
      expect(controller.metadataReadState, isNull);
      expect(notificationCount, notificationsAtDispose);
      expect(bridge.albumRequests, isEmpty);
    },
  );
}

class _ArtistBridge implements MashinCoreBridgeApi {
  _ArtistBridge({this.deferSections = false});
  final bool deferSections;
  final updates = StreamController<Map<String, dynamic>>.broadcast(sync: true);
  var metadata = Completer<Artist>();
  int albumRequestCount = 0;
  final albumRequests = <String, Completer<List<Album>>>{};
  final trackRequests = <String, Completer<List<Track>>>{};
  final similarRequests = <String, Completer<List<Artist>>>{};

  @override
  Stream<Map<String, dynamic>> get events => updates.stream;

  // Synthetic catalog observations use the production event decoder.
  void emit(
    String itemId,
    String name, {
    List<Map<String, dynamic>> mappings = const [],
  }) {
    updates.add(<String, dynamic>{
      'type': 'catalog_updated',
      'query': <String, dynamic>{
        'Artist': <String, dynamic>{'item_id': itemId, 'provider': 'library'},
      },
      'value': <String, dynamic>{
        'Artist': <String, dynamic>{
          'media_type': 'artist',
          'item_id': itemId,
          'provider': 'library',
          'name': name,
          'metadata': <String, dynamic>{'description': 'Cached biography'},
          'provider_mappings': mappings,
        },
      },
    });
  }

  void emitSection(
    String query,
    String value,
    String itemId,
    String provider,
    List<Map<String, dynamic>> items, {
    int? limit,
  }) {
    updates.add({
      'type': 'catalog_updated',
      'query': {
        query: {
          'item_id': itemId,
          'provider': provider,
          if (limit != null) 'limit': limit,
        },
      },
      'value': {value: items},
    });
  }

  void emitReadState(
    String query,
    String itemId,
    String provider, {
    required String phase,
    required String knowledge,
    required String freshness,
    int? observedAt,
    String? error,
    int? limit,
  }) {
    updates.add({
      'type': 'catalog_read_changed',
      'query': {
        query: {
          'item_id': itemId,
          'provider': provider,
          if (limit != null) 'limit': limit,
        },
      },
      'snapshot': {
        'phase': phase,
        'knowledge': knowledge,
        'freshness': freshness,
        'observed_at': observedAt,
        'error': error,
      },
    });
  }

  @override
  Future<Artist> getArtist(String itemId, String provider) => metadata.future;

  @override
  Future<List<Album>> getArtistAlbums(
    String itemId,
    String provider, {
    bool isBackground = false,
  }) {
    albumRequestCount++;
    if (deferSections) {
      return (albumRequests[provider] = Completer<List<Album>>()).future;
    }
    return Future.value([]);
  }

  @override
  Future<List<Track>> getArtistTopTracks(String itemId, String provider) {
    if (deferSections)
      return (trackRequests[provider] = Completer<List<Track>>()).future;
    return Future.value([
      Track(
        itemId: 'track',
        provider: provider,
        name: 'Track',
        duration: 120,
        artists: const [],
      ),
    ]);
  }

  @override
  Future<List<Artist>> getSimilarArtists(
    String itemId,
    String provider, {
    int limit = 25,
  }) {
    if (deferSections)
      return (similarRequests[provider] = Completer<List<Artist>>()).future;
    return Future.value([]);
  }

  @override
  void log(LogLevelPreference level, String category, String message) {}

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
