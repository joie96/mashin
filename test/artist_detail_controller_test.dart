import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/features/artist/artist_detail_controller.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_models.dart';

void main() {
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
  final List<String> queries = <String>[];

  @override
  Future<Artist> getArtist(String itemId, String provider) async {
    queries.add('getArtist:$itemId:$provider');
    return switch (itemId) {
      'artist-1' => _artist('artist-1', 'Primary Artist'),
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
      artists: <Artist>[
        Artist.fromJson(_artistJson(artistId)),
      ],
    );

Map<String, dynamic> _artistJson([String itemId = 'artist-1']) =>
    <String, dynamic>{
      'item_id': itemId,
      'provider': 'library',
      'name': itemId == 'artist-1' ? 'Primary Artist' : 'Similar Artist',
    };
