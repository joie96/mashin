import 'dart:async';

import 'package:flutter/material.dart' hide SearchController;
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_events.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/core/core_query_scope.dart';
import 'package:mashin/features/core/core_userdata.dart';
import 'package:mashin/features/search/search_controller.dart';
import 'package:mashin/features/search/search_screen_desktop.dart';
import 'package:mashin/features/search/search_screen_mobile.dart';
import 'package:mashin/features/shared/widgets/media_table_section_desktop.dart';
import 'package:mashin/features/shared/widgets/media_table_section_mobile.dart';

void main() {
  test(
    'search restarts pending reads on provider change without erasing enrichment',
    () async {
      final bridge = _ObservedSearchCoreBridge();
      final controller = SearchController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final searching = controller.runSearch('jazz');
      bridge.emitSearch('jazz', MediaType.track);
      bridge.emitAlbums('artist-0', year: 2020);
      final cached = controller.tracks;
      final oldRequests = bridge.requests.values.toList();
      bridge.updates.add({'type': 'provider_instances_changed'});
      expect(bridge.albumRequests, hasLength(2));
      expect(identical(controller.tracks, cached), isTrue);
      bridge.emitSearch('jazz', MediaType.track);
      expect(identical(controller.tracks, cached), isTrue);
      expect(bridge.albumRequests, hasLength(2));
      for (final request in oldRequests) {
        request.completeError(StateError('late search'));
      }
      bridge.albumRequests.first.completeError(StateError('late albums'));
      bridge.failAll('jazz');
      await searching;
      await Future<void>.delayed(Duration.zero);
      expect(identical(controller.tracks, cached), isTrue);
    },
  );

  test(
    'favorite reprojection remains authoritative during album updates',
    () async {
      final bridge = _ObservedSearchCoreBridge();
      final userData = _SearchUserData(coreBridge: bridge);
      final controller = SearchController(
        coreBridge: bridge,
        userDataController: userData,
      );
      addTearDown(() async {
        controller.dispose();
        userData.dispose();
        await bridge.updates.close();
      });
      final searching = controller.runSearch('jazz');
      bridge.emitSearch('jazz', MediaType.track);
      bridge.emitAlbums('artist-0', year: 2020);
      userData.isTrackFavorite = true;
      controller.refreshFavoriteStatuses();
      bridge.emitAlbums('artist-0', year: 2024);
      expect(controller.tracks.single.favorite, isTrue);
      expect(controller.tracks.single.album?.year, 2024);
      bridge.emitSearch('jazz', MediaType.track);
      expect(controller.tracks.single.favorite, isTrue);
      expect(controller.tracks.single.album?.year, 2024);
      expect(bridge.albumRequests, hasLength(1));
      bridge.failAll('jazz');
      await searching;
    },
  );

  test('explicit search track album years do not start enrichment', () async {
    final bridge = _ObservedSearchCoreBridge();
    final controller = SearchController(coreBridge: bridge);
    addTearDown(() async {
      controller.dispose();
      await bridge.updates.close();
    });
    final searching = controller.runSearch('jazz');
    bridge.emitSearch('jazz', MediaType.track, trackYear: 1999);
    bridge.emitAlbums('artist-0', year: 2024);
    expect(controller.tracks.single.album?.year, 1999);
    expect(bridge.albumRequests, isEmpty);
    bridge.failAll('jazz');
    await searching;
  });

  test(
    'identical search completions preserve enriched list and do not restart reads',
    () async {
      final bridge = _ObservedSearchCoreBridge();
      final controller = SearchController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final searching = controller.runSearch('jazz');
      bridge.emitSearch('jazz', MediaType.track);
      final source = controller.tracks;
      bridge.emitAlbums('artist-0', year: 2024);
      final cached = controller.tracks;
      bridge.requests[('jazz', MediaType.track)]!.complete(
        SearchResults(
          artists: const [],
          albums: const [],
          tracks: source,
          playlists: const [],
          radio: const [],
        ),
      );
      await Future<void>.delayed(Duration.zero);
      expect(identical(controller.tracks, cached), isTrue);
      expect(bridge.albumRequests, hasLength(1));
      bridge.failAll('jazz');
      await searching;
    },
  );

  for (final type in [
    MediaType.track,
    MediaType.album,
    MediaType.playlist,
    MediaType.artist,
  ]) {
    for (final isEmpty in [false, true]) {
      test(
        'early $type results end first loading and survive failure, empty=$isEmpty',
        () async {
          final bridge = _ObservedSearchCoreBridge();
          final controller = SearchController(coreBridge: bridge);
          addTearDown(() async {
            controller.dispose();
            await bridge.updates.close();
          });
          final searching = controller.runSearch('jazz');
          List<MediaItem> values() => switch (type) {
            MediaType.track => controller.tracks,
            MediaType.album => controller.albums,
            MediaType.playlist => controller.playlists,
            MediaType.artist => controller.artists,
            _ => throw ArgumentError.value(type),
          };
          bool isLoading() => switch (type) {
            MediaType.track => controller.isLoadingTracks,
            MediaType.album => controller.isLoadingAlbums,
            MediaType.playlist => controller.isLoadingPlaylists,
            MediaType.artist => controller.isLoadingArtists,
            _ => throw ArgumentError.value(type),
          };
          var changes = 0;
          controller.addListener(() => changes++);
          bridge.emitSearch('jazz', type, isEmpty: isEmpty, limit: 1);
          expect(changes, 0);
          expect(isLoading(), isTrue);
          bridge.emitSearch('jazz', type, isEmpty: isEmpty);
          expect(values(), hasLength(isEmpty ? 0 : 1));
          expect(isLoading(), isFalse);
          expect(changes, 1);
          final cached = values();
          bridge.emitSearch('jazz', type, isEmpty: isEmpty);
          expect(changes, 1);
          expect(identical(values(), cached), isTrue);
          bridge.failAll('jazz');
          await searching;
          expect(identical(values(), cached), isTrue);
          expect(isLoading(), isFalse);
        },
      );
    }
  }

  for (final retirement in [
    'new search',
    'reset',
    'dispose',
    'search invalidated',
    'albums invalidated',
  ]) {
    test(
      'search cancels owned reads and rejects late values after $retirement',
      () async {
        final bridge = _ObservedSearchCoreBridge();
        final controller = SearchController(coreBridge: bridge);
        addTearDown(() async {
          if (retirement != 'dispose') controller.dispose();
          await bridge.updates.close();
        });
        final searching = controller.runSearch('jazz');
        bridge.emitSearch('jazz', MediaType.track);
        bridge.emitAlbums('artist-0', year: 2020);
        expect(controller.tracks.single.album?.year, 2020);
        Future<void>? next;
        switch (retirement) {
          case 'new search':
            next = controller.runSearch('rock');
            bridge.emitSearch(
              'rock',
              MediaType.track,
              trackId: 'new',
              artistCount: 0,
            );
          case 'reset':
            controller.resetResults();
          case 'dispose':
            controller.dispose();
          case 'search invalidated':
            bridge.emitInvalidated('Search');
          case 'albums invalidated':
            bridge.emitInvalidated('ArtistAlbums');
        }
        final visible = controller.tracks;
        var changes = 0;
        if (retirement != 'dispose') controller.addListener(() => changes++);
        expect(
          bridge.canceled,
          containsAll([
            'jazz:track',
            'jazz:album',
            'jazz:playlist',
            'jazz:artist',
            'albums:artist-0',
          ]),
        );
        bridge.emitSearch('jazz', MediaType.track, trackId: 'late');
        bridge.emitAlbums('artist-0', year: 2040);
        bridge.albumRequests.first.complete([
          Album(
            itemId: 'Album-1',
            provider: 'library',
            name: 'Late',
            year: 2050,
            artists: const [],
          ),
        ]);
        bridge.failAll('jazz');
        await searching;
        await Future<void>.delayed(Duration.zero);
        expect(identical(controller.tracks, visible), isTrue);
        expect(changes, 0);
        if (next != null) {
          bridge.failAll('rock');
          await next;
        }
      },
    );
  }

  test(
    'search joins all cached artist albums by exact identity and replaces derived years',
    () async {
      final bridge = _ObservedSearchCoreBridge();
      final controller = SearchController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final searching = controller.runSearch('jazz');
      bridge.emitSearch('jazz', MediaType.track, artistCount: 5);
      expect(bridge.albumRequests, hasLength(5));
      bridge.emitAlbums('unrelated', year: 1900);
      bridge.emitAlbums('artist-4', year: 1901, albumId: 'album-1');
      expect(controller.tracks.single.album?.year, isNull);
      bridge.emitAlbums('artist-4', year: 2020);
      expect(controller.tracks.single.album?.year, 2020);
      bridge.emitAlbums('artist-4', year: 2024);
      expect(controller.tracks.single.album?.year, 2024);
      var changes = 0;
      controller.addListener(() => changes++);
      bridge.emitAlbums('artist-4', year: 2024);
      expect(changes, 0);
      bridge.emitAlbums('artist-4');
      expect(controller.tracks.single.album?.year, isNull);
      bridge.failAll('jazz');
      await searching;
    },
  );

  test(
    'changed search tracks replace enrichment without waiting for old albums',
    () async {
      final bridge = _ObservedSearchCoreBridge();
      final controller = SearchController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final searching = controller.runSearch('jazz');
      bridge.emitSearch('jazz', MediaType.track, artistCount: 2);
      expect(bridge.albumRequests, hasLength(2));
      bridge.emitSearch('jazz', MediaType.track, trackId: 'new');
      expect(bridge.albumRequests, hasLength(3));
      expect(
        bridge.canceled,
        containsAll(['albums:artist-0', 'albums:artist-1']),
      );
      bridge.emitAlbums('artist-1', year: 1900);
      expect(controller.tracks.single.album?.year, isNull);
      bridge.emitAlbums('artist-0', year: 2024);
      final cached = controller.tracks;
      bridge.emitSearch('jazz', MediaType.track, trackId: 'new');
      expect(identical(controller.tracks, cached), isTrue);
      expect(bridge.albumRequests, hasLength(3));
      bridge.albumRequests.first.complete([
        Album(
          itemId: 'Album-1',
          provider: 'library',
          name: 'Old',
          year: 1901,
          artists: const [],
        ),
      ]);
      await Future<void>.delayed(Duration.zero);
      expect(identical(controller.tracks, cached), isTrue);
      bridge.emitSearch('jazz', MediaType.track, isEmpty: true);
      bridge.emitAlbums('artist-0', year: 2030);
      expect(controller.tracks, isEmpty);
      bridge.failAll('jazz');
      await searching;
    },
  );

  test(
    'only matching search and active album invalidations retire results',
    () async {
      final bridge = _ObservedSearchCoreBridge();
      final controller = SearchController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final searching = controller.runSearch('jazz');
      bridge.emitSearch('jazz', MediaType.track);
      final cached = controller.tracks;
      bridge.emitInvalidated('Search', query: 'old');
      bridge.emitInvalidated('Search', limit: 1);
      bridge.emitInvalidated('Search', mediaType: MediaType.radio);
      bridge.emitInvalidated('ArtistAlbums', artistId: 'unrelated');
      expect(identical(controller.tracks, cached), isTrue);
      expect(bridge.canceled, isEmpty);
      bridge.failAll('jazz');
      await searching;
    },
  );

  test(
    'cached search tracks start album reads before either refresh',
    () async {
      final bridge = _ObservedSearchCoreBridge();
      final controller = SearchController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      final searching = controller.runSearch('jazz');
      bridge.emitSearch('jazz', MediaType.track);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.albumRequests, hasLength(1));
      expect(bridge.requests[('jazz', MediaType.track)]!.isCompleted, isFalse);
      bridge.emitAlbums('artist-0', year: 2020);
      expect(controller.tracks.single.album?.year, 2020);
      expect(controller.isLoadingTracks, isFalse);
      final cached = controller.tracks;
      bridge.failAll('jazz');
      bridge.albumRequests.single.completeError(StateError('offline albums'));
      await searching;
      await Future<void>.delayed(Duration.zero);
      expect(identical(controller.tracks, cached), isTrue);
    },
  );

  test(
    'database clearing reloads active search without clearing visible results',
    () async {
      final bridge = _EventSearchCoreBridge();
      final controller = SearchController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.updates.close();
      });
      await controller.runSearch('jazz');
      expect(bridge.requestedTypes, hasLength(4));
      bridge.updates.add({'type': 'cache_cleared', 'cache': 'images'});
      expect(bridge.requestedTypes, hasLength(4));
      bridge.updates.add({'type': 'cache_cleared', 'cache': 'database'});
      expect(bridge.requestedTypes, hasLength(8));
      expect(controller.tracks.single.name, 'Track');
      expect(controller.isLoadingTracks, isTrue);
      await Future<void>.delayed(Duration.zero);
      expect(controller.isLoadingTracks, isFalse);
      expect(controller.tracks.single.name, 'Track');
      bridge.updates.add({'type': 'provider_instances_changed'});
      expect(bridge.requestedTypes, hasLength(12));
      expect(controller.tracks.single.name, 'Track');
      await Future<void>.delayed(Duration.zero);
      expect(controller.isLoadingTracks, isFalse);
    },
  );

  test('catalog updates respect active search', () async {
    final bridge = _EventSearchCoreBridge();
    final controller = SearchController(coreBridge: bridge);
    addTearDown(() async {
      controller.dispose();
      await bridge.updates.close();
    });
    await controller.runSearch('jazz');
    void emit(String query, List<Map<String, dynamic>> tracks) {
      bridge.updates.add(<String, dynamic>{
        'type': 'catalog_updated',
        'query': <String, dynamic>{
          'Search': <String, dynamic>{
            'query': query,
            'media_type': 'track',
            'limit': 50,
          },
        },
        'value': <String, dynamic>{
          'Search': <String, dynamic>{'tracks': tracks},
        },
      });
    }

    emit('old', []);
    expect(controller.tracks.single.name, 'Track');
    emit('jazz', [
      {'item_id': 'fresh', 'provider': 'library', 'name': 'Fresh'},
    ]);
    expect(controller.tracks.single.name, 'Fresh');
    emit('jazz', []);
    expect(controller.tracks, isEmpty);
  });

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

class _EventSearchCoreBridge extends _FakeSearchCoreBridge {
  final updates = StreamController<Map<String, dynamic>>.broadcast(sync: true);
  @override
  Stream<Map<String, dynamic>> get events => updates.stream;
}

class _FakeSearchCoreBridge extends Fake implements MashinCoreBridgeApi {
  @override
  Stream<Map<String, dynamic>> get events => const Stream.empty();

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

class _SearchUserData extends UserData {
  _SearchUserData({required super.coreBridge});

  bool isTrackFavorite = false;

  @override
  bool isFavorite(MediaItem item) =>
      item is Track ? isTrackFavorite : item.favorite;
}

class _ObservedSearchCoreBridge extends _DeferredSearchCoreBridge {
  final updates = StreamController<Map<String, dynamic>>.broadcast(sync: true);
  final albumRequests = <Completer<List<Album>>>[];
  final albumArtists = <String>[];
  final canceled = <String>[];

  @override
  Stream<Map<String, dynamic>> get events => updates.stream;

  @override
  Future<SearchResults> searchMedia(
    String query,
    MediaType mediaType, {
    int limit = 50,
  }) {
    final result = super.searchMedia(query, mediaType, limit: limit);
    return CoreQueryScope.current?.execute(
          () => result,
          onCancel: () => canceled.add('$query:${mediaType.name}'),
        ) ??
        result;
  }

  @override
  Future<List<Album>> getArtistAlbums(String itemId, String provider) {
    final pending = Completer<List<Album>>();
    albumRequests.add(pending);
    albumArtists.add(itemId);
    return CoreQueryScope.current?.execute(
          () => pending.future,
          onCancel: () => canceled.add('albums:$itemId'),
        ) ??
        pending.future;
  }

  void emitSearch(
    String query,
    MediaType type, {
    bool isEmpty = false,
    int artistCount = 1,
    int limit = 50,
    String trackId = 'track',
    String albumId = 'Album-1',
    int? trackYear,
  }) {
    final key = switch (type) {
      MediaType.track => 'tracks',
      MediaType.album => 'albums',
      MediaType.playlist => 'playlists',
      MediaType.artist => 'artists',
      _ => throw ArgumentError.value(type),
    };
    updates.add({
      'type': 'catalog_updated',
      'query': {
        'Search': {'query': query, 'media_type': type.name, 'limit': limit},
      },
      'value': {
        'Search': {
          key: [
            if (!isEmpty)
              {
                'provider': 'library',
                'item_id': type == MediaType.track ? trackId : type.name,
                'name': 'Cached ${type.name}',
                if (type == MediaType.track)
                  'album': {
                    'provider': 'library',
                    'item_id': albumId,
                    'name': 'Album',
                    'year': ?trackYear,
                  },
                if (type == MediaType.track)
                  'artists': [
                    for (var index = 0; index < artistCount; index++)
                      {
                        'provider': 'library',
                        'item_id': 'artist-$index',
                        'name': 'Artist',
                      },
                  ],
              },
          ],
        },
      },
    });
  }

  void emitAlbums(String artistId, {int? year, String albumId = 'Album-1'}) {
    updates.add({
      'type': 'catalog_updated',
      'query': {
        'ArtistAlbums': {'provider': 'library', 'item_id': artistId},
      },
      'value': {
        'Albums': [
          if (year != null)
            {
              'provider': 'library',
              'item_id': albumId,
              'name': 'Full album',
              'year': year,
            },
        ],
      },
    });
  }

  void failAll(String query) {
    for (final entry in requests.entries) {
      if (entry.key.$1 == query && !entry.value.isCompleted) {
        entry.value.completeError(StateError('offline search'));
      }
    }
  }

  void emitInvalidated(
    String kind, {
    String query = 'jazz',
    int limit = 50,
    String artistId = 'artist-0',
    MediaType mediaType = MediaType.track,
  }) {
    updates.add({
      'type': 'catalog_read_changed',
      'query': {
        kind: kind == 'Search'
            ? {'query': query, 'media_type': mediaType.name, 'limit': limit}
            : {'provider': 'library', 'item_id': artistId},
      },
      'snapshot': {
        'phase': 'invalidated',
        'knowledge': 'unknown',
        'freshness': 'unknown',
      },
    });
  }
}

class _DeferredSearchCoreBridge extends Fake implements MashinCoreBridgeApi {
  @override
  Stream<Map<String, dynamic>> get events => const Stream.empty();

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
