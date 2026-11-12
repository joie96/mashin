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

  test('cached artist mapping starts albums before artist refresh', () async {
    final bridge = _AlbumBridge(
      shouldDelayTracks: true,
      shouldDelayArtists: true,
    );
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
    bridge.emit('Cached', hasArtist: true, hasMappings: true);
    await Future<void>.delayed(Duration.zero);
    bridge.emitArtist('library', 'artist', {
      'provider': 'library',
      'item_id': 'artist',
      'name': 'Artist',
      'provider_mappings': [
        {
          'provider_domain': 'source',
          'provider_instance': 'source-a',
          'item_id': 'artist-a',
          'available': true,
        },
      ],
    });
    await Future<void>.delayed(Duration.zero);
    expect(bridge.pendingArtists[('library', 'artist')]!.isCompleted, isFalse);
    expect(bridge.artistAlbumRequestCount, 1);
    bridge.artistAlbums.complete([]);
    bridge.pendingArtists[('library', 'artist')]!.completeError(
      StateError('offline artist'),
    );
    bridge.pendingTracks[('007', 'source-a')]!.complete([]);
    bridge.metadata.complete(controller.album!);
    await loading;
    expect(controller.isLoadingOtherAlbums, isFalse);
  });

  test('cached artist details enrich similar artists before refresh', () async {
    final bridge = _AlbumBridge(
      shouldDelayTracks: true,
      shouldDelaySimilarTracks: true,
      shouldDelayArtists: true,
    );
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
    bridge.emitTracks('library', '44', 'Track');
    await Future<void>.delayed(Duration.zero);
    bridge.emitRelatedTracks('SimilarTracks', 'library', 'track', [
      {
        'provider': 'library',
        'item_id': 'similar',
        'name': 'Similar track',
        'artists': [
          {
            'provider': 'library',
            'item_id': 'related',
            'name': 'Related artist',
          },
        ],
      },
    ]);
    await Future<void>.delayed(Duration.zero);
    bridge.emitArtist('library', 'related', {
      'provider': 'library',
      'item_id': 'related',
      'name': 'Cached full artist',
    });
    await Future<void>.delayed(Duration.zero);
    expect(controller.similarArtists.single.name, 'Cached full artist');
    expect(bridge.pendingArtists[('library', 'related')]!.isCompleted, isFalse);
    bridge.pendingArtists[('library', 'related')]!.completeError(
      StateError('offline artist'),
    );
    bridge.pendingSimilarTracks[('library', 'track')]!.completeError(
      StateError('offline similar'),
    );
    bridge.pendingTracks[('44', 'library')]!.complete(controller.tracks);
    bridge.metadata.complete(controller.album!);
    await loading;
    expect(controller.similarArtists.single.name, 'Cached full artist');
  });

  test(
    'changed album seed rejects late enrichment from the previous seed',
    () async {
      final bridge = _AlbumBridge(
        shouldDelayTracks: true,
        shouldDelaySimilarTracks: true,
        shouldDelayArtists: true,
      );
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
      bridge.emitTracks('library', '44', 'Track');
      await Future<void>.delayed(Duration.zero);
      bridge.emitRelatedTracks('SimilarTracks', 'library', 'track', [
        {
          'provider': 'library',
          'item_id': 'similar',
          'name': 'Similar track',
          'artists': [
            {
              'provider': 'library',
              'item_id': 'related',
              'name': 'Cached artist',
            },
          ],
        },
      ]);
      await Future<void>.delayed(Duration.zero);
      final cached = controller.similarArtists;
      bridge.pendingTracks[('44', 'library')]!.complete([
        Track.fromJson({
          'provider': 'source-b',
          'item_id': 'new-seed',
          'name': 'New seed',
        }),
      ]);
      await Future<void>.delayed(Duration.zero);
      expect(
        bridge.pendingSimilarTracks.containsKey(('source-b', 'new-seed')),
        isTrue,
      );
      expect(
        bridge.pendingSimilarTracks[('library', 'track')]!.isCompleted,
        isFalse,
      );
      bridge.pendingArtists[('library', 'related')]!.complete(
        Artist.fromJson({
          'provider': 'library',
          'item_id': 'related',
          'name': 'Late old enrichment',
        }),
      );
      await Future<void>.delayed(Duration.zero);
      expect(identical(controller.similarArtists, cached), isTrue);
      bridge.pendingSimilarTracks[('library', 'track')]!.complete([]);
      await Future<void>.delayed(Duration.zero);
      bridge.pendingSimilarTracks[('source-b', 'new-seed')]!.complete([]);
      bridge.metadata.complete(controller.album!);
      await loading;
      expect(controller.similarArtists, isEmpty);
    },
  );

  for (final retirement in ['hidden', 'disposed', 'invalidated']) {
    test('active similar reads ignore late values after $retirement', () async {
      final bridge = _AlbumBridge(
        shouldDelayTracks: true,
        shouldDelaySimilarTracks: true,
        shouldDelayArtists: true,
      );
      final controller = AlbumDetailController(
        coreBridge: bridge,
        itemId: '44',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        if (retirement != 'disposed') controller.dispose();
        await bridge.updates.close();
      });
      final loading = controller.loadAlbum();
      bridge.emit('Cached');
      await Future<void>.delayed(Duration.zero);
      bridge.emitTracks('library', '44', 'Track');
      await Future<void>.delayed(Duration.zero);
      bridge.emitRelatedTracks('SimilarTracks', 'library', 'track', [
        {
          'provider': 'library',
          'item_id': 'similar',
          'name': 'Similar track',
          'artists': [
            {
              'provider': 'library',
              'item_id': 'related',
              'name': 'Cached artist',
            },
          ],
        },
      ]);
      await Future<void>.delayed(Duration.zero);
      final header = controller.album!;
      final tracks = controller.tracks;
      final cached = controller.similarArtists;
      if (retirement == 'hidden') {
        await controller.setPageVisible(false);
      } else if (retirement == 'disposed') {
        controller.dispose();
      } else {
        bridge.emitPhase('invalidated', kind: 'SimilarTracks', itemId: 'track');
        expect(controller.album, isNull);
        expect(controller.similarArtists, isEmpty);
      }
      bridge.emitRelatedTracks('SimilarTracks', 'library', 'track', [
        {
          'provider': 'library',
          'item_id': 'late',
          'name': 'Late track',
          'artists': [
            {
              'provider': 'library',
              'item_id': 'late-artist',
              'name': 'Late artist',
            },
          ],
        },
      ]);
      bridge.pendingArtists[('library', 'related')]!.complete(
        Artist.fromJson({
          'provider': 'library',
          'item_id': 'related',
          'name': 'Late enrichment',
        }),
      );
      bridge.pendingSimilarTracks[('library', 'track')]!.complete([]);
      bridge.pendingTracks[('44', 'library')]!.complete(tracks);
      bridge.metadata.complete(header);
      await loading;
      expect(bridge.artistRequests, [('library', 'related')]);
      expect(bridge.versionRequests, isEmpty);
      if (retirement == 'invalidated') {
        expect(controller.similarArtists, isEmpty);
      } else {
        expect(identical(controller.similarArtists, cached), isTrue);
      }
    });
  }

  test('confirmed empty similar refresh replaces cached artists', () async {
    final bridge = _AlbumBridge(
      shouldDelayTracks: true,
      shouldDelaySimilarTracks: true,
    );
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
    bridge.emitTracks('library', '44', 'Track');
    await Future<void>.delayed(Duration.zero);
    bridge.emitRelatedTracks('SimilarTracks', 'library', 'track', [
      {
        'provider': 'library',
        'item_id': 'similar',
        'name': 'Similar track',
        'artists': [
          {
            'provider': 'library',
            'item_id': 'related',
            'name': 'Related artist',
          },
        ],
      },
    ]);
    await Future<void>.delayed(Duration.zero);
    expect(controller.similarArtists, hasLength(1));
    bridge.pendingSimilarTracks[('library', 'track')]!.complete([]);
    await Future<void>.delayed(Duration.zero);
    expect(controller.similarArtists, isEmpty);
    expect(bridge.versionRequests, [('library', 'track')]);
    bridge.pendingTracks[('44', 'library')]!.complete(controller.tracks);
    bridge.metadata.complete(controller.album!);
    await loading;
  });

  for (final primaryChanges in [false, true]) {
    test(
      'cached versions start fallback before refresh, primary changes=$primaryChanges',
      () async {
        final bridge = _AlbumBridge(
          shouldDelayTracks: true,
          shouldDelaySimilarTracks: true,
          shouldDelayVersions: true,
        );
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
        bridge.emitTracks('library', '44', 'Track');
        await Future<void>.delayed(Duration.zero);
        bridge.emitRelatedTracks('SimilarTracks', 'library', 'track', []);
        await Future<void>.delayed(Duration.zero);
        expect(bridge.versionRequests, [('library', 'track')]);
        final versions = [
          {'provider': 'source-b', 'item_id': 'version', 'name': 'Version'},
        ];
        bridge.emitRelatedTracks('TrackVersions', 'library', 'track', versions);
        await Future<void>.delayed(Duration.zero);
        expect(
          bridge.pendingVersions[('library', 'track')]!.isCompleted,
          isFalse,
        );
        expect(bridge.similarTrackRequests, [
          ('library', 'track'),
          ('source-b', 'version'),
        ]);
        bridge.emitRelatedTracks('TrackVersions', 'library', 'track', versions);
        bridge.emitRelatedTracks('SimilarTracks', 'source-b', 'version', [
          {
            'provider': 'source-b',
            'item_id': 'fallback',
            'name': 'Fallback track',
            'artists': [
              {
                'provider': 'source-b',
                'item_id': 'fallback-artist',
                'name': 'Related artist',
              },
            ],
          },
        ]);
        await Future<void>.delayed(Duration.zero);
        expect(controller.similarArtists.single.itemId, 'fallback-artist');
        expect(controller.isLoadingSimilarArtists, isFalse);
        expect(bridge.similarTrackRequestCount, 2);
        if (primaryChanges) {
          bridge.pendingSimilarTracks[('library', 'track')]!.complete([
            Track.fromJson({
              'provider': 'library',
              'item_id': 'primary',
              'name': 'Primary track',
              'artists': [
                {
                  'provider': 'library',
                  'item_id': 'primary-artist',
                  'name': 'Related artist',
                },
              ],
            }),
          ]);
          await Future<void>.delayed(Duration.zero);
          expect(controller.similarArtists.single.itemId, 'primary-artist');
        } else {
          bridge.pendingSimilarTracks[('library', 'track')]!.completeError(
            StateError('offline primary'),
          );
        }
        bridge.emitRelatedTracks('SimilarTracks', 'source-b', 'version', [
          {
            'provider': 'source-b',
            'item_id': 'late',
            'name': 'Late track',
            'artists': [
              {
                'provider': 'source-b',
                'item_id': 'late-artist',
                'name': 'Related artist',
              },
            ],
          },
        ]);
        await Future<void>.delayed(Duration.zero);
        expect(
          controller.similarArtists.single.itemId,
          primaryChanges ? 'primary-artist' : 'late-artist',
        );
        bridge.pendingSimilarTracks[('source-b', 'version')]!.completeError(
          StateError('offline fallback'),
        );
        bridge.pendingVersions[('library', 'track')]!.completeError(
          StateError('offline versions'),
        );
        bridge.pendingTracks[('44', 'library')]!.complete(controller.tracks);
        bridge.metadata.complete(controller.album!);
        await loading;
        expect(
          controller.similarArtists.single.itemId,
          primaryChanges ? 'primary-artist' : 'late-artist',
        );
      },
    );
  }

  test(
    'known empty versions end loading before their failed refresh',
    () async {
      final bridge = _AlbumBridge(
        shouldDelayTracks: true,
        shouldDelaySimilarTracks: true,
        shouldDelayVersions: true,
      );
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
      bridge.emitTracks('library', '44', 'Track');
      await Future<void>.delayed(Duration.zero);
      bridge.emitRelatedTracks('SimilarTracks', 'library', 'track', []);
      await Future<void>.delayed(Duration.zero);
      bridge.emitRelatedTracks('TrackVersions', 'library', 'track', []);
      await Future<void>.delayed(Duration.zero);
      expect(
        bridge.pendingVersions[('library', 'track')]!.isCompleted,
        isFalse,
      );
      expect(controller.similarArtists, isEmpty);
      expect(controller.isLoadingSimilarArtists, isFalse);
      bridge.pendingVersions[('library', 'track')]!.completeError(
        StateError('offline versions'),
      );
      bridge.pendingSimilarTracks[('library', 'track')]!.completeError(
        StateError('offline similar'),
      );
      bridge.pendingTracks[('44', 'library')]!.complete(controller.tracks);
      bridge.metadata.complete(controller.album!);
      await loading;
      expect(controller.similarArtists, isEmpty);
      expect(bridge.similarTrackRequestCount, 1);
    },
  );

  test(
    'similar observations match all query arguments and deduplicate terminal content',
    () async {
      final bridge = _AlbumBridge(
        shouldDelayTracks: true,
        shouldDelaySimilarTracks: true,
      );
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
      bridge.emitTracks('library', '44', 'Track');
      await Future<void>.delayed(Duration.zero);
      final tracks = [
        {
          'provider': 'library',
          'item_id': 'similar',
          'name': 'Similar track',
          'artists': [
            {
              'provider': 'library',
              'item_id': 'related',
              'name': 'Related artist',
            },
          ],
        },
      ];
      bridge.emitRelatedTracks('SimilarTracks', 'wrong', 'track', tracks);
      bridge.emitRelatedTracks('SimilarTracks', 'library', 'wrong', tracks);
      bridge.emitRelatedTracks(
        'SimilarTracks',
        'library',
        'track',
        tracks,
        limit: 25,
      );
      bridge.emitRelatedTracks(
        'SimilarTracks',
        'library',
        'track',
        tracks,
        allowLookup: false,
      );
      await Future<void>.delayed(Duration.zero);
      expect(controller.similarArtists, isEmpty);
      expect(bridge.artistRequests, isEmpty);
      bridge.emitRelatedTracks('SimilarTracks', 'library', 'track', tracks);
      await Future<void>.delayed(Duration.zero);
      final cached = controller.similarArtists;
      var changes = 0;
      controller.relatedListenable.addListener(() => changes++);
      bridge.emitRelatedTracks('SimilarTracks', 'library', 'track', tracks);
      bridge.pendingSimilarTracks[('library', 'track')]!.complete(
        tracks.map(Track.fromJson).toList(),
      );
      await Future<void>.delayed(Duration.zero);
      expect(identical(controller.similarArtists, cached), isTrue);
      expect(changes, 0);
      expect(bridge.artistRequests, [('library', 'related')]);
      bridge.pendingTracks[('44', 'library')]!.complete(controller.tracks);
      bridge.metadata.complete(controller.album!);
      await loading;
    },
  );

  test(
    'late artist enrichment cannot overwrite newer similar tracks',
    () async {
      final bridge = _AlbumBridge(
        shouldDelayTracks: true,
        shouldDelaySimilarTracks: true,
        shouldDelayArtists: true,
      );
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
      bridge.emitTracks('library', '44', 'Track');
      await Future<void>.delayed(Duration.zero);
      bridge.emitRelatedTracks('SimilarTracks', 'library', 'track', [
        {
          'provider': 'library',
          'item_id': 'old',
          'name': 'Old track',
          'artists': [
            {
              'provider': 'library',
              'item_id': 'old-artist',
              'name': 'Old artist',
            },
          ],
        },
      ]);
      await Future<void>.delayed(Duration.zero);
      expect(controller.similarArtists.single.name, 'Old artist');
      expect(controller.isLoadingSimilarArtists, isFalse);
      bridge.pendingSimilarTracks[('library', 'track')]!.complete([
        Track.fromJson({
          'provider': 'library',
          'item_id': 'new',
          'name': 'New track',
          'artists': [
            {
              'provider': 'library',
              'item_id': 'new-artist',
              'name': 'New artist',
            },
          ],
        }),
      ]);
      await Future<void>.delayed(Duration.zero);
      expect(controller.similarArtists.single.name, 'New artist');
      bridge.pendingArtists[('library', 'old-artist')]!.complete(
        Artist.fromJson({
          'provider': 'library',
          'item_id': 'old-artist',
          'name': 'Late old artist',
        }),
      );
      await Future<void>.delayed(Duration.zero);
      expect(controller.similarArtists.single.name, 'New artist');
      bridge.pendingArtists[('library', 'new-artist')]!.complete(
        Artist.fromJson({
          'provider': 'library',
          'item_id': 'new-artist',
          'name': 'Enriched new artist',
        }),
      );
      bridge.pendingTracks[('44', 'library')]!.complete(controller.tracks);
      bridge.metadata.complete(controller.album!);
      await loading;
      expect(controller.similarArtists.single.name, 'Enriched new artist');
    },
  );

  test(
    'cached similar tracks publish artists before refresh and survive failure',
    () async {
      final bridge = _AlbumBridge(
        shouldDelayTracks: true,
        shouldDelaySimilarTracks: true,
      );
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
      bridge.emitTracks('library', '44', 'Track');
      await Future<void>.delayed(Duration.zero);
      bridge.emitRelatedTracks('SimilarTracks', 'library', 'track', [
        {
          'provider': 'library',
          'item_id': 'similar',
          'name': 'Similar track',
          'artists': [
            {
              'provider': 'library',
              'item_id': 'related',
              'name': 'Related artist',
            },
          ],
        },
      ]);
      await Future<void>.delayed(Duration.zero);
      expect(
        bridge.pendingSimilarTracks[('library', 'track')]!.isCompleted,
        isFalse,
      );
      expect(controller.similarArtists.single.itemId, 'related');
      expect(controller.isLoadingSimilarArtists, isFalse);
      final cached = controller.similarArtists;
      bridge.pendingSimilarTracks[('library', 'track')]!.completeError(
        StateError('offline similar tracks'),
      );
      bridge.pendingTracks[('44', 'library')]!.complete(controller.tracks);
      bridge.metadata.complete(controller.album!);
      await loading;
      expect(identical(controller.similarArtists, cached), isTrue);
    },
  );

  test(
    'similar artist reads start from cached tracks before either refresh completes',
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
      bridge.emit('Cached', hasArtist: true);
      await Future<void>.delayed(Duration.zero);
      bridge.emitTracks('library', '44', 'Track');
      await Future<void>.delayed(Duration.zero);
      expect(bridge.pendingTracks[('44', 'library')]!.isCompleted, isFalse);
      expect(bridge.artistAlbums.isCompleted, isFalse);
      expect(bridge.similarTrackRequestCount, 1);
      expect(controller.isLoadingSimilarArtists, isFalse);
      bridge.pendingTracks[('44', 'library')]!.complete(controller.tracks);
      bridge.artistAlbums.complete([]);
      bridge.metadata.complete(controller.album!);
      await loading;
      expect(bridge.similarTrackRequestCount, 1);
    },
  );

  for (final hasCachedTracks in [true, false]) {
    for (final terminal in ['same', 'changed', 'empty', 'failed']) {
      test(
        'track seed refresh $terminal after cached tracks=$hasCachedTracks',
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
          bridge.emitTracks(
            'library',
            '44',
            'Cached track',
            isEmpty: !hasCachedTracks,
          );
          await Future<void>.delayed(Duration.zero);
          final cached = controller.tracks;
          expect(bridge.pendingTracks[('44', 'library')]!.isCompleted, isFalse);
          expect(
            bridge.similarTrackRequests,
            hasCachedTracks ? [('library', 'track')] : [],
          );
          expect(controller.isLoadingSimilarArtists, isFalse);
          bridge.emitTracks(
            'library',
            '44',
            'Cached track',
            isEmpty: !hasCachedTracks,
          );
          if (terminal == 'failed') {
            bridge.pendingTracks[('44', 'library')]!.completeError(
              StateError('offline tracks'),
            );
          } else {
            final refreshed = switch (terminal) {
              'same' => cached,
              'changed' => [
                Track.fromJson({
                  'provider': 'source-b',
                  'item_id': 'track',
                  'name': 'Refreshed track',
                }),
              ],
              _ => <Track>[],
            };
            bridge.pendingTracks[('44', 'library')]!.complete(refreshed);
          }
          bridge.metadata.complete(controller.album!);
          await loading;
          expect(bridge.similarTrackRequests, [
            if (hasCachedTracks) ('library', 'track'),
            if (terminal == 'changed') ('source-b', 'track'),
          ]);
          if (terminal == 'failed' || terminal == 'same') {
            expect(identical(controller.tracks, cached), isTrue);
          } else if (terminal == 'empty') {
            expect(controller.tracks, isEmpty);
          } else {
            expect(controller.tracks.single.provider, 'source-b');
          }
        },
      );
    }
  }

  test(
    'hidden album starts no follow-up after the first track event',
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
      await controller.setPageVisible(false);
      bridge.pendingTracks[('44', 'library')]!.complete(controller.tracks);
      bridge.metadata.complete(controller.album!);
      await loading;
      expect(bridge.similarTrackRequests, isEmpty);
    },
  );

  for (final hasCachedAlbums in [true, false]) {
    for (final terminal in ['failed', 'same', 'empty']) {
      test(
        'related albums publish ${hasCachedAlbums ? 'cached content' : 'known emptiness'} before $terminal refresh',
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
          bridge.emit('Cached', hasArtist: true);
          await Future<void>.delayed(Duration.zero);
          expect(bridge.artistAlbumRequestCount, 1);
          final event = <String, dynamic>{
            'type': 'catalog_updated',
            'query': {
              'ArtistAlbums': {'provider': 'library', 'item_id': 'artist'},
            },
            'value': {
              'Albums': hasCachedAlbums
                  ? [
                      {
                        'provider': 'library',
                        'item_id': '44',
                        'name': 'Current',
                        'year': 2026,
                      },
                      {
                        'provider': 'library',
                        'item_id': 'older',
                        'name': 'Older',
                        'year': 2001,
                      },
                      {
                        'provider': 'library',
                        'item_id': 'newer',
                        'name': 'Newer',
                        'year': 2020,
                      },
                    ]
                  : <Map<String, dynamic>>[],
            },
          };
          bridge.updates.add(event);
          expect(bridge.artistAlbums.isCompleted, isFalse);
          expect(controller.isLoadingOtherAlbums, isFalse);
          expect(
            controller.otherAlbums.map((album) => album.itemId),
            hasCachedAlbums ? ['newer', 'older'] : <String>[],
          );
          final cached = controller.otherAlbums;
          var changes = 0;
          controller.relatedListenable.addListener(() => changes++);
          bridge.updates.add(event);
          expect(identical(controller.otherAlbums, cached), isTrue);
          expect(changes, 0);
          expect(bridge.pendingTracks[('44', 'library')]!.isCompleted, isFalse);
          if (terminal == 'failed') {
            bridge.artistAlbums.completeError(
              StateError('offline related albums'),
            );
          } else {
            final albums = terminal == 'empty'
                ? <Album>[]
                : ((event['value'] as Map)['Albums'] as List)
                      .map(
                        (item) => Album.fromJson(item as Map<String, dynamic>),
                      )
                      .toList();
            bridge.artistAlbums.complete(albums);
          }
          await Future<void>.delayed(Duration.zero);
          expect(changes, terminal == 'empty' && hasCachedAlbums ? 1 : 0);
          bridge.pendingTracks[('44', 'library')]!.complete([]);
          bridge.metadata.completeError(StateError('offline header'));
          await loading;
          if (terminal == 'empty') {
            expect(controller.otherAlbums, isEmpty);
          } else {
            expect(identical(controller.otherAlbums, cached), isTrue);
          }
          expect(controller.isLoadingOtherAlbums, isFalse);
        },
      );
    }
  }

  test(
    'database clearing reloads visible album without clearing content and defers hidden reads',
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
      final album = controller.album!;
      bridge.metadata.complete(album);
      bridge.pendingTracks[('44', 'library')]!.complete(controller.tracks);
      await loading;
      bridge.updates.add({'type': 'cache_cleared', 'cache': 'images'});
      await Future<void>.delayed(Duration.zero);
      expect(bridge.trackRequestCount, 1);
      bridge.updates.add({'type': 'provider_instances_changed'});
      await Future<void>.delayed(Duration.zero);
      expect(bridge.trackRequestCount, 2);
      expect(identical(controller.album, album), isTrue);
      expect(controller.tracks.single.name, 'Track');
      bridge.pendingTracks[('44', 'library')]!.complete(controller.tracks);
      await Future<void>.delayed(Duration.zero);
      await controller.setPageVisible(false);
      bridge.updates.add({'type': 'cache_cleared', 'cache': 'database'});
      await Future<void>.delayed(Duration.zero);
      expect(bridge.trackRequestCount, 2);
      final returning = controller.setPageVisible(true);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.trackRequestCount, 3);
      bridge.pendingTracks[('44', 'library')]!.complete(controller.tracks);
      await returning;
    },
  );

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
      expect(bridge.similarTrackRequests, isEmpty);
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
      expect(bridge.similarTrackRequests, isEmpty);
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
  _AlbumBridge({
    this.shouldDelayTracks = false,
    this.shouldDelaySimilarTracks = false,
    this.shouldDelayVersions = false,
    this.shouldDelayArtists = false,
  });
  final bool shouldDelayTracks;
  final bool shouldDelaySimilarTracks;
  final bool shouldDelayVersions;
  final bool shouldDelayArtists;
  final pendingSimilarTracks = <(String, String), Completer<List<Track>>>{};
  final pendingVersions = <(String, String), Completer<List<Track>>>{};
  final versionRequests = <(String, String)>[];
  final pendingArtists = <(String, String), Completer<Artist>>{};
  final artistRequests = <(String, String)>[];
  final updates = StreamController<Map<String, dynamic>>.broadcast(sync: true);
  final metadata = Completer<Album>();
  final artistAlbums = Completer<List<Album>>();
  int artistAlbumRequestCount = 0;
  final similarTrackRequests = <(String, String)>[];
  int get similarTrackRequestCount => similarTrackRequests.length;
  final pendingTracks = <(String, String), Completer<List<Track>>>{};
  int trackRequestCount = 0;

  @override
  Stream<Map<String, dynamic>> get events => updates.stream;

  // Synthetic observations exercise the production catalog event decoder.
  void emit(String name, {bool hasMappings = false, bool hasArtist = false}) {
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
          if (hasArtist)
            'artists': [
              {'provider': 'library', 'item_id': 'artist', 'name': 'Artist'},
            ],
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

  void emitArtist(String provider, String itemId, Map<String, dynamic> artist) {
    updates.add({
      'type': 'catalog_updated',
      'query': {
        'Artist': {'provider': provider, 'item_id': itemId},
      },
      'value': {'Artist': artist},
    });
  }

  void emitTracks(
    String provider,
    String itemId,
    String name, {
    bool isEmpty = false,
  }) {
    updates.add({
      'type': 'catalog_updated',
      'query': {
        'AlbumTracks': {'provider': provider, 'item_id': itemId},
      },
      'value': {
        'Tracks': isEmpty
            ? <Map<String, dynamic>>[]
            : [
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

  void emitRelatedTracks(
    String kind,
    String provider,
    String itemId,
    List<Map<String, dynamic>> tracks, {
    int limit = 50,
    bool allowLookup = true,
  }) {
    updates.add({
      'type': 'catalog_updated',
      'query': {
        kind: {
          'provider': provider,
          'item_id': itemId,
          if (kind == 'SimilarTracks') 'limit': limit,
          if (kind == 'SimilarTracks') 'allow_lookup': allowLookup,
        },
      },
      'value': {'Tracks': tracks},
    });
  }

  void emitPhase(String phase, {String kind = 'Album', String itemId = '44'}) {
    updates.add({
      'type': 'catalog_read_changed',
      'query': {
        kind: {'item_id': itemId, 'provider': 'library'},
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
  Future<List<Album>> getArtistAlbums(
    String itemId,
    String provider, {
    bool isBackground = false,
  }) {
    artistAlbumRequestCount++;
    return artistAlbums.future;
  }

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
  Future<List<Track>> getSimilarTracks(
    String itemId,
    String provider, {
    int limit = 50,
    bool allowLookup = true,
  }) async {
    similarTrackRequests.add((provider, itemId));
    if (shouldDelaySimilarTracks) {
      final pending = Completer<List<Track>>();
      pendingSimilarTracks[(provider, itemId)] = pending;
      return pending.future;
    }
    return [];
  }

  @override
  Future<Artist> getArtist(String itemId, String provider) async {
    artistRequests.add((provider, itemId));
    if (shouldDelayArtists) {
      final pending = Completer<Artist>();
      pendingArtists[(provider, itemId)] = pending;
      return pending.future;
    }
    return Artist.fromJson({
      'provider': provider,
      'item_id': itemId,
      'name': 'Related artist',
    });
  }

  @override
  Future<List<Track>> getTrackVersions(String itemId, String provider) async {
    versionRequests.add((provider, itemId));
    if (shouldDelayVersions) {
      final pending = Completer<List<Track>>();
      pendingVersions[(provider, itemId)] = pending;
      return pending.future;
    }
    return [];
  }

  @override
  void log(LogLevelPreference level, String category, String message) {}

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
