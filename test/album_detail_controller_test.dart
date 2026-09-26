import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/features/album/album_detail_controller.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_models.dart';

void main() {
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
  final List<String> queries = <String>[];

  @override
  Future<Album> getAlbum(String itemId, String provider) async {
    queries.add('getAlbum:$itemId:$provider');
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
    return Artist(itemId: itemId, provider: provider, name: 'Similar Artist');
  }
}

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
