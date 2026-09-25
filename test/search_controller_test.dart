import 'dart:async';

import 'package:flutter/material.dart' hide SearchController;
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_events.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/search/search_controller.dart';
import 'package:mashin_flutter/features/search/search_screen_mobile.dart';

void main() {
  test('loads every search result type through the core bridge', () async {
    final coreBridge = _FakeSearchCoreBridge();
    final controller = SearchController(coreBridge: coreBridge);
    addTearDown(controller.dispose);

    await controller.runSearch('jazz');

    expect(coreBridge.query, 'jazz');
    expect(coreBridge.requestedTypes, <MediaType>[
      MediaType.track,
      MediaType.album,
      MediaType.playlist,
      MediaType.artist,
    ]);
    expect(controller.tracks.single.name, 'Track');
    expect(controller.albums.single.name, 'Album');
    expect(controller.playlists.single.name, 'Playlist');
    expect(controller.artists.single.name, 'Artist');
    expect(controller.isLoadingTracks, isFalse);
    expect(controller.isLoadingAlbums, isFalse);
    expect(controller.isLoadingPlaylists, isFalse);
    expect(controller.isLoadingArtists, isFalse);
  });

  test('publishes each media type as soon as its request completes', () async {
    final _DeferredSearchCoreBridge coreBridge = _DeferredSearchCoreBridge();
    final SearchController controller = SearchController(
      coreBridge: coreBridge,
    );
    addTearDown(controller.dispose);

    final Future<void> search = controller.runSearch('jazz');
    expect(coreBridge.requests.keys.map((request) => request.$2).toSet(), {
      MediaType.track,
      MediaType.album,
      MediaType.playlist,
      MediaType.artist,
    });
    expect(controller.isLoadingTracks, isTrue);
    expect(controller.isLoadingAlbums, isTrue);

    coreBridge.requests[('jazz', MediaType.track)]!.complete(
      SearchResults(
        artists: const <Artist>[],
        albums: const <Album>[],
        tracks: <Track>[
          Track(
            itemId: 'track-1',
            provider: 'library',
            name: 'Track',
            duration: 120,
            artists: const <ArtistReference>[],
          ),
        ],
        playlists: const <Playlist>[],
        radio: const <MediaItem>[],
      ),
    );
    await Future<void>.delayed(Duration.zero);
    expect(controller.tracks.single.name, 'Track');
    expect(controller.isLoadingTracks, isFalse);
    expect(controller.isLoadingAlbums, isTrue);

    coreBridge.requests[('jazz', MediaType.album)]!.completeError(
      StateError('Album search failed'),
    );
    coreBridge.requests[('jazz', MediaType.playlist)]!.complete(_emptyResults);
    coreBridge.requests[('jazz', MediaType.artist)]!.complete(_emptyResults);
    await search;
    expect(controller.isLoadingAlbums, isFalse);
    expect(controller.tracks.single.name, 'Track');
  });

  test('ignores outdated results after a new search or reset', () async {
    final _DeferredSearchCoreBridge coreBridge = _DeferredSearchCoreBridge();
    final SearchController controller = SearchController(
      coreBridge: coreBridge,
    );
    addTearDown(controller.dispose);

    final Future<void> oldSearch = controller.runSearch('old');
    final Future<void> newSearch = controller.runSearch('new');
    coreBridge.completeAll('old');
    await oldSearch;
    expect(controller.isLoadingTracks, isTrue);

    controller.resetResults();
    coreBridge.completeAll('new');
    await newSearch;
    expect(controller.hasSearchRequest, isFalse);
    expect(controller.tracks, isEmpty);
    expect(controller.isLoadingTracks, isFalse);
  });

  testWidgets('mobile search runs only on Enter or search button', (
    WidgetTester tester,
  ) async {
    final _FakeSearchCoreBridge coreBridge = _FakeSearchCoreBridge();
    final MashinCoreBridgeEvents coreEvents = MashinCoreBridgeEvents(
      coreBridge: coreBridge,
    );
    addTearDown(coreEvents.dispose);

    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: SearchScreenMobile(
            coreBridge: coreBridge,
            coreEvents: coreEvents,
          ),
        ),
      ),
    );

    await tester.enterText(find.byType(TextField), 'jazz');
    await tester.pump(const Duration(milliseconds: 400));
    expect(coreBridge.query, isNull);

    await tester.testTextInput.receiveAction(TextInputAction.search);
    await tester.pump();
    expect(coreBridge.query, 'jazz');

    await tester.enterText(find.byType(TextField), 'rock');
    await tester.pump(const Duration(milliseconds: 400));
    expect(coreBridge.query, 'jazz');

    await tester.tap(find.byTooltip('Suchen'));
    await tester.pump();
    expect(coreBridge.query, 'rock');
  });
}

class _FakeSearchCoreBridge extends Fake implements MashinCoreBridgeApi {
  String? query;
  final List<MediaType> requestedTypes = <MediaType>[];

  @override
  Future<SearchResults> searchMedia(
    String searchQuery,
    MediaType mediaType, {
    int limit = 50,
  }) async {
    query = searchQuery;
    requestedTypes.add(mediaType);
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

const SearchResults _emptyResults = SearchResults(
  artists: <Artist>[],
  albums: <Album>[],
  tracks: <Track>[],
  playlists: <Playlist>[],
  radio: <MediaItem>[],
);

class _DeferredSearchCoreBridge extends Fake implements MashinCoreBridgeApi {
  final Map<(String, MediaType), Completer<SearchResults>> requests =
      <(String, MediaType), Completer<SearchResults>>{};

  @override
  Future<SearchResults> searchMedia(
    String query,
    MediaType mediaType, {
    int limit = 50,
  }) {
    final Completer<SearchResults> request = Completer<SearchResults>();
    requests[(query, mediaType)] = request;
    return request.future;
  }

  void completeAll(String query) {
    for (final MapEntry<(String, MediaType), Completer<SearchResults>> entry
        in requests.entries) {
      if (entry.key.$1 == query) {
        entry.value.complete(_emptyResults);
      }
    }
  }
}
