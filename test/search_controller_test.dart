import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/search/search_controller.dart';

void main() {
  test('loads every search result type through the core bridge', () async {
    final coreBridge = _FakeSearchCoreBridge();
    final controller = SearchController(coreBridge: coreBridge);
    addTearDown(controller.dispose);

    await controller.runSearch('jazz');

    expect(coreBridge.query, 'jazz');
    expect(controller.tracks.single.name, 'Track');
    expect(controller.albums.single.name, 'Album');
    expect(controller.playlists.single.name, 'Playlist');
    expect(controller.artists.single.name, 'Artist');
    expect(controller.isLoadingTracks, isFalse);
    expect(controller.isLoadingAlbums, isFalse);
    expect(controller.isLoadingPlaylists, isFalse);
    expect(controller.isLoadingArtists, isFalse);
  });
}

class _FakeSearchCoreBridge extends Fake implements MashinCoreBridgeApi {
  String? query;

  @override
  Future<SearchResults> searchMusic(
    String searchQuery, {
    int limit = 50,
  }) async {
    query = searchQuery;
    return SearchResults(
      artists: <Artist>[
        Artist(itemId: 'artist-1', provider: 'library', name: 'Artist'),
      ],
      albums: <Album>[
        Album(
          itemId: 'album-1',
          provider: 'library',
          name: 'Album',
          artists: const <ArtistReference>[],
        ),
      ],
      tracks: <Track>[
        Track(
          itemId: 'track-1',
          provider: 'library',
          name: 'Track',
          duration: 120,
          artists: const <ArtistReference>[],
        ),
      ],
      playlists: <Playlist>[
        Playlist(
          itemId: 'playlist-1',
          provider: 'library',
          name: 'Playlist',
          isEditable: false,
        ),
      ],
      radio: const <MediaItem>[],
    );
  }
}
