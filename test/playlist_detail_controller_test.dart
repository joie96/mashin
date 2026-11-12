import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/session/preferred_provider_controller.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/core/core_query_scope.dart';
import 'package:mashin/features/playlist/playlist_detail_controller.dart';

void main() {
  setUp(() {
    PreferredProviderController.instance.initialize(
      preferredProvider: null,
      persist: (_) async {},
    );
  });
  tearDown(PreferredProviderController.instance.reset);

  for (final retirement in ['hidden', 'disposed', 'invalidated']) {
    test(
      'playlist album enrichment ignores late results after $retirement',
      () async {
        final bridge = _PlaylistBridge();
        final controller = PlaylistDetailController(
          coreBridge: bridge,
          itemId: '44',
          providerInstanceIdOrDomain: 'library',
        );
        addTearDown(() async {
          if (retirement != 'disposed') controller.dispose();
          await bridge.updates.close();
        });
        final loading = controller.loadPlaylist();
        await bridge.requested.future;
        bridge.emitTracks(withAlbum: true);
        await Future<void>.delayed(Duration.zero);
        bridge.emitAlbums(2020);
        await Future<void>.delayed(Duration.zero);
        final cached = controller.tracks;
        if (retirement == 'hidden') {
          await controller.setPageVisible(false);
        } else if (retirement == 'disposed') {
          controller.dispose();
        } else {
          bridge.emitPhase(
            'invalidated',
            kind: 'ArtistAlbums',
            itemId: 'artist',
            provider: 'source-a',
          );
          expect(controller.playlist, isNull);
        }
        bridge.emitAlbums(2040);
        bridge.albums.completeError(StateError('late albums'));
        bridge.tracks.completeError(StateError('late tracks'));
        await loading;
        await Future<void>.delayed(Duration.zero);
        if (retirement == 'invalidated') {
          expect(controller.tracks, isEmpty);
        } else {
          expect(identical(controller.tracks, cached), isTrue);
        }
      },
    );
  }

  test(
    'all playlist artists expose cache values while their refreshes remain pending',
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
      bridge.emitTracks(withAlbum: true, artistCount: 5);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.albumRequestCount, 5);
      bridge.emitAlbums(1900, artistId: 'unrelated');
      expect(controller.tracks.first.album?.year, isNull);
      bridge.emitAlbums(2020, artistId: 'artist-4');
      await Future<void>.delayed(Duration.zero);
      expect(controller.tracks.first.album?.year, 2020);
      bridge.emitAlbums(null, artistId: 'artist-4');
      await Future<void>.delayed(Duration.zero);
      expect(controller.tracks.first.album?.year, isNull);
      bridge.emitAlbums(2024, artistId: 'artist-4');
      await Future<void>.delayed(Duration.zero);
      final cached = controller.tracks;
      bridge.albums.completeError(StateError('offline albums'));
      bridge.tracks.completeError(StateError('offline tracks'));
      await loading;
      await Future<void>.delayed(Duration.zero);
      expect(identical(controller.tracks, cached), isTrue);
    },
  );

  test(
    'playlist enriches cached tracks from cached artist albums before either refresh',
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
      bridge.emitTracks(withAlbum: true);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.albumRequestCount, 1);
      expect(bridge.tracks.isCompleted, isFalse);
      for (final year in [2020, 2024]) {
        bridge.emitAlbums(year);
        await Future<void>.delayed(Duration.zero);
        expect(controller.tracks.first.album?.year, year);
        expect(controller.tracks.map((track) => track.position), [1, 4]);
      }
      final cached = controller.tracks;
      bridge.albums.completeError(StateError('offline albums'));
      bridge.tracks.completeError(StateError('offline tracks'));
      await loading;
      await Future<void>.delayed(Duration.zero);
      expect(identical(controller.tracks, cached), isTrue);
    },
  );

  test('track updates retain artist reads and observed album years', () async {
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
    bridge.emitTracks(withAlbum: true);
    await Future<void>.delayed(Duration.zero);
    bridge.emitAlbums(2024);
    await Future<void>.delayed(Duration.zero);
    bridge.emitTracks(name: 'Refreshed', withAlbum: true);
    await Future<void>.delayed(Duration.zero);
    expect(bridge.albumRequestCount, 1);
    expect(controller.tracks.first.name, 'Refreshed');
    expect(controller.tracks.first.album?.year, 2024);
    bridge.albums.completeError(StateError('offline albums'));
    bridge.tracks.completeError(StateError('offline tracks'));
    await loading;
  });

  test(
    'album bursts publish once and confirmed absence restores source tracks',
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
      bridge.emitTracks(withAlbum: true);
      await Future<void>.delayed(Duration.zero);
      var publications = 0;
      controller.addListener(() => publications++);
      bridge.emitAlbums(2020);
      bridge.emitAlbums(2022);
      bridge.emitAlbums(2024);
      await Future<void>.delayed(Duration.zero);
      expect(publications, 1);
      expect(controller.tracks.map((track) => track.album?.year), [2024, 2024]);
      expect(controller.tracks.map((track) => track.position), [1, 4]);
      bridge.emitAlbums(2024);
      await Future<void>.delayed(Duration.zero);
      expect(publications, 1);
      bridge.emitAlbums(null);
      await Future<void>.delayed(Duration.zero);
      expect(publications, 2);
      expect(
        controller.tracks.every((track) => track.album?.year == null),
        isTrue,
      );
      bridge.albums.completeError(StateError('offline albums'));
      bridge.tracks.completeError(StateError('offline tracks'));
      await loading;
    },
  );

  test(
    'large playlists yield between cache reads without awaiting refreshes',
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
      bridge.emitTracks(withAlbum: true, artistCount: 217);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.albumRequestCount, inInclusiveRange(1, 8));
      for (var turn = 0; turn < 30; turn++) {
        await Future<void>.delayed(Duration.zero);
      }
      expect(bridge.albumRequestCount, 217);
      expect(bridge.albums.isCompleted, isFalse);
      bridge.emitAlbums(2024, artistId: 'artist-216');
      await Future<void>.delayed(Duration.zero);
      expect(controller.tracks.first.album?.year, 2024);
      await controller.setPageVisible(false);
      bridge.albums.completeError(StateError('retired albums'));
      bridge.tracks.completeError(StateError('retired tracks'));
      await loading;
    },
  );

  test('track membership changes cancel only retired artist reads', () async {
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
    bridge.emitTracks(withAlbum: true, artistCount: 5);
    await Future<void>.delayed(Duration.zero);
    bridge.emitAlbums(2024, artistId: 'artist-4');
    await Future<void>.delayed(Duration.zero);
    expect(controller.tracks.first.album?.year, 2024);
    bridge.emitTracks(name: 'Refreshed', withAlbum: true, artistCount: 4);
    await Future<void>.delayed(Duration.zero);
    expect(bridge.albumRequestCount, 5);
    expect(bridge.canceledArtists, ['artist-4']);
    expect(controller.tracks.first.album?.year, isNull);
    bridge.emitAlbums(2040, artistId: 'artist-4');
    await Future<void>.delayed(Duration.zero);
    expect(controller.tracks.first.album?.year, isNull);
    await controller.setPageVisible(false);
    expect(bridge.canceledArtists.toSet(), {
      'artist-0',
      'artist-1',
      'artist-2',
      'artist-3',
      'artist-4',
    });
    bridge.albums.completeError(StateError('retired albums'));
    bridge.tracks.completeError(StateError('retired tracks'));
    await loading;
  });

  for (final shouldFailRefresh in [false, true]) {
    test(
      'cached playlist header starts tracks before metadata refresh, failure=$shouldFailRefresh',
      () async {
        final metadata = Completer<Playlist>();
        final bridge = _PlaylistBridge(metadata: metadata);
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
        await bridge.metadataRequested.future;
        bridge.emitHeader('Cached header');
        await bridge.requested.future;
        bridge.emitTracks();
        expect(metadata.isCompleted, isFalse);
        expect(controller.playlistName, 'Cached header');
        expect(controller.isLoadingMetadata, isFalse);
        expect(controller.tracks, hasLength(2));
        final header = controller.playlist;
        var changes = 0;
        controller.addListener(() => changes++);
        bridge.emitHeader('Cached header');
        expect(identical(controller.playlist, header), isTrue);
        expect(changes, 0);
        if (shouldFailRefresh) {
          metadata.completeError(StateError('offline'));
        } else {
          metadata.complete(
            Playlist.fromJson({
              'item_id': '44',
              'provider': 'library',
              'name': 'Server header',
            }),
          );
        }
        bridge.tracks.completeError(StateError('offline'));
        await loading;
        expect(
          controller.playlistName,
          shouldFailRefresh ? 'Cached header' : 'Server header',
        );
        expect(controller.tracks, hasLength(2));
      },
    );
  }

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

  for (final kind in ['Playlist', 'PlaylistTracks']) {
    test(
      '$kind invalidation rejects pending completions and late events',
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
        bridge.emitPhase('invalidated', kind: kind);
        bridge.emitTracks(name: 'Late');
        bridge.tracks.complete(tracks);
        await loading;
        expect(controller.tracks, isEmpty);
        expect(controller.isLoadingTracks, isFalse);
      },
    );
  }
}

class _PlaylistBridge implements MashinCoreBridgeApi {
  _PlaylistBridge({this.metadata});

  final Completer<Playlist>? metadata;
  final updates = StreamController<Map<String, dynamic>>.broadcast(sync: true);
  final tracks = Completer<List<Track>>();
  final albums = Completer<List<Album>>();
  int albumRequestCount = 0;
  final List<String> canceledArtists = [];
  final requested = Completer<void>();
  final metadataRequested = Completer<void>();

  @override
  Stream<Map<String, dynamic>> get events => updates.stream;

  void emitHeader(String name) {
    updates.add({
      'type': 'catalog_updated',
      'query': {
        'Playlist': {'item_id': '44', 'provider': 'library'},
      },
      'value': {
        'Playlist': {'item_id': '44', 'provider': 'library', 'name': name},
      },
    });
  }

  void emitAlbums(int? year, {String artistId = 'artist'}) {
    updates.add({
      'type': 'catalog_updated',
      'query': {
        'ArtistAlbums': {'provider': 'source-a', 'item_id': artistId},
      },
      'value': {
        'Albums': [
          if (year != null)
            {
              'provider': 'source-a',
              'item_id': 'album',
              'name': 'Album',
              'year': year,
            },
        ],
      },
    });
  }

  void emitTracks({
    String name = 'Cached',
    bool allowDynamicTracks = false,
    bool withAlbum = false,
    int artistCount = 1,
  }) {
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
              if (withAlbum)
                'album': {
                  'provider': 'source-a',
                  'item_id': 'album',
                  'name': 'Album',
                },
              if (withAlbum)
                'artists': [
                  for (var index = 0; index < artistCount; index++)
                    {
                      'provider': 'source-a',
                      'item_id': artistCount == 1 ? 'artist' : 'artist-$index',
                      'name': 'Artist',
                    },
                ],
            },
        ],
      },
    });
  }

  void emitPhase(
    String phase, {
    String kind = 'PlaylistTracks',
    String itemId = '44',
    String provider = 'library',
  }) {
    updates.add({
      'type': 'catalog_read_changed',
      'query': {
        kind: {
          'item_id': itemId,
          'provider': provider,
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
  Future<Playlist> getPlaylist(String itemId, String provider) async {
    metadataRequested.complete();
    return metadata?.future ??
        Future.value(
          Playlist.fromJson({
            'item_id': itemId,
            'provider': provider,
            'name': 'Library header',
          }),
        );
  }

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
  Future<List<Album>> getArtistAlbums(
    String itemId,
    String provider, {
    bool isBackground = false,
  }) {
    expect(isBackground, isTrue);
    albumRequestCount++;
    return CoreQueryScope.current!.execute(
      () => albums.future,
      onCancel: () => canceledArtists.add(itemId),
    );
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
