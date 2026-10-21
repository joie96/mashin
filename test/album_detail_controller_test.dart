import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/album/album_detail_controller.dart';
import 'package:mashin_flutter/features/album/album_detail_page_desktop.dart';
import 'package:mashin_flutter/features/album/album_detail_page_mobile.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_events.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/app/session/preferred_provider_controller.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(
    () => PreferredProviderController.instance.initialize(
      preferredProvider: null,
      persist: (_) async {},
    ),
  );
  tearDown(PreferredProviderController.instance.reset);

  test('album play and shuffle send the loaded provider tracks', () async {
    final bridge = _FakeAlbumCoreBridge(hasMappings: true);
    final controller = AlbumDetailController(
      coreBridge: bridge,
      itemId: 'album-1',
      providerInstanceIdOrDomain: 'library',
    );
    addTearDown(controller.dispose);
    await controller.loadAlbum();
    await controller.selectProvider(controller.sectionProvider.mappings.last);

    await controller.playAlbum();
    expect(bridge.playedItems, controller.tracks);
    expect(bridge.playedItems.single.provider, 'ytmusic--one');
    expect(bridge.playedProvider, 'ytmusic--one');
    expect(bridge.playedShuffle, isFalse);

    await controller.shuffleAlbum();
    expect(bridge.playedItems, controller.tracks);
    expect(bridge.playedProvider, 'ytmusic--one');
    expect(bridge.playedShuffle, isTrue);
  });

  for (final isDesktop in <bool>[true, false]) {
    testWidgets(
      '${isDesktop ? 'desktop' : 'mobile'} album header switches only sections',
      (tester) async {
        tester.view.physicalSize = Size(isDesktop ? 1280 : 320, 900);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final bridge = _FakeAlbumCoreBridge(hasMappings: true);
        final events = MashinCoreBridgeEvents(coreBridge: bridge);
        addTearDown(events.dispose);
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark(),
            home: Scaffold(
              body: isDesktop
                  ? AlbumDetailPageDesktop(
                      coreBridge: bridge,
                      coreEvents: events,
                      itemId: 'album-1',
                      providerInstanceIdOrDomain: 'library',
                    )
                  : AlbumDetailPageMobile(
                      coreBridge: bridge,
                      coreEvents: events,
                      itemId: 'album-1',
                      providerInstanceIdOrDomain: 'library',
                    ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await tester.tap(
          find.byKey(
            ValueKey<String>(
              isDesktop ? 'ytmusic--one' : 'detail-provider-cycle',
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(
          bridge.queries,
          contains('getAlbumTracks:yt-album:ytmusic--one'),
        );
        expect(
          bridge.queries.where((query) => query == 'getAlbum:album-1:library'),
          hasLength(1),
        );
        expect(tester.takeException(), isNull);
      },
    );
  }

  test(
    'album provider switch preserves header and maps section artist',
    () async {
      final coreBridge = _FakeAlbumCoreBridge(hasMappings: true);
      final controller = AlbumDetailController(
        coreBridge: coreBridge,
        itemId: 'album-1',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(controller.dispose);
      await controller.loadAlbum();
      final Album? original = controller.album;
      expect(
        coreBridge.queries,
        contains('getAlbumTracks:dz-album:deezer--one'),
      );
      coreBridge.queries.clear();
      await controller.selectProvider(controller.sectionProvider.mappings.last);
      expect(identical(controller.album, original), isTrue);
      expect(controller.album?.provider, 'library');
      expect(
        coreBridge.queries,
        contains('getAlbumTracks:yt-album:ytmusic--one'),
      );
      expect(
        coreBridge.queries,
        contains('getArtistAlbums:yt-artist:ytmusic--one'),
      );
      expect(
        coreBridge.queries.where((query) => query.startsWith('getAlbum:')),
        isEmpty,
      );
    },
  );

  test(
    'loads album details and similar-artist fallback through core queries',
    () async {
      final coreBridge = _FakeAlbumCoreBridge();
      final controller = AlbumDetailController(
        coreBridge: coreBridge,
        itemId: 'album-1',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(controller.dispose);

      await controller.loadAlbum();

      expect(controller.album?.name, 'Album');
      expect(controller.tracks.single.version, 'Deluxe');
      expect(controller.tracks.single.externalIds, <List<String>>[
        <String>['isrc', 'track-isrc'],
      ]);
      expect(controller.tracks.single.isPlayable, isFalse);
      expect(controller.tracks.single.position, 7);
      expect(controller.tracks.single.dateAdded, '2026-09-14T12:00:00Z');
      expect(controller.tracks.single.discNumber, 2);
      expect(controller.tracks.single.trackNumber, 3);
      expect(controller.tracks.single.metadata?.description, 'Track metadata');
      expect(controller.tracks.single.album?.version, 'Remastered');
      expect(controller.otherAlbums.single.name, 'Other Album');
      expect(controller.similarArtists.single.name, 'Similar Artist');
      expect(coreBridge.queries, <String>[
        'getAlbum:album-1:library',
        'getAlbumTracks:album-1:library',
        'getArtistAlbums:artist-1:library',
        'getSimilarTracks:track-1:library:50:true',
        'getTrackVersions:track-1:library',
        'getSimilarTracks:track-1-alt:alt:50:true',
        'getArtist:artist-2:library',
      ]);
    },
  );
}

class _FakeAlbumCoreBridge extends Fake implements MashinCoreBridgeApi {
  _FakeAlbumCoreBridge({this.hasMappings = false});

  final bool hasMappings;
  final List<String> queries = <String>[];
  List<MediaItem> playedItems = <MediaItem>[];
  bool playedShuffle = false;
  String? playedProvider;

  @override
  Future<void> queuePlayMedia(
    List<MediaItem> mediaItems, {
    QueueOption option = QueueOption.replace,
    int? startItem,
    bool shuffle = false,
    String? providerInstance,
  }) async {
    playedItems = mediaItems;
    playedShuffle = shuffle;
    playedProvider = providerInstance;
  }

  @override
  Future<Album> getAlbum(String itemId, String provider) async {
    queries.add('getAlbum:$itemId:$provider');
    if (hasMappings) {
      return Album(
        itemId: 'album-1',
        provider: 'library',
        name: 'Album',
        artists: _album('album-1', 'Album').artists,
        providerMappings: _providerMappings('album'),
      );
    }
    return _album('album-1', 'Album', version: 'Remastered');
  }

  @override
  Future<List<Track>> getAlbumTracks(String itemId, String provider) async {
    queries.add('getAlbumTracks:$itemId:$provider');
    return <Track>[_track('track-1', provider)];
  }

  @override
  Future<List<Album>> getArtistAlbums(String itemId, String provider) async {
    queries.add('getArtistAlbums:$itemId:$provider');
    return <Album>[_album('album-2', 'Other Album')];
  }

  @override
  Future<List<Track>> getSimilarTracks(
    String itemId,
    String provider, {
    int limit = 50,
    bool allowLookup = true,
  }) async {
    queries.add('getSimilarTracks:$itemId:$provider:$limit:$allowLookup');
    if (itemId == 'track-1') {
      return const <Track>[];
    }
    return <Track>[_track('track-2', 'library', artistId: 'artist-2')];
  }

  @override
  Future<List<Track>> getTrackVersions(String itemId, String provider) async {
    queries.add('getTrackVersions:$itemId:$provider');
    return <Track>[_track('track-1-alt', 'alt')];
  }

  @override
  Future<Artist> getArtist(String itemId, String provider) async {
    queries.add('getArtist:$itemId:$provider');
    return Artist(
      itemId: itemId,
      provider: provider,
      name: 'Similar Artist',
      providerMappings: hasMappings
          ? _providerMappings('artist')
          : const <ProviderMapping>[],
    );
  }
}

List<ProviderMapping> _providerMappings(String kind) => <ProviderMapping>[
  ProviderMapping(
    itemId: 'dz-$kind',
    providerDomain: 'deezer',
    providerInstance: 'deezer--one',
    available: true,
  ),
  ProviderMapping(
    itemId: 'yt-$kind',
    providerDomain: 'ytmusic',
    providerInstance: 'ytmusic--one',
    available: true,
  ),
];

Album _album(String itemId, String name, {String version = ''}) => Album(
  itemId: itemId,
  provider: 'library',
  name: name,
  version: version,
  artists: <Artist>[
    Artist.fromJson(<String, dynamic>{
      'item_id': 'artist-1',
      'provider': 'library',
      'name': 'Album Artist',
    }),
  ],
);

Track _track(String itemId, String provider, {String artistId = 'artist-1'}) =>
    Track(
      itemId: itemId,
      provider: provider,
      name: 'Track',
      duration: 120,
      artists: <Artist>[
        Artist.fromJson(<String, dynamic>{
          'item_id': artistId,
          'provider': 'library',
          'name': artistId == 'artist-1' ? 'Album Artist' : 'Similar Artist',
        }),
      ],
      version: 'Deluxe',
      externalIds: <List<String>>[
        <String>['isrc', 'track-isrc'],
      ],
      isPlayable: false,
      metadata: const MediaItemMetadata(
        description: 'Track metadata',
        images: <MediaItemImage>[],
        genres: <String>[],
      ),
      position: 7,
      dateAdded: '2026-09-14T12:00:00Z',
      discNumber: 2,
      trackNumber: 3,
    );
