import 'dart:async';

import 'package:flutter/material.dart' hide SearchController;
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_events.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/search/search_controller.dart';
import 'package:mashin_flutter/features/search/search_screen_desktop.dart';
import 'package:mashin_flutter/features/search/search_screen_mobile.dart';
import 'package:mashin_flutter/features/shared/widgets/media_table_section_desktop.dart';
import 'package:mashin_flutter/features/shared/widgets/media_table_section_mobile.dart';

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
            artists: const <Artist>[],
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

  test('shows tracks before deduplicated artist albums arrive', () async {
    final coreBridge = _AlbumSearchCoreBridge();
    final controller = SearchController(coreBridge: coreBridge);
    addTearDown(controller.dispose);

    await controller.runSearch('jazz');
    expect(controller.isLoadingTracks, isFalse);
    expect(controller.tracks.first.album?.year, isNull);
    expect(coreBridge.requestedArtists, <String>['artist-1:library']);

    coreBridge.pendingAlbums.single.complete(<Album>[
      Album(
        itemId: 'album-1',
        provider: 'library',
        name: 'Complete album',
        artists: const <Artist>[],
        year: 2014,
      ),
    ]);
    await Future<void>.delayed(Duration.zero);
    expect(controller.tracks.first.album?.year, 2014);
    expect(controller.tracks.first.album, isA<Album>());
    expect(controller.tracks.last.album?.year, 2014);
    expect(controller.tracks.last.discNumber, 2);
  });

  test('ignores album responses from a previous search', () async {
    final coreBridge = _AlbumSearchCoreBridge();
    final controller = SearchController(coreBridge: coreBridge);
    addTearDown(controller.dispose);

    await controller.runSearch('old');
    await controller.runSearch('new');
    coreBridge.pendingAlbums.last.complete(<Album>[
      Album(
        itemId: 'album-1',
        provider: 'library',
        name: 'New album',
        artists: const <Artist>[],
        year: 2020,
      ),
    ]);
    await Future<void>.delayed(Duration.zero);
    expect(controller.tracks.first.album?.year, 2020);

    coreBridge.pendingAlbums.first.complete(<Album>[
      Album(
        itemId: 'album-1',
        provider: 'library',
        name: 'Old album',
        artists: const <Artist>[],
        year: 2014,
      ),
    ]);
    await Future<void>.delayed(Duration.zero);
    expect(controller.tracks.first.album?.year, 2020);

    controller.resetResults();
    expect(controller.tracks, isEmpty);
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
    expect(
      tester
          .widget<MediaTableSectionMobile<Track>>(
            find.byType(MediaTableSectionMobile<Track>),
          )
          .startsTrackRadio,
      isTrue,
    );

    await tester.enterText(find.byType(TextField), 'rock');
    await tester.pump(const Duration(milliseconds: 400));
    expect(coreBridge.query, 'jazz');

    await tester.tap(find.byTooltip('Suchen'));
    await tester.pump();
    expect(coreBridge.query, 'rock');
  });

  testWidgets('mobile search does not refocus after scrolling', (
    WidgetTester tester,
  ) async {
    tester.view.physicalSize = const Size(360, 400);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final coreBridge = _FakeSearchCoreBridge();
    final coreEvents = MashinCoreBridgeEvents(coreBridge: coreBridge);
    addTearDown(coreEvents.dispose);

    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Builder(
          builder: (BuildContext context) => Scaffold(
            body: TextButton(
              onPressed: () => Navigator.of(context).push(
                MaterialPageRoute<void>(
                  builder: (BuildContext context) => Scaffold(
                    body: SearchScreenMobile(
                      coreBridge: coreBridge,
                      coreEvents: coreEvents,
                    ),
                  ),
                ),
              ),
              child: const Text('Suche öffnen'),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Suche öffnen'));
    await tester.pumpAndSettle();

    final input = find.byType(EditableText);
    expect(tester.widget<EditableText>(input).focusNode.hasFocus, isTrue);

    await tester.enterText(find.byType(TextField), 'jazz');
    await tester.testTextInput.receiveAction(TextInputAction.search);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Suche'));
    await tester.pumpAndSettle();
    expect(tester.widget<EditableText>(input).focusNode.hasFocus, isFalse);

    final scrollView = find.byType(ListView).first;
    await tester.drag(scrollView, const Offset(0, -1000));
    await tester.pumpAndSettle();
    await tester.drag(scrollView, const Offset(0, 1000));
    await tester.pumpAndSettle();

    expect(tester.widget<EditableText>(input).focusNode.hasFocus, isFalse);
    expect(tester.testTextInput.isVisible, isFalse);
  });

  testWidgets('desktop search keeps scroll extent stable with results', (
    WidgetTester tester,
  ) async {
    tester.view.physicalSize = const Size(1100, 320);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final _FakeSearchCoreBridge coreBridge = _FakeSearchCoreBridge();
    final MashinCoreBridgeEvents coreEvents = MashinCoreBridgeEvents(
      coreBridge: coreBridge,
    );
    addTearDown(coreEvents.dispose);

    Widget searchPage(String query) => MaterialApp(
      theme: AppTheme.dark(),
      home: Scaffold(
        body: SearchScreenDesktop(
          coreBridge: coreBridge,
          coreEvents: coreEvents,
          desktopSearchDriven: true,
          desktopSearchQuery: query,
        ),
      ),
    );

    await tester.pumpWidget(searchPage(''));
    expect(
      find.text('Nutze die Suchleiste oben, um Ergebnisse anzuzeigen.'),
      findsOneWidget,
    );
    expect(tester.takeException(), isNull);

    await tester.pumpWidget(searchPage('jazz'));
    await tester.pumpAndSettle();
    expect(coreBridge.query, 'jazz');
    expect(
      tester
          .widget<MediaTableSectionDesktop<Track>>(
            find.byType(MediaTableSectionDesktop<Track>),
          )
          .startsTrackRadio,
      isTrue,
    );
    expect(find.byType(CustomScrollView), findsOneWidget);

    final ScrollPosition position = tester
        .state<ScrollableState>(find.byType(Scrollable).first)
        .position;
    final double loadedExtent = position.maxScrollExtent;
    expect(loadedExtent, greaterThan(0));
    for (final double target in <double>[loadedExtent, 0, loadedExtent]) {
      position.jumpTo(target);
      await tester.pump();
      expect(position.maxScrollExtent, closeTo(loadedExtent, 0.1));
    }
    expect(tester.takeException(), isNull);
  });
}

class _AlbumSearchCoreBridge extends _FakeSearchCoreBridge {
  final List<Completer<List<Album>>> pendingAlbums = <Completer<List<Album>>>[];
  final List<String> requestedArtists = <String>[];

  @override
  Future<SearchResults> searchMedia(
    String searchQuery,
    MediaType mediaType, {
    int limit = 50,
  }) async {
    final SearchResults results = await super.searchMedia(
      searchQuery,
      mediaType,
      limit: limit,
    );
    if (mediaType != MediaType.track) {
      return results;
    }
    final artist = Artist.fromJson(<String, dynamic>{
      'item_id': 'artist-1',
      'provider': 'library',
      'name': 'Artist',
      'image': null,
    });
    final album = Album.fromJson(<String, dynamic>{
      'item_id': 'album-1',
      'provider': 'library',
      'name': 'Incomplete album',
      'image': null,
    });
    return SearchResults(
      artists: results.artists,
      albums: results.albums,
      playlists: results.playlists,
      radio: results.radio,
      tracks: <Track>[
        for (final String itemId in <String>['first', 'second'])
          Track(
            itemId: itemId,
            provider: 'library',
            name: itemId,
            duration: 120,
            artists: <Artist>[artist],
            album: album,
            discNumber: 2,
          ),
      ],
    );
  }

  @override
  Future<List<Album>> getArtistAlbums(String itemId, String provider) {
    requestedArtists.add('$itemId:$provider');
    final completer = Completer<List<Album>>();
    pendingAlbums.add(completer);
    return completer.future;
  }
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
          artists: const <Artist>[],
        ),
      ],
      tracks: <Track>[
        Track(
          itemId: 'track-1',
          provider: 'library',
          name: 'Track',
          duration: 120,
          artists: const <Artist>[],
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
