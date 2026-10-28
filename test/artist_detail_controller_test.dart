import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/features/artist/artist_detail_controller.dart';
import 'package:mashin/features/artist/artist_detail_page_desktop.dart';
import 'package:mashin/features/artist/artist_detail_page_mobile.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_events.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/app/session/preferred_provider_controller.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(
    () => PreferredProviderController.instance.initialize(
      preferredProvider: null,
      persist: (_) async {},
    ),
  );
  tearDown(PreferredProviderController.instance.reset);

  test(
    'artist play and shuffle retain the selected provider for top tracks',
    () async {
      final bridge = _FakeArtistCoreBridge(hasMappings: true);
      final controller = ArtistDetailController(
        coreBridge: bridge,
        itemId: 'artist-1',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(controller.dispose);
      await controller.loadArtist();
      await controller.selectProvider(controller.sectionProvider.mappings.last);

      await controller.playArtist();
      expect(bridge.playedItems, controller.topTracks);
      expect(bridge.playedProvider, 'ytmusic--one');
      expect(bridge.playedShuffle, isFalse);

      await controller.shuffleArtist();
      expect(bridge.playedItems, controller.topTracks);
      expect(bridge.playedProvider, 'ytmusic--one');
      expect(bridge.playedShuffle, isTrue);
    },
  );

  for (final isDesktop in <bool>[true, false]) {
    testWidgets(
      '${isDesktop ? 'desktop' : 'mobile'} header switches sections in both directions',
      (tester) async {
        tester.view.physicalSize = Size(isDesktop ? 1280 : 320, 900);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final bridge = _FakeArtistCoreBridge(hasMappings: true);
        final events = MashinCoreBridgeEvents(coreBridge: bridge);
        addTearDown(events.dispose);
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark().copyWith(
              visualDensity: isDesktop
                  ? VisualDensity.compact
                  : VisualDensity.standard,
            ),
            home: Scaffold(
              body: isDesktop
                  ? ArtistDetailPageDesktop(
                      coreBridge: bridge,
                      coreEvents: events,
                      itemId: 'artist-1',
                      providerInstanceIdOrDomain: 'library',
                    )
                  : ArtistDetailPageMobile(
                      coreBridge: bridge,
                      coreEvents: events,
                      itemId: 'artist-1',
                      providerInstanceIdOrDomain: 'library',
                    ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        final Finder cycleButton = find.byKey(
          ValueKey<String>(
            isDesktop ? 'ytmusic--one' : 'detail-provider-cycle',
          ),
        );
        final Finder initialButton = isDesktop
            ? find.byKey(const ValueKey<String>('deezer--one'))
            : cycleButton;
        expect(
          tester
              .getCenter(
                find.byKey(
                  ValueKey<String>(
                    '${isDesktop ? 'desktop' : 'mobile'}-provider-deezer--one',
                  ),
                ),
              )
              .dy,
          tester.getCenter(initialButton).dy,
        );
        if (isDesktop) {
          final Finder titleRow = find
              .ancestor(of: cycleButton, matching: find.byType(Row))
              .first;
          final Finder title = find
              .descendant(of: titleRow, matching: find.byType(Text))
              .first;
          expect(tester.getCenter(cycleButton).dy, tester.getCenter(title).dy);
        }
        await tester.tap(cycleButton);
        await tester.pumpAndSettle();
        expect(
          tester
              .getCenter(
                find.byKey(
                  ValueKey<String>(
                    '${isDesktop ? 'desktop' : 'mobile'}-provider-ytmusic--one',
                  ),
                ),
              )
              .dy,
          tester.getCenter(cycleButton).dy,
        );
        expect(
          bridge.queries,
          contains('getArtistAlbums:channel:ytmusic--one'),
        );
        await tester.tap(isDesktop ? initialButton : cycleButton);
        await tester.pumpAndSettle();
        expect(
          bridge.queries.where(
            (query) => query == 'getArtistAlbums:406:deezer--one',
          ),
          hasLength(2),
        );
        expect(
          bridge.queries.where(
            (query) => query == 'getArtist:artist-1:library',
          ),
          hasLength(1),
        );
        expect(tester.takeException(), isNull);
      },
    );
  }

  test(
    'provider switch reloads sections but retains library metadata',
    () async {
      final coreBridge = _FakeArtistCoreBridge(hasMappings: true);
      final controller = ArtistDetailController(
        coreBridge: coreBridge,
        itemId: 'artist-1',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(controller.dispose);
      await controller.loadArtist();
      final Artist? original = controller.artist;
      expect(controller.sectionProvider.selectedProvider, 'deezer--one');
      expect(coreBridge.queries, contains('getArtistAlbums:406:deezer--one'));
      coreBridge.queries.clear();

      await controller.selectProvider(controller.sectionProvider.mappings.last);

      expect(identical(controller.artist, original), isTrue);
      expect(controller.artist?.provider, 'library');
      expect(
        coreBridge.queries,
        contains('getArtistAlbums:channel:ytmusic--one'),
      );
      expect(
        coreBridge.queries,
        contains('getArtistTopTracks:channel:ytmusic--one'),
      );
      expect(
        coreBridge.queries,
        contains('getSimilarArtists:channel:ytmusic--one:25'),
      );
      expect(
        coreBridge.queries.where(
          (query) => query.startsWith('getArtist:artist-1'),
        ),
        isEmpty,
      );
      expect(
        controller.sectionProvider.mappings.first.providerDomain,
        'ytmusic',
      );
    },
  );

  test(
    'loads artist details and fallback artists through core queries',
    () async {
      final coreBridge = _FakeArtistCoreBridge();
      final controller = ArtistDetailController(
        coreBridge: coreBridge,
        itemId: 'artist-1',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(controller.dispose);

      await controller.loadArtist();

      expect(controller.artist?.name, 'Primary Artist');
      expect(controller.albums.single.name, 'Album');
      expect(controller.topTracks.single.name, 'Top Track');
      expect(controller.similarArtists.single.name, 'Similar Artist');
      expect(coreBridge.queries, <String>[
        'getArtist:artist-1:library',
        'getArtistAlbums:artist-1:library',
        'getArtistTopTracks:artist-1:library',
        'getSimilarArtists:artist-1:library:25',
        'getSimilarTracks:track-1:library:50:true',
        'getTrackVersions:track-1:library',
        'getSimilarTracks:track-1-alt:alt:50:true',
        'getArtist:artist-2:library',
      ]);
    },
  );
}

class _FakeArtistCoreBridge extends Fake implements MashinCoreBridgeApi {
  _FakeArtistCoreBridge({this.hasMappings = false});

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
  Future<Artist> getArtist(String itemId, String provider) async {
    queries.add('getArtist:$itemId:$provider');
    return switch (itemId) {
      'artist-1' => Artist(
        itemId: 'artist-1',
        provider: 'library',
        name: 'Primary Artist',
        providerMappings: hasMappings
            ? const <ProviderMapping>[
                ProviderMapping(
                  itemId: '406',
                  providerDomain: 'deezer',
                  providerInstance: 'deezer--one',
                  available: true,
                ),
                ProviderMapping(
                  itemId: 'channel',
                  providerDomain: 'ytmusic',
                  providerInstance: 'ytmusic--one',
                  available: true,
                ),
              ]
            : const <ProviderMapping>[],
      ),
      'artist-2' => _artist('artist-2', 'Similar Artist'),
      _ => throw StateError('unexpected artist $itemId'),
    };
  }

  @override
  Future<List<Album>> getArtistAlbums(String itemId, String provider) async {
    queries.add('getArtistAlbums:$itemId:$provider');
    return <Album>[
      Album(
        itemId: 'album-1',
        provider: provider,
        name: 'Album',
        artists: <Artist>[Artist.fromJson(_artistJson())],
        year: 2025,
      ),
    ];
  }

  @override
  Future<List<Track>> getArtistTopTracks(String itemId, String provider) async {
    queries.add('getArtistTopTracks:$itemId:$provider');
    return <Track>[_track('track-1', provider)];
  }

  @override
  Future<List<Artist>> getSimilarArtists(
    String itemId,
    String provider, {
    int limit = 25,
  }) async {
    queries.add('getSimilarArtists:$itemId:$provider:$limit');
    return const <Artist>[];
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
}

Artist _artist(String itemId, String name) =>
    Artist(itemId: itemId, provider: 'library', name: name);

Track _track(String itemId, String provider, {String artistId = 'artist-1'}) =>
    Track(
      itemId: itemId,
      provider: provider,
      name: itemId == 'track-1' ? 'Top Track' : 'Fallback Track',
      duration: 120,
      artists: <Artist>[Artist.fromJson(_artistJson(artistId))],
    );

Map<String, dynamic> _artistJson([String itemId = 'artist-1']) =>
    <String, dynamic>{
      'item_id': itemId,
      'provider': 'library',
      'name': itemId == 'artist-1' ? 'Primary Artist' : 'Similar Artist',
    };
