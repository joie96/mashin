// ignore_for_file: prefer_const_constructors
import 'dart:async';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/core/core_userdata.dart';
import 'package:mashin/features/artist/artist_detail_controller.dart';
import 'package:mashin/features/album/album_detail_controller.dart';
import 'package:mashin/features/playlist/playlist_detail_controller.dart';
import 'package:mashin/features/home/home_controller.dart';
import 'package:mashin/features/explore/explore_controller.dart';

class _CatalogEventBridge extends _UnusedCoreBridge {
  final updates = StreamController<Map<String, dynamic>>.broadcast(sync: true);

  @override
  Stream<Map<String, dynamic>> get events => updates.stream;

  @override
  void log(LogLevelPreference level, String category, String message) {}
}

class _UnusedCoreBridge extends Fake implements MashinCoreBridgeApi {
  @override
  Stream<Map<String, dynamic>> get events => const Stream.empty();
}

class _RecommendationEventBridge extends _CatalogEventBridge {
  String? trackName = 'Cached';
  final refreshes = <bool>[];
  var recentReads = 0;

  @override
  Future<List<MediaItem>> getRecentlyPlayedTracks({int limit = 40}) async {
    recentReads++;
    return [];
  }

  @override
  Future<RecommendationSnapshot> getRecommendations({
    required RecommendationView view,
    bool refresh = true,
  }) async {
    refreshes.add(refresh);
    final tracks = <Map<String, dynamic>>[
      if (trackName != null)
        _mediaJson('track')
          ..['name'] = trackName
          ..['is_playable'] = true,
    ];
    return RecommendationSnapshot.fromJson(<String, dynamic>{
      'is_loading': false,
      'has_load_error': false,
      'folders': <Map<String, dynamic>>[
        <String, dynamic>{
          'item_id': view == RecommendationView.home
              ? 'user_recommendations'
              : 'user_exploration_weekly',
          'provider': 'test',
          'name': 'Recommendations',
          'items': tracks,
        },
      ],
    });
  }
}

class _PendingUserDataBridge extends _CatalogEventBridge {
  final initial = Completer<Map<String, dynamic>>();
  final snapshots = <Completer<Map<String, dynamic>>>[];
  var initialReads = 0;

  @override
  Future<Map<String, dynamic>> loadUserData() {
    initialReads++;
    return initial.future;
  }

  @override
  Future<Map<String, dynamic>> getUserData() {
    final snapshot = Completer<Map<String, dynamic>>();
    snapshots.add(snapshot);
    return snapshot.future;
  }

  Map<String, dynamic> createSnapshot({
    bool hasItems = true,
    bool isLoading = false,
    int revision = 0,
  }) => {
    'revision': revision,
    'has_loaded_favorites': true,
    'is_loading_favorites': isLoading,
    'is_loading_playlists': isLoading,
    'favorites': {
      'tracks': [if (hasItems) _mediaJson('track')],
    },
    'playlists': [
      if (hasItems) _mediaJson('playlist')..['is_loading_tracks'] = isLoading,
    ],
  };
}

class _FavoriteSnapshotBridge extends _UnusedCoreBridge {
  bool hasFavorite = true;

  @override
  Future<Map<String, dynamic>> getUserData() => loadUserData();

  @override
  Future<Map<String, dynamic>> loadUserData() async => <String, dynamic>{
    'has_loaded_favorites': true,
    'favorites': <String, dynamic>{
      'tracks': <Map<String, dynamic>>[
        if (hasFavorite)
          _mediaJson('track')
            ..['favorite'] = true
            ..['uri'] = 'library://track/track-id',
      ],
    },
    'playlists': <Map<String, dynamic>>[],
  };
}

Map<String, dynamic> _mediaJson(String mediaType) => <String, dynamic>{
  'media_type': mediaType,
  'item_id': '$mediaType-id',
  'provider': 'library',
  'name': mediaType,
  'version': 'deluxe',
  'external_ids': <List<String>>[
    <String>['isrc', 'US-TEST-01'],
  ],
  'is_playable': false,
  'position': 4,
  'date_added': '2026-09-14T12:00:00Z',
  'provider_mappings': <Map<String, dynamic>>[
    <String, dynamic>{
      'item_id': 'source-id',
      'provider_domain': 'source',
      'provider_instance': 'source--one',
      'in_library': true,
      'is_unique': false,
      'audio_format': <String, dynamic>{
        'content_type': 'flac',
        'codec_type': 'pcm',
        'sample_rate': 96000,
        'bit_depth': 24,
        'channels': 2,
        'output_format_str': 'flac',
        'bit_rate': 1200,
      },
      'details': 'lossless',
    },
  ],
  'metadata': <String, dynamic>{
    'description_language': 'de',
    'explicit': true,
    'lyrics': 'lyrics',
    'transcript': 'transcript',
  },
};

void main() {
  test(
    'recent tracks decode typed catalog values and tagged completion values',
    () {
      final track = <String, dynamic>{
        'provider': 'source',
        'item_id': '007',
        'name': 'Cached track',
        'image': null,
        'available': true,
      };
      expect(
        () => mediaItemFromJson(track),
        throwsA(
          isA<FormatException>().having(
            (error) => error.message,
            'message',
            'Unknown Music Assistant media type: null',
          ),
        ),
      );
      for (final item in [
        track,
        {...track, 'media_type': 'track'},
      ]) {
        final event = <String, dynamic>{
          'type': 'catalog_updated',
          'query': {
            'RecentlyPlayed': {'limit': 40},
          },
          'value': {
            'Tracks': [item],
          },
        };
        final update = CatalogUpdate.fromJson(event);
        final decoded = tracksFromJson((event['value'] as Map).values.single);
        expect(update.kind, CatalogQueryKind.recentlyPlayed);
        expect(update.limit, 40);
        expect(decoded.single.itemId, '007');
        expect(decoded.single.provider, 'source');
        expect(decoded.single.name, 'Cached track');
        expect(hasSameMediaListContent(decoded, update.tracks), isTrue);
      }
      expect(tracksFromJson([]), isEmpty);
      expect(() => tracksFromJson({}), throwsFormatException);
    },
  );

  test(
    'home and explore separate media invalidations and stop after disposal',
    () async {
      for (final view in RecommendationView.values) {
        final bridge = _RecommendationEventBridge();
        final home = HomeController(coreBridge: bridge);
        final explore = ExploreController(coreBridge: bridge);
        if (view == RecommendationView.home) {
          await home.load();
        } else {
          await explore.load();
        }
        await Future<void>.delayed(Duration.zero);
        List<Track> tracks() => view == RecommendationView.home
            ? home.recommendationItems
            : explore.weeklyExploration;
        expect(tracks().single.name, 'Cached');
        bridge.updates.add(<String, dynamic>{
          'type': 'cache_cleared',
          'cache': 'images',
        });
        await Future<void>.delayed(Duration.zero);
        expect(bridge.refreshes, <bool>[true]);
        bridge.trackName = 'Fresh';
        bridge.updates.add(<String, dynamic>{
          'type': 'cache_cleared',
          'cache': 'database',
        });
        expect(tracks().single.name, 'Cached');
        await Future<void>.delayed(Duration.zero);
        expect(tracks().single.name, 'Fresh');
        expect(bridge.refreshes, <bool>[true, false]);
        final recentReads = bridge.recentReads;
        bridge.updates.add(<String, dynamic>{
          'type': 'provider_instances_changed',
        });
        await Future<void>.delayed(Duration.zero);
        expect(
          bridge.recentReads,
          recentReads + (view == RecommendationView.home ? 1 : 0),
        );
        expect(tracks().single.name, 'Fresh');
        expect(bridge.refreshes, <bool>[true, false]);
        final readsBeforeDisposal = bridge.recentReads;
        home.dispose();
        explore.dispose();
        bridge.updates.add(<String, dynamic>{
          'type': 'cache_cleared',
          'cache': 'database',
        });
        bridge.updates.add(<String, dynamic>{
          'type': 'provider_instances_changed',
        });
        await Future<void>.delayed(Duration.zero);
        expect(bridge.refreshes, <bool>[true, false]);
        expect(bridge.recentReads, readsBeforeDisposal);
        await bridge.updates.close();
      }
    },
  );
  test(
    'provider updates decode and complete replacements suppress identical notifications',
    () {
      ProviderManifestRegistry.clear();
      final update = CatalogUpdate.fromJson(<String, dynamic>{
        'query': <String, dynamic>{'ProviderManifests': <String, dynamic>{}},
        'value': <String, dynamic>{
          'ProviderManifests': <Map<String, dynamic>>[
            <String, dynamic>{
              'domain': 'builtin',
              'name': 'Built in',
              'icon_images': <String>['default'],
            },
          ],
        },
      });
      expect(update.kind, CatalogQueryKind.providerManifests);
      ProviderManifestRegistry.replaceAll(update.providerManifests);
      final revision = ProviderManifestRegistry.changes.value;
      ProviderManifestRegistry.replaceAll(
        update.providerManifests.map(
          (manifest) => ProviderManifest.fromJson(manifest.raw),
        ),
      );
      expect(ProviderManifestRegistry.changes.value, revision);
      expect(
        ProviderManifestRegistry.getByProvider('builtin')?.hasDefaultIcon,
        isTrue,
      );
      ProviderManifestRegistry.replaceAll(const <ProviderManifest>[]);
      expect(ProviderManifestRegistry.getByProvider('builtin'), isNull);
      ProviderManifestRegistry.clear();
    },
  );
  test(
    'server read success preserves a separate cache persistence failure',
    () {
      final change = CatalogReadChange.fromJson({
        'query': {
          'Album': {'item_id': '44', 'provider': 'library'},
        },
        'snapshot': {
          'phase': 'ready',
          'source': 'server',
          'knowledge': 'observed_response',
          'freshness': 'fresh',
          'observed_at': 1700000000,
          'error': null,
          'cache_error': 'synthetic write failure',
        },
      });
      expect(change.kind, CatalogQueryKind.album);
      expect(change.phase, CatalogLoadPhase.ready);
      expect(change.source, CatalogDataSource.server);
      expect(change.error, isNull);
      expect(change.cacheError, 'synthetic write failure');
    },
  );

  test('search read states preserve query, category and limit identity', () {
    final change = CatalogReadChange.fromJson({
      'query': {
        'Search': {'query': 'jazz', 'media_type': 'track', 'limit': 50},
      },
      'snapshot': {
        'phase': 'invalidated',
        'knowledge': 'unknown',
        'freshness': 'unknown',
      },
    });
    expect(change.kind, CatalogQueryKind.search);
    expect(change.searchQuery, 'jazz');
    expect(change.mediaType, MediaType.track);
    expect(change.limit, 50);
  });

  test(
    'catalog read states preserve unknown, partial, empty and invalidated observations',
    () {
      for (final phase in [
        'reading_local',
        'refreshing',
        'ready',
        'failed',
        'invalidated',
      ]) {
        for (final knowledge in ['unknown', 'partial', 'observed_response']) {
          final change = CatalogReadChange.fromJson({
            'query': {
              'Artist': {'item_id': '007', 'provider': 'source-a'},
            },
            'snapshot': {
              'phase': phase,
              'knowledge': knowledge,
              'freshness': 'unknown',
              'observed_at': null,
              'error': phase == 'failed' ? 'offline' : null,
            },
          });
          expect(change.kind, CatalogQueryKind.artist);
          expect(change.observedAt, isNull);
          expect(
            change.knowledge,
            knowledge == 'observed_response'
                ? CatalogReadKnowledge.observedResponse
                : CatalogReadKnowledge.values.byName(knowledge),
          );
        }
      }
      final empty = CatalogUpdate.fromJson({
        'query': {
          'SimilarArtists': {
            'item_id': '007',
            'provider': 'source-a',
            'limit': 25,
          },
        },
        'value': {'Artists': []},
      });
      expect(empty.artists, isEmpty);
      expect(empty.limit, 25);
      expect(
        () => CatalogReadChange.fromJson({
          'query': {
            'Artist': {'item_id': '007', 'provider': 'source-a'},
          },
          'snapshot': {
            'phase': 'ready',
            'knowledge': 'complete',
            'freshness': 'fresh',
          },
        }),
        throwsFormatException,
      );
    },
  );

  test(
    'provider item artist events preserve the requested and canonical identities',
    () {
      final update = CatalogUpdate.fromJson({
        'query': {
          'ProviderItem': {
            'item_id': '007',
            'provider': 'source-a',
            'media_type': 'artist',
          },
        },
        'value': {'Item': _mediaJson('artist')},
      });
      expect(update.kind, CatalogQueryKind.providerItem);
      expect(update.itemId, '007');
      expect(update.provider, 'source-a');
      expect(update.item, isA<Artist>());
      expect(update.item!.provider, 'library');
    },
  );

  test(
    'home and explore apply changed and empty recommendations without refresh loops',
    () async {
      for (final view in RecommendationView.values) {
        final bridge = _RecommendationEventBridge();
        final home = HomeController(coreBridge: bridge);
        final explore = ExploreController(coreBridge: bridge);
        if (view == RecommendationView.home) {
          await home.load();
        } else {
          await explore.load();
        }
        await Future<void>.delayed(Duration.zero);
        List<Track> tracks() => view == RecommendationView.home
            ? home.recommendationItems
            : explore.weeklyExploration;
        expect(tracks().single.name, 'Cached');
        bridge.trackName = 'Fresh';
        bridge.updates.add(<String, dynamic>{
          'type': 'recommendations_changed',
        });
        await Future<void>.delayed(Duration.zero);
        expect(tracks().single.name, 'Fresh');
        bridge.trackName = null;
        bridge.updates.add(<String, dynamic>{
          'type': 'recommendations_changed',
        });
        await Future<void>.delayed(Duration.zero);
        expect(tracks(), isEmpty);
        expect(bridge.refreshes, <bool>[true, false, false]);
        home.dispose();
        explore.dispose();
        await bridge.updates.close();
      }
    },
  );

  test(
    'catalog updates replace matching detail sections without new requests',
    () async {
      final bridge = _CatalogEventBridge();
      final artist = ArtistDetailController(
        coreBridge: bridge,
        itemId: 'artist-id',
        providerInstanceIdOrDomain: 'library',
      );
      final album = AlbumDetailController(
        coreBridge: bridge,
        itemId: 'album-id',
        providerInstanceIdOrDomain: 'library',
      );
      final playlist = PlaylistDetailController(
        coreBridge: bridge,
        itemId: 'playlist-id',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(() async {
        artist.dispose();
        album.dispose();
        playlist.dispose();
        await bridge.updates.close();
      });
      void emit(
        String query,
        String id,
        String resultKind,
        Object result, {
        String provider = 'library',
      }) {
        bridge.updates.add(<String, dynamic>{
          'type': 'catalog_updated',
          'query': <String, dynamic>{
            query: <String, dynamic>{'item_id': id, 'provider': provider},
          },
          'value': <String, dynamic>{resultKind: result},
        });
      }

      emit(
        'Artist',
        'artist-id',
        'Artist',
        _mediaJson('artist')..['name'] = 'Fresh artist',
      );
      emit(
        'Album',
        'album-id',
        'Album',
        _mediaJson('album')..['name'] = 'Fresh album',
      );
      emit(
        'Playlist',
        'playlist-id',
        'Playlist',
        _mediaJson('playlist')..['name'] = 'Fresh playlist',
      );
      expect(artist.artist?.name, 'Fresh artist');
      expect(album.album?.name, 'Fresh album');
      expect(playlist.playlist?.name, 'Fresh playlist');
      emit('PlaylistTracks', 'playlist-id', 'Tracks', <Object>[
        _mediaJson('track'),
      ]);
      expect(playlist.tracks.single.itemId, 'track-id');
      emit('PlaylistTracks', 'playlist-id', 'Tracks', <Object>[]);
      expect(playlist.tracks, isEmpty);
      emit(
        'Album',
        'album-id',
        'Album',
        _mediaJson('album')..['name'] = 'Wrong provider',
        provider: 'other',
      );
      expect(album.album?.name, 'Fresh album');
      await artist.setPageVisible(false);
      emit(
        'Artist',
        'artist-id',
        'Artist',
        _mediaJson('artist')..['name'] = 'Hidden update',
      );
      expect(artist.artist?.name, 'Fresh artist');
    },
  );

  test(
    'catalog update preserves request identity separately from canonical media',
    () {
      final update = CatalogUpdate.fromJson(<String, dynamic>{
        'query': <String, dynamic>{
          'Album': <String, dynamic>{
            'item_id': 'source-album',
            'provider': 'source',
          },
        },
        'value': <String, dynamic>{'Album': _mediaJson('album')},
      });
      expect(
        update.matches(CatalogQueryKind.album, 'source-album', 'source'),
        isTrue,
      );
      expect(update.item?.provider, 'library');
      expect(
        () => CatalogUpdate.fromJson(<String, dynamic>{
          'query': <String, dynamic>{},
          'value': <String, dynamic>{},
        }),
        throwsFormatException,
      );
    },
  );

  test(
    'recommendation snapshots preserve independent folder loading and cached playlist tracks',
    () {
      final snapshot = RecommendationSnapshot.fromJson({
        'is_loading': true,
        'has_load_error': true,
        'folders': [
          {
            'item_id': 'ready',
            'provider': 'lb',
            'items': [],
            'is_loading': false,
            'playlist_tracks': [_mediaJson('track')],
          },
          {
            'item_id': 'pending',
            'provider': 'lb',
            'items': [],
            'is_loading': true,
            'has_load_error': true,
          },
        ],
      });
      expect(snapshot.isLoading, isTrue);
      expect(snapshot.hasLoadError, isTrue);
      expect(snapshot.hasFolderLoadError('ready'), isFalse);
      expect(snapshot.hasFolderLoadError('pending'), isTrue);
      expect(snapshot.hasFolderLoadError('not-discovered'), isTrue);
      expect(snapshot.isFolderLoading('ready'), isFalse);
      expect(snapshot.isFolderLoading('pending'), isTrue);
      expect(snapshot.isFolderLoading('not-discovered'), isTrue);
      expect(snapshot.folders.first.playlistTracks.single.itemId, 'track-id');
      expect(const RecommendationSnapshot().isFolderLoading('absent'), isFalse);
    },
  );

  test(
    'queue playback provider prefers stream details over library mappings',
    () {
      final item = QueueItem.fromJson(<String, dynamic>{
        'queue_item_id': 'queued-track',
        'media_item': _mediaJson('track'),
        'streamdetails': <String, dynamic>{'provider': 'deezer--one'},
      });

      expect(item.playbackProvider, 'deezer--one');
      expect(item.mediaItem.provider, 'library');
    },
  );

  test(
    'queue playback provider resolves library mappings without a stream',
    () {
      for (final streamDetails in <Object?>[
        null,
        <String, dynamic>{'provider': '  '},
      ]) {
        final item = QueueItem.fromJson(<String, dynamic>{
          'queue_item_id': 'queued-track',
          'media_item': _mediaJson('track'),
          'streamdetails': streamDetails,
        });

        expect(item.playbackProvider, 'source--one');
      }
    },
  );

  test(
    'queue playback provider preserves direct providers and unmapped items',
    () {
      for (final provider in <String>['deezer--one', 'library', '']) {
        final item = QueueItem.fromJson(<String, dynamic>{
          'queue_item_id': 'queued-track',
          'media_item': <String, dynamic>{
            'media_type': 'track',
            'item_id': 'track-id',
            'provider': provider,
            'name': 'Track',
          },
        });

        expect(item.playbackProvider, provider);
      }
    },
  );

  test('queue artwork survives parsing without media artwork', () {
    MediaItemImage.configureProxyBaseUrl('https://music.example');
    final item = QueueItem.fromJson(<String, dynamic>{
      'queue_item_id': 'queued-track',
      'media_item': <String, dynamic>{
        'media_type': 'track',
        'item_id': 'track-id',
        'provider': 'library',
        'name': 'Queued track',
      },
      'image': <String, dynamic>{
        'type': 'thumb',
        'path': 'https://example.com/queue-cover.jpg',
        'proxy_id': 'queue-cover',
        'provider': 'test',
        'remotely_accessible': true,
      },
    });

    expect(item.mediaItem.imageUriForSize(80), isNull);
    expect(item.image, isNotNull);
    expect(item.imageUriForSize(80), item.image!.getProxyPath(size: 80));
    expect(item.imageUriForSize(512), item.image!.getProxyPath(size: 512));
  });

  test('queue artwork falls back to media artwork when absent or null', () {
    MediaItemImage.configureProxyBaseUrl('https://music.example');
    for (final hasImageField in <bool>[false, true]) {
      final json = <String, dynamic>{
        'queue_item_id': 'queued-track',
        'media_item': _mediaJson('track')
          ..['metadata'] = <String, dynamic>{
            'images': <Map<String, dynamic>>[
              <String, dynamic>{
                'type': 'thumb',
                'path': 'https://example.com/media-cover.jpg',
                'proxy_id': 'media-cover',
                'provider': 'test',
                'remotely_accessible': true,
              },
            ],
          },
        if (hasImageField) 'image': null,
      };
      final item = QueueItem.fromJson(json);

      expect(item.image, isNull);
      expect(item.imageUriForSize(80), isNotNull);
      expect(item.imageUriForSize(80), item.mediaItem.imageUriForSize(80));
    }
  });

  test(
    'playlist deltas retain unrelated data and notify only their own listeners',
    () async {
      final bridge = _PendingUserDataBridge();
      final userData = UserData(coreBridge: bridge);
      addTearDown(() async {
        userData.dispose();
        await bridge.updates.close();
      });
      final loading = userData.loadLibraryData();
      bridge.initial.complete(bridge.createSnapshot(isLoading: true));
      await loading;
      final favorites = userData.favoriteTracks;
      final playlists = userData.playlists;
      var favoriteChanges = 0;
      var playlistChanges = 0;
      var trackChanges = 0;
      userData.favoritesChanges.addListener(() => favoriteChanges++);
      userData.playlistsChanges.addListener(() => playlistChanges++);
      userData.playlistChanges('playlist-id').addListener(() => trackChanges++);
      var revision = 0;
      void publish({String name = 'track', bool isEmpty = false}) {
        bridge.updates.add({
          'type': 'user_data_changed',
          'revision': ++revision,
          'playlist': _mediaJson('playlist')
            ..['is_loading_tracks'] = false
            ..['items'] = [if (!isEmpty) _mediaJson('track')..['name'] = name],
        });
      }

      publish();
      await Future<void>.delayed(Duration.zero);
      final tracks = userData.tracksForPlaylist('playlist-id');
      expect(tracks, hasLength(1));
      expect(userData.isLoadingPlaylistTracks('playlist-id'), isFalse);
      publish();
      await Future<void>.delayed(Duration.zero);
      expect(userData.tracksForPlaylist('playlist-id'), same(tracks));
      expect(trackChanges, 1);
      publish(name: 'Changed');
      await Future<void>.delayed(Duration.zero);
      expect(userData.tracksForPlaylist('playlist-id').single.name, 'Changed');
      publish(isEmpty: true);
      await Future<void>.delayed(Duration.zero);
      expect(userData.tracksForPlaylist('playlist-id'), isEmpty);
      expect(trackChanges, 3);
      expect(favoriteChanges, 0);
      expect(playlistChanges, 0);
      expect(userData.favoriteTracks, same(favorites));
      expect(userData.playlists, same(playlists));
      expect(bridge.snapshots, isEmpty);
    },
  );

  test('playlist deltas retire older in-flight full snapshots', () async {
    final bridge = _PendingUserDataBridge();
    final userData = UserData(coreBridge: bridge);
    addTearDown(() async {
      userData.dispose();
      await bridge.updates.close();
    });
    final loading = userData.loadLibraryData();
    bridge.initial.complete(bridge.createSnapshot());
    await loading;
    final refresh = userData.refreshLibrarySnapshot();
    await Future<void>.delayed(Duration.zero);
    final updatedPlaylist = _mediaJson('playlist')
      ..['items'] = [_mediaJson('track')..['name'] = 'New'];
    bridge.updates.add({
      'type': 'user_data_changed',
      'playlist': updatedPlaylist,
      'revision': 1,
    });
    expect(userData.tracksForPlaylist('playlist-id').single.name, 'New');
    bridge.snapshots.first.complete(bridge.createSnapshot());
    await Future<void>.delayed(Duration.zero);
    expect(userData.tracksForPlaylist('playlist-id').single.name, 'New');
    expect(bridge.snapshots, hasLength(2));
    bridge.snapshots.last.complete(
      bridge.createSnapshot(revision: 1)..['playlists'] = [updatedPlaylist],
    );
    await refresh;
    expect(userData.tracksForPlaylist('playlist-id').single.name, 'New');
  });

  test(
    'targeted metadata changes notify playlists without changing favorites',
    () async {
      final bridge = _PendingUserDataBridge();
      final userData = UserData(coreBridge: bridge);
      addTearDown(() async {
        userData.dispose();
        await bridge.updates.close();
      });
      final loading = userData.loadLibraryData();
      bridge.initial.complete(bridge.createSnapshot());
      await loading;
      var playlistsChanged = 0;
      var favoritesChanged = 0;
      userData.playlistsChanges.addListener(() => playlistsChanged++);
      userData.favoritesChanges.addListener(() => favoritesChanged++);
      bridge.updates.add({
        'type': 'user_data_changed',
        'revision': 1,
        'playlist': _mediaJson('playlist')..['name'] = 'Renamed',
      });
      await Future<void>.delayed(Duration.zero);
      expect(userData.playlists.single.name, 'Renamed');
      expect(playlistsChanged, 1);
      expect(favoritesChanged, 0);
      expect(bridge.snapshots, isEmpty);
    },
  );

  test(
    'playlist revisions reject stale deltas without losing other playlists',
    () async {
      final bridge = _PendingUserDataBridge();
      final userData = UserData(coreBridge: bridge);
      addTearDown(() async {
        userData.dispose();
        await bridge.updates.close();
      });
      Map<String, dynamic> playlist(String itemId, String name) =>
          _mediaJson('playlist')
            ..['item_id'] = itemId
            ..['items'] = [_mediaJson('track')..['name'] = name];
      final loading = userData.loadLibraryData();
      bridge.initial.complete(
        bridge.createSnapshot(revision: 4)
          ..['playlists'] = [
            playlist('playlist-id', 'Original'),
            playlist('other', 'Original'),
          ],
      );
      await loading;
      void publish(String itemId, String name, int revision) =>
          bridge.updates.add({
            'type': 'user_data_changed',
            'revision': revision,
            'playlist': playlist(itemId, name),
          });
      publish('other', 'Other new', 6);
      publish('playlist-id', 'First new', 5);
      publish('other', 'Stale', 5);
      expect(userData.tracksForPlaylist('other').single.name, 'Other new');
      expect(
        userData.tracksForPlaylist('playlist-id').single.name,
        'First new',
      );
      final refresh = userData.refreshLibrarySnapshot();
      await Future<void>.delayed(Duration.zero);
      bridge.snapshots.single.complete(
        bridge.createSnapshot(revision: 8)
          ..['playlists'] = [
            playlist('playlist-id', 'Snapshot'),
            playlist('other', 'Snapshot'),
          ],
      );
      await refresh;
      publish('playlist-id', 'Delayed', 7);
      expect(userData.tracksForPlaylist('playlist-id').single.name, 'Snapshot');
      expect(bridge.snapshots, hasLength(1));
    },
  );

  test('area listeners observe loading starts and failures', () async {
    final bridge = _PendingUserDataBridge();
    final userData = UserData(coreBridge: bridge);
    addTearDown(() async {
      userData.dispose();
      await bridge.updates.close();
    });
    final favoriteLoading = <bool>[];
    final playlistLoading = <bool>[];
    userData.favoritesChanges.addListener(
      () => favoriteLoading.add(userData.isLoadingFavorites),
    );
    userData.playlistsChanges.addListener(
      () => playlistLoading.add(userData.isLoadingPlaylists),
    );
    final loading = userData.loadLibraryData();
    bridge.initial.completeError(StateError('load failed'));
    await expectLater(loading, throwsStateError);
    expect(favoriteLoading, [true, false]);
    expect(playlistLoading, [true, false]);
  });

  test('identical full snapshots preserve lists and do not notify', () async {
    final bridge = _PendingUserDataBridge();
    final userData = UserData(coreBridge: bridge);
    addTearDown(() async {
      userData.dispose();
      await bridge.updates.close();
    });
    final loading = userData.loadLibraryData();
    bridge.initial.complete(bridge.createSnapshot());
    await loading;
    final tracks = userData.favoriteTracks;
    final playlists = userData.playlists;
    var changes = 0;
    userData.addListener(() => changes++);
    final refresh = userData.refreshLibrarySnapshot();
    await Future<void>.delayed(Duration.zero);
    bridge.snapshots.single.complete(bridge.createSnapshot());
    await refresh;
    expect(changes, 0);
    expect(userData.favoriteTracks, same(tracks));
    expect(userData.playlists, same(playlists));
  });

  test(
    'user data rejects late startup snapshots and preserves newer loading state',
    () async {
      for (final shouldFail in <bool>[false, true]) {
        final bridge = _PendingUserDataBridge();
        final userData = UserData(coreBridge: bridge);
        final loading = userData.loadLibraryData();
        bridge.updates.add({'type': 'user_data_changed'});
        await Future<void>.delayed(Duration.zero);
        bridge.snapshots.single.complete(
          bridge.createSnapshot(isLoading: true),
        );
        await Future<void>.delayed(Duration.zero);
        final tracks = userData.favoriteTracks;
        final playlists = userData.playlists;
        expect(tracks, hasLength(1));
        expect(playlists, hasLength(1));
        if (shouldFail) {
          bridge.initial.completeError(StateError('initial read failed'));
          await expectLater(loading, throwsStateError);
        } else {
          bridge.initial.complete(bridge.createSnapshot(hasItems: false));
          await loading;
        }
        expect(identical(userData.favoriteTracks, tracks), isTrue);
        expect(identical(userData.playlists, playlists), isTrue);
        expect(userData.isLoadingFavorites, isTrue);
        expect(userData.isLoadingPlaylists, isTrue);
        expect(userData.isLoadingPlaylistTracks('playlist-id'), isTrue);
        userData.dispose();
        await bridge.updates.close();
      }
    },
  );

  test(
    'user data keeps native loading flags until a confirmed empty snapshot',
    () async {
      final bridge = _PendingUserDataBridge();
      final userData = UserData(coreBridge: bridge);
      addTearDown(() async {
        userData.dispose();
        await bridge.updates.close();
      });
      final loading = userData.loadLibraryData();
      bridge.initial.complete(bridge.createSnapshot(isLoading: true));
      await loading;
      expect(userData.isLoadingLibraryData, isFalse);
      expect(userData.isLoadingFavorites, isTrue);
      expect(userData.isLoadingPlaylists, isTrue);
      final track = userData.favoriteTracks.single;
      final refresh = userData.refreshLibrarySnapshot();
      await Future<void>.delayed(Duration.zero);
      expect(userData.favoriteTracks.single, same(track));
      bridge.snapshots.single.complete(bridge.createSnapshot(hasItems: false));
      await refresh;
      expect(userData.favoriteTracks, isEmpty);
      expect(userData.playlists, isEmpty);
      expect(userData.isFavorite(track), isFalse);
      expect(userData.isLoadingFavorites, isFalse);
      expect(userData.isLoadingPlaylists, isFalse);
      expect(userData.isLoadingPlaylistTracks('playlist-id'), isFalse);
    },
  );

  test(
    'user data drains newer snapshot requests after superseded results or errors',
    () async {
      for (final shouldFail in <bool>[false, true]) {
        final bridge = _PendingUserDataBridge();
        final userData = UserData(coreBridge: bridge);
        var notifications = 0;
        userData.addListener(() => notifications++);
        final refresh = userData.refreshLibrarySnapshot();
        final completion = expectLater(refresh, completes);
        await Future<void>.delayed(Duration.zero);
        expect(userData.refreshLibrarySnapshot(), same(refresh));
        if (shouldFail) {
          bridge.snapshots.first.completeError(StateError('superseded read'));
        } else {
          bridge.snapshots.first.complete(bridge.createSnapshot());
        }
        await Future<void>.delayed(Duration.zero);
        expect(bridge.snapshots, hasLength(2));
        expect(notifications, 0);
        bridge.snapshots.last.complete(bridge.createSnapshot(hasItems: false));
        await completion;
        expect(userData.favoriteTracks, isEmpty);
        expect(userData.playlists, isEmpty);
        expect(notifications, 1);
        final lateRefresh = userData.refreshLibrarySnapshot();
        await Future<void>.delayed(Duration.zero);
        userData.dispose();
        if (shouldFail) {
          bridge.snapshots.last.completeError(StateError('disposed read'));
        } else {
          bridge.snapshots.last.complete(bridge.createSnapshot());
        }
        await lateRefresh;
        expect(notifications, 1);
        expect(userData.favoriteTracks, isEmpty);
        await userData.refreshLibrarySnapshot();
        await userData.loadLibraryData();
        expect(bridge.snapshots, hasLength(3));
        expect(bridge.initialReads, 0);
        await bridge.updates.close();
      }
    },
  );

  test(
    'user data retains confirmed lists on a current read failure and can retry',
    () async {
      final bridge = _PendingUserDataBridge();
      final userData = UserData(coreBridge: bridge);
      addTearDown(() async {
        userData.dispose();
        await bridge.updates.close();
      });
      final loading = userData.loadLibraryData();
      bridge.initial.complete(bridge.createSnapshot());
      await loading;
      final tracks = userData.favoriteTracks;
      final playlists = userData.playlists;
      final failedRefresh = userData.refreshLibrarySnapshot();
      await Future<void>.delayed(Duration.zero);
      bridge.snapshots.single.completeError(StateError('current read failed'));
      await expectLater(failedRefresh, throwsStateError);
      expect(userData.favoriteTracks, same(tracks));
      expect(userData.playlists, same(playlists));
      final retry = userData.refreshLibrarySnapshot();
      await Future<void>.delayed(Duration.zero);
      bridge.snapshots.last.complete(bridge.createSnapshot(hasItems: false));
      await retry;
      expect(userData.favoriteTracks, isEmpty);
      expect(userData.playlists, isEmpty);
    },
  );

  test(
    'provider aliases without a URI use the confirmed library favorite',
    () async {
      final userData = UserData(coreBridge: _FavoriteSnapshotBridge());
      await userData.loadLibraryData();
      final providerItem = Track.fromJson(<String, dynamic>{
        'item_id': 'source-id',
        'provider': 'source--one',
        'name': 'Track',
      });
      expect(userData.isFavorite(providerItem), isTrue);
      userData.dispose();
    },
  );

  test('playlist ownership survives favorite projection', () {
    final userData = UserData(coreBridge: _UnusedCoreBridge());
    final playlist = Playlist.fromJson(
      _mediaJson('playlist')
        ..['access'] = <String, dynamic>{
          'owner': 'user-one',
          'sharing': 'private',
          'shared_users': <String>[],
          'collaborative': false,
        },
    );
    final projected = userData.withFavoriteStatus(playlist);
    expect(
      createMusicAssistantMediaItemPayload(projected)['access'],
      <String, dynamic>{
        'owner': 'user-one',
        'sharing': 'private',
        'shared_users': <String>[],
        'collaborative': false,
      },
    );
    userData.dispose();
  });

  test(
    'favorite indexes preserve identities, mappings and URI matches',
    () async {
      final bridge = _FavoriteSnapshotBridge();
      final userData = UserData(coreBridge: bridge);
      addTearDown(userData.dispose);
      await userData.loadLibraryData();
      for (final json in <Map<String, dynamic>>[
        {'item_id': 'track-id', 'provider': 'library', 'name': 'Direct'},
        {
          'item_id': 'source-id',
          'provider': 'source--one',
          'name': 'Provider alias',
        },
        {
          'item_id': 'other',
          'provider': 'another',
          'name': 'Shared mapping',
          'provider_mappings': [
            {
              'item_id': 'source-id',
              'provider_instance': 'source--one',
              'provider_domain': 'source',
            },
          ],
        },
        {
          'item_id': 'other',
          'provider': 'another',
          'name': 'Primary mapping',
          'provider_mappings': [
            {
              'item_id': 'track-id',
              'provider_instance': 'library',
              'provider_domain': 'library',
            },
          ],
        },
        {
          'item_id': 'other',
          'provider': 'another',
          'name': 'URI',
          'uri': 'library://track/track-id',
        },
      ]) {
        expect(
          userData.isFavorite(Track.fromJson(json)),
          isTrue,
          reason: json['name'] as String,
        );
      }
      final favorite = userData.favoriteTracks.single;
      expect(userData.withFavoriteStatus(favorite), same(favorite));
      expect(
        userData.isFavorite(
          Album.fromJson({
            'item_id': favorite.itemId,
            'provider': favorite.provider,
            'name': 'Other type',
          }),
        ),
        isFalse,
      );
      final unrelated = Track.fromJson({
        'item_id': 'other',
        'provider': 'unrelated',
        'name': 'Unrelated',
        'favorite': true,
      });
      expect(userData.isFavorite(unrelated), isFalse);
      expect(userData.withFavoriteStatus(unrelated).favorite, isFalse);
      bridge.hasFavorite = false;
      await userData.refreshLibrarySnapshot();
      expect(userData.isFavorite(favorite), isFalse);
      expect(
        userData.isFavorite(
          Track.fromJson({
            'item_id': 'source-id',
            'provider': 'source--one',
            'name': 'Alias',
          }),
        ),
        isFalse,
      );
    },
  );

  test('favorite projection preserves complete modeled media payloads', () {
    final userData = UserData(coreBridge: _UnusedCoreBridge());
    for (final mediaType in <String>['track', 'album', 'artist', 'playlist']) {
      final json = _mediaJson(mediaType)
        ..['uri'] = 'library://$mediaType/item'
        ..['duration'] = 233
        ..['disc_number'] = 2
        ..['track_number'] = 4
        ..['year'] = 2026
        ..['artist_type'] = 'artist'
        ..['is_dynamic'] = true;
      final original = mediaItemFromJson(json);
      final projected = userData.withFavoriteStatus(original);
      expect(projected, same(original));
      expect(
        createMusicAssistantMediaItemPayload(projected),
        createMusicAssistantMediaItemPayload(original),
      );
      if (projected is Playlist) {
        expect(
          createMusicAssistantMediaItemPayload(projected)['is_dynamic'],
          isTrue,
        );
      }
    }
    userData.dispose();
  });

  test('availability survives parsing and playback payload serialization', () {
    for (final mediaType in <String>[
      'track',
      'album',
      'artist',
      'playlist',
      'radio',
      'audiobook',
      'podcast',
      'folder',
    ]) {
      final json = _mediaJson(mediaType)..['available'] = false;
      final item = mediaItemFromJson(json);
      expect(item.isAvailable, isFalse);
      expect(item.canPlay, isFalse);
      expect(createMusicAssistantMediaItemPayload(item)['available'], isFalse);
    }
  });

  test(
    'unknown availability is optimistic but unavailable mappings are not',
    () {
      final json = _mediaJson('track')..['is_playable'] = true;
      expect(Track.fromJson(json).canPlay, isTrue);
      (json['provider_mappings'] as List).first['available'] = false;
      expect(Track.fromJson(json).canPlay, isFalse);
      json['provider_mappings'] = null;
      expect(Track.fromJson(json).canPlay, isTrue);
    },
  );

  tearDown(() => MediaItemImage.configureProxyBaseUrl(null));

  test('settings wire contains only online session preferences', () {
    expect(
      const SessionSettingsUpdate(
        isDarkTheme: true,
        logLevel: LogLevelPreference.info,
        shouldShowMultiDeviceButton: false,
      ).toJson(),
      {
        'theme': 'dark',
        'logLevel': 'info',
        'shouldShowMultiDeviceButton': false,
      },
    );
  });

  test('artwork requires an observed proxy ID or inline content', () {
    MediaItemImage.configureProxyBaseUrl('https://music.example/ma');
    for (final path in [
      'cover.jpg',
      '/local/cover.jpg',
      'https://cdn.example/cover.jpg',
    ]) {
      expect(
        MediaItemImage(
          type: 'thumb',
          path: path,
          provider: 'url',
          remotelyAccessible: true,
        ).getProxyPath(),
        isNull,
      );
    }
    const inline =
        'data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%2F%3E';
    expect(
      const MediaItemImage(
        type: 'thumb',
        path: inline,
        provider: 'builtin',
        remotelyAccessible: false,
      ).getProxyPath(),
      inline,
    );
    final proxy = const MediaItemImage(
      type: 'thumb',
      path: '/server/cover.jpg',
      provider: 'library',
      remotelyAccessible: false,
      proxyId: 'known-cover',
    ).getProxyPath(size: 160)!;
    expect(proxy, 'https://music.example/ma/imageproxy/known-cover?size=160');
    expect(MediaItemImage.isProxyUrl(proxy), isTrue);
    for (final invalid in [
      '',
      'data:image/png;base64,AA==',
      'https://cdn.example/imageproxy/known-cover',
      'https://music.example/ma/local/cover.jpg',
      'https://music.example/ma/imageproxy/',
      'https://music.example/ma/imageproxy/known-cover/other',
    ]) {
      expect(MediaItemImage.isProxyUrl(invalid), isFalse);
    }
  });

  test('decodes all concrete Music Assistant media variants', () {
    expect(mediaItemFromJson(_mediaJson('artist')), isA<Artist>());
    expect(mediaItemFromJson(_mediaJson('album')), isA<Album>());
    expect(mediaItemFromJson(_mediaJson('track')), isA<Track>());
    expect(mediaItemFromJson(_mediaJson('playlist')), isA<Playlist>());
    expect(mediaItemFromJson(_mediaJson('radio')), isA<Radio>());
    expect(mediaItemFromJson(_mediaJson('audiobook')), isA<Audiobook>());
    expect(mediaItemFromJson(_mediaJson('podcast')), isA<Podcast>());

    final episode = _mediaJson('podcast_episode')
      ..['podcast'] = _mediaJson('podcast');
    expect(mediaItemFromJson(episode), isA<PodcastEpisode>());

    for (final mediaType in <String>[
      'folder',
      'collection',
      'announcement',
      'flow_stream',
      'plugin_source',
      'audio_source',
      'sound_effect',
      'genre',
    ]) {
      expect(mediaItemFromJson(_mediaJson(mediaType)), isA<GenericMediaItem>());
    }

    expect(
      () => mediaItemFromJson(_mediaJson('future_media_type')),
      throwsFormatException,
    );
  });

  test('decodes compact nested media as complete typed models', () {
    final track = Track.fromJson(<String, dynamic>{
      ..._mediaJson('track'),
      'artists': <Map<String, dynamic>>[
        <String, dynamic>{
          'item_id': 'artist-id',
          'provider': 'library',
          'name': 'Artist',
          'media_type': 'artist',
          'available': true,
        },
      ],
      'album': <String, dynamic>{
        'item_id': 'album-id',
        'provider': 'library',
        'name': 'Album',
        'media_type': 'album',
        'image': <String, dynamic>{
          'type': 'thumb',
          'path': 'cover.jpg',
          'provider': 'library',
        },
      },
    });

    expect(track.version, 'deluxe');
    expect(track.externalIds, <List<String>>[
      <String>['isrc', 'US-TEST-01'],
    ]);
    expect(track.isPlayable, isFalse);
    expect(track.metadata?.descriptionLanguage, 'de');
    expect(track.metadata?.explicit, isTrue);
    expect(
      track.providerMappings.single.audioFormat?.contentType,
      ContentType.flac,
    );
    expect(track.providerMappings.single.inLibrary, isTrue);
    expect(track.artists.single, isA<Artist>());
    expect(track.artists.single.metadata, isNull);
    expect(track.album, isA<Album>());
    expect(track.album?.year, isNull);
    expect(track.album?.primaryImage?.path, 'cover.jpg');
  });

  test('compact playlist images survive recommendation decoding', () {
    MediaItemImage.configureProxyBaseUrl('http://music-assistant.local:8095');
    final Playlist playlist = Playlist.fromJson(<String, dynamic>{
      'item_id': 'artist-radio',
      'provider': 'listenbrainz_recommendations--test',
      'name': 'Radio: Artist',
      'media_type': 'playlist',
      'image': <String, dynamic>{
        'type': 'thumb',
        'path': 'https://example.test/artist.jpg',
        'provider': 'url',
        'remotely_accessible': true,
        'proxy_id': 'artist-cover',
      },
    });

    expect(playlist.primaryImage?.path, 'https://example.test/artist.jpg');
    expect(
      playlist.imageUriForSize(256),
      'http://music-assistant.local:8095/imageproxy/artist-cover?size=256',
    );
  });

  test('resolves sized proxy URLs for playlist and album artwork', () {
    MediaItemImage.configureProxyBaseUrl('http://music-assistant.local:8095/');
    final Map<String, dynamic> image = <String, dynamic>{
      'type': 'thumb',
      'path': 'relative-cover.jpg',
      'provider': 'library',
      'proxy_id': 'cover-proxy-id',
    };
    final playlist = Playlist.fromJson(<String, dynamic>{
      ..._mediaJson('playlist'),
      'metadata': <String, dynamic>{
        'images': <Map<String, dynamic>>[image],
        'cache_checksum': 'playlist checksum',
      },
    });
    final track = Track.fromJson(<String, dynamic>{
      ..._mediaJson('track'),
      'album': <String, dynamic>{
        'item_id': 'album-id',
        'provider': 'library',
        'name': 'Album',
        'media_type': 'album',
        'image': image,
      },
    });

    expect(
      playlist.imageUriForSize(175),
      'http://music-assistant.local:8095/imageproxy/cover-proxy-id'
      '?checksum=playlist+checksum&size=256',
    );
    expect(
      track.imageUriForSize(256),
      'http://music-assistant.local:8095/imageproxy/cover-proxy-id'
      '?size=256',
    );

    final recentTrack = Track.fromJson(<String, dynamic>{
      ..._mediaJson('track'),
      'image': <String, dynamic>{
        'type': 'thumb',
        'path': 'https://cdn.example.test/cover.jpg',
        'provider': 'deezer--instance',
        'remotely_accessible': true,
        'proxy_id': 'recent-track-proxy-id',
      },
    });

    expect(
      recentTrack.imageUriForSize(175),
      'http://music-assistant.local:8095/imageproxy/recent-track-proxy-id'
      '?size=256',
    );
  });

  test(
    'YouTube metadata covers survive favorite payload and snapshot parsing',
    () {
      MediaItemImage.configureProxyBaseUrl('http://music-assistant.local:8095');
      final image = <String, dynamic>{
        'type': 'thumb',
        'path': 'https://yt3.googleusercontent.com/cover',
        'provider': 'ytmusic--test',
        'remotely_accessible': true,
        'proxy_id': 'youtube-cover',
      };
      final track = Track.fromJson(<String, dynamic>{
        'media_type': 'track',
        'item_id': 'video',
        'provider': 'ytmusic--test',
        'name': 'Track',
        'uri': 'ytmusic--test://track/video',
        'image': null,
        'metadata': <String, dynamic>{
          'images': <Map<String, dynamic>>[image],
          'genres': null,
        },
      });
      final payload = createMusicAssistantMediaItemPayload(track);
      expect(payload['image'], isNull);
      expect(payload['metadata']['images'].single, image);
      final restored = Track.fromJson(<String, dynamic>{
        ...payload,
        'image': image,
        'favorite': true,
      });
      expect(restored.favorite, isTrue);
      expect(
        restored.imageUriForSize(256),
        'http://music-assistant.local:8095/imageproxy/youtube-cover?size=256',
      );
      expect(track.imageUriForSize(256), restored.imageUriForSize(256));
    },
  );

  test(
    'encodes direct track and nested model images for typed Rust commands',
    () {
      final Track track = Track.fromJson(<String, dynamic>{
        ..._mediaJson('track'),
        'image': <String, dynamic>{
          'type': 'thumb',
          'path': 'track.jpg',
          'provider': 'library',
          'proxy_id': 'track-image',
        },
        'artists': <Map<String, dynamic>>[
          <String, dynamic>{
            'media_type': 'artist',
            'item_id': 'artist-id',
            'provider': 'library',
            'name': 'Artist',
            'image': <String, dynamic>{
              'type': 'thumb',
              'path': 'artist.jpg',
              'provider': 'library',
              'proxy_id': 'artist-image',
            },
          },
        ],
        'album': <String, dynamic>{
          'media_type': 'album',
          'item_id': 'album-id',
          'provider': 'library',
          'name': 'Album',
          'image': <String, dynamic>{
            'type': 'thumb',
            'path': 'album.jpg',
            'provider': 'library',
            'proxy_id': 'album-image',
          },
        },
      });

      final Map<String, dynamic> payload = createMusicAssistantMediaItemPayload(
        track,
      );

      expect(payload['image'], <String, dynamic>{
        'type': 'thumb',
        'path': 'track.jpg',
        'provider': 'library',
        'remotely_accessible': false,
        'proxy_id': 'track-image',
      });
      expect(
        payload['artists'][0]['metadata']['images'][0]['proxy_id'],
        'artist-image',
      );
      expect(payload['artists'][0]['artist_type'], isNull);
      expect(
        payload['album']['metadata']['images'][0]['proxy_id'],
        'album-image',
      );
      expect(payload['album']['year'], isNull);
    },
  );

  test('retains complete album metadata in a queued track payload', () {
    final track = Track.fromJson(<String, dynamic>{
      ..._mediaJson('track'),
      'disc_number': 2,
      'track_number': 3,
      'artists': <Map<String, dynamic>>[
        <String, dynamic>{
          'item_id': 'artist-1',
          'provider': 'library',
          'name': 'Artist',
          'metadata': <String, dynamic>{'images': <Object>[]},
        },
      ],
      'album': <String, dynamic>{
        'item_id': 'album-1',
        'provider': 'library',
        'name': 'Complete album',
        'year': 2014,
        'album_type': 'album',
        'artists': <Map<String, dynamic>>[
          <String, dynamic>{
            'item_id': 'artist-1',
            'provider': 'library',
            'name': 'Artist',
            'metadata': <String, dynamic>{'images': <Object>[]},
          },
        ],
        'metadata': <String, dynamic>{'images': <Object>[]},
      },
    });

    final payload = createMusicAssistantMediaItemPayload(track);

    expect(payload['album']['year'], 2014);
    expect(payload['album']['album_type'], 'album');
    expect(payload['album']['artists'][0]['metadata'], isNotNull);
    expect(payload['artists'][0]['metadata'], isNotNull);
    expect(payload['disc_number'], 2);
    expect(payload['track_number'], 3);
  });

  test('encodes a compact podcast as a complete episode payload', () {
    final episode = PodcastEpisode.fromJson(<String, dynamic>{
      ..._mediaJson('podcast_episode'),
      'podcast': <String, dynamic>{
        'item_id': 'podcast-1',
        'provider': 'library',
        'name': 'Podcast',
        'image': <String, dynamic>{
          'type': 'thumb',
          'path': 'podcast.jpg',
          'provider': 'library',
          'proxy_id': 'podcast-proxy-id',
        },
      },
    });

    final payload = createMusicAssistantMediaItemPayload(episode);
    expect(episode.podcast, isA<Podcast>());
    expect(payload['podcast']['media_type'], 'podcast');
    expect(payload['podcast']['publisher'], isNull);
    expect(payload['podcast']['total_episodes'], isNull);
    expect(
      payload['podcast']['metadata']['images'][0]['proxy_id'],
      'podcast-proxy-id',
    );
  });

  test('decodes canonical Music Assistant player and session contracts', () {
    final player = MusicAssistantPlayer.fromJson(<String, dynamic>{
      'player_id': 'living-room',
      'provider': 'cast',
      'type': 'player',
      'name': 'Living Room',
      'available': true,
      'device_info': <String, dynamic>{
        'model': 'Speaker',
        'manufacturer': 'Example',
        'software_version': '1.2.3',
      },
      'playback_state': 'playing',
      'current_media': <String, dynamic>{
        'uri': 'library://track/1',
        'media_type': 'track',
        'queue_item_id': 'queue-1',
      },
      'output_protocols': <Map<String, dynamic>>[
        <String, dynamic>{
          'output_protocol_id': 'http',
          'name': 'HTTP',
          'protocol_domain': 'builtin',
        },
      ],
    });
    final user = MusicAssistantUser.fromJson(<String, dynamic>{
      'user_id': 'user-1',
      'username': 'Ada',
      'role': 'admin',
      'enabled': true,
      'provider_filter': <String>['library'],
      'player_filter': <String>['living-room'],
    });
    final session = MusicAssistantSession.fromJson(<String, dynamic>{
      'server_url': 'https://music-assistant.example',
      'access_token': 'token',
      'user_id': 'user-1',
      'username': 'Ada',
    });

    expect(player.deviceInfo.softwareVersion, '1.2.3');
    expect(player.playbackState, PlayState.playing);
    expect(player.currentMedia?.queueItemId, 'queue-1');
    expect(player.outputProtocols.single.protocolDomain, 'builtin');
    expect(user.providerFilter, <String>['library']);
    expect(session.accessToken, 'token');
  });
}
