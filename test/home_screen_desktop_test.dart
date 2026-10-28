import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_events.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/core/core_userdata.dart';
import 'package:mashin/features/home/home_screen_desktop.dart';
import 'package:mashin/features/home/home_controller.dart';
import 'package:mashin/features/home/home_screen_mobile.dart';
import 'package:mashin/features/shared/widgets/media_row_section_desktop.dart';
import 'package:mashin/features/shared/widgets/media_row_section_mobile.dart';
import 'package:mashin/features/shared/widgets/media_slide_section_desktop.dart';
import 'package:mashin/features/shared/widgets/media_slide_section_mobile.dart';

class _HomeCoreBridge implements MashinCoreBridgeApi {
  List<RecommendationFolder> artistFolders = <RecommendationFolder>[];
  RecommendationSnapshot? snapshot;
  int recommendationCalls = 0;
  final eventController = StreamController<Map<String, dynamic>>.broadcast(
    sync: true,
  );

  @override
  Stream<Map<String, dynamic>> get events => eventController.stream;

  @override
  Future<RecommendationSnapshot> getRecommendations({
    required RecommendationView view,
  }) async {
    if (view == RecommendationView.explore) {
      return const RecommendationSnapshot();
    }
    expect(view, RecommendationView.home);
    recommendationCalls += 1;
    if (snapshot != null) return snapshot!;
    final List<Track> tracks = List<Track>.generate(
      12,
      (int index) => Track(
        itemId: 'track-$index',
        provider: 'test',
        name: 'Track $index',
        duration: 120,
        artists: const <Artist>[],
      ),
    );
    final List<Playlist> genres = List<Playlist>.generate(
      9,
      (int index) => Playlist(
        itemId: 'genre-$index',
        provider: 'test',
        name: 'Genre $index',
        isEditable: false,
      ),
    );

    return RecommendationSnapshot(
      folders: <RecommendationFolder>[
        ...artistFolders,
        RecommendationFolder(
          itemId: 'user_recommendations',
          provider: 'listenbrainz_recommendations--test',
          title: 'Empfehlungen',
          items: <MediaItem>[tracks.first],
        ),
        RecommendationFolder(
          itemId: 'user_genre_playlists_monthly',
          provider: 'listenbrainz_recommendations--test',
          title: 'Genres',
          items: genres,
        ),
        RecommendationFolder(
          itemId: 'user_top_tracks_monthly',
          provider: 'listenbrainz_recommendations--test',
          title: 'Tracks',
          items: tracks,
        ),
        RecommendationFolder(
          itemId: 'user_artist_playlists_monthly',
          provider: 'listenbrainz_recommendations--test',
          title: 'Artist Playlists',
          items: <MediaItem>[genres.first],
        ),
        for (final String id in <String>[
          'user_top_artists_monthly',
          if (artistFolders.isEmpty) 'user_similar_artists_seed_monthly',
        ])
          RecommendationFolder(
            itemId: id,
            provider: 'listenbrainz_recommendations--test',
            title: 'Artists',
            items: const <MediaItem>[
              Artist(itemId: 'artist', provider: 'test', name: 'Artist'),
            ],
          ),
      ],
    );
  }

  @override
  Future<List<MediaItem>> getRecentlyPlayedTracks({int limit = 40}) async =>
      <MediaItem>[
        const Track(
          itemId: 'recent-track',
          provider: 'test',
          name: 'Kürzlich gehört',
          duration: 120,
          artists: <Artist>[],
        ),
      ];

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  test(
    'home starts with skeletons and reads Rust after page recreation',
    () async {
      final bridge = _HomeCoreBridge();
      final userData = UserData(coreBridge: bridge);
      addTearDown(userData.dispose);
      addTearDown(bridge.eventController.close);
      final first = HomeController(
        coreBridge: bridge,
        userDataController: userData,
      );
      expect(first.recommendationItems, isEmpty);
      expect(first.isLoadingRecommendations, isTrue);
      expect(bridge.recommendationCalls, 0);
      await first.load();
      await Future<void>.delayed(Duration.zero);
      expect(first.recommendationItems, isNotEmpty);
      first.dispose();

      final reopened = HomeController(
        coreBridge: bridge,
        userDataController: userData,
      );
      addTearDown(reopened.dispose);
      expect(reopened.isLoadingRecommendations, isTrue);
      expect(reopened.isLoadingGenrePlaylists, isTrue);
      expect(reopened.recommendationItems, isEmpty);
      expect(reopened.recentListens, isEmpty);
      expect(reopened.isLoadingRecentListens, isTrue);
      await reopened.load();
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recommendationCalls, 2);
      expect(reopened.recommendationItems, isNotEmpty);
      expect(reopened.recentListens.single.itemId, 'recent-track');
    },
  );

  test('home retains per-folder loading from the Rust snapshot', () async {
    final bridge = _HomeCoreBridge()
      ..snapshot = const RecommendationSnapshot(isLoading: true);
    final userData = UserData(coreBridge: bridge);
    addTearDown(userData.dispose);
    addTearDown(bridge.eventController.close);
    final controller = HomeController(
      coreBridge: bridge,
      userDataController: userData,
    );
    addTearDown(controller.dispose);
    expect(controller.isLoadingRecommendations, isTrue);
    expect(controller.recommendationItems, isEmpty);
    await controller.load();
    await Future<void>.delayed(Duration.zero);

    bridge.snapshot = const RecommendationSnapshot();
    bridge.eventController.add({'type': 'recommendations_changed'});
    await Future<void>.delayed(Duration.zero);
    expect(controller.isLoadingRecommendations, isFalse);
    expect(controller.recommendationItems, isEmpty);
    expect(bridge.recommendationCalls, 2);
  });

  test(
    'home reads a recommendation snapshot only once per initial load',
    () async {
      final bridge = _HomeCoreBridge();
      final userData = UserData(coreBridge: bridge);
      addTearDown(userData.dispose);
      addTearDown(bridge.eventController.close);

      final controller = HomeController(
        coreBridge: bridge,
        userDataController: userData,
      );
      addTearDown(controller.dispose);
      await controller.load();
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recommendationCalls, 1);
      expect(controller.recommendationItems, isNotEmpty);
    },
  );

  test('home unlocks ready folders while later folders are loading', () async {
    final bridge = _HomeCoreBridge()
      ..snapshot = const RecommendationSnapshot(isLoading: true);
    final controller = HomeController(coreBridge: bridge);
    addTearDown(controller.dispose);
    addTearDown(bridge.eventController.close);
    await controller.load();
    await Future<void>.delayed(Duration.zero);
    expect(controller.isLoadingRecommendations, isTrue);
    bridge.snapshot = const RecommendationSnapshot(
      isLoading: true,
      folders: [
        RecommendationFolder(
          itemId: 'user_recommendations',
          provider: 'lb',
          title: 'Ready',
          items: [
            Track(
              itemId: 'first',
              provider: 'lb',
              name: 'First',
              duration: 120,
              artists: [],
            ),
          ],
        ),
        RecommendationFolder(
          itemId: 'user_genre_playlists_monthly',
          provider: 'lb',
          title: 'Pending',
          items: [],
          isLoading: true,
        ),
      ],
    );
    bridge.eventController.add({'type': 'recommendations_changed'});
    await Future<void>.delayed(Duration.zero);
    expect(controller.recommendationItems.single.itemId, 'first');
    expect(controller.isLoadingRecommendations, isFalse);
    expect(controller.isLoadingGenrePlaylists, isTrue);
    bridge.snapshot = const RecommendationSnapshot(
      folders: [
        RecommendationFolder(
          itemId: 'user_recommendations',
          provider: 'lb',
          title: 'Ready',
          items: [
            Track(
              itemId: 'first',
              provider: 'lb',
              name: 'First',
              duration: 120,
              artists: [],
            ),
          ],
        ),
      ],
    );
    bridge.eventController.add({'type': 'recommendations_changed'});
    await Future<void>.delayed(Duration.zero);
    expect(controller.recommendationItems.single.itemId, 'first');
    expect(controller.isLoadingGenrePlaylists, isFalse);
  });

  test('home maps renamed ListenBrainz folders', () async {
    final HomeController controller = HomeController(
      coreBridge: _HomeCoreBridge(),
    );
    addTearDown(controller.dispose);
    await controller.load();
    await Future<void>.delayed(Duration.zero);
    expect(controller.recommendationItems, hasLength(1));
    expect(controller.genrePlaylists, hasLength(9));
    expect(controller.topTracks, hasLength(12));
    expect(controller.artistPlaylists, hasLength(1));
    expect(controller.topArtists, hasLength(1));
    expect(controller.artistSections.single.artists, hasLength(1));
  });

  List<RecommendationFolder> artistFolders() => <RecommendationFolder>[
    for (final String category in <String>['genre', 'similar'])
      for (int index = 0; index < 5; index++)
        RecommendationFolder(
          itemId:
              'user_${category}_artists_${category == 'genre' ? index % 4 : index}_monthly',
          provider: 'lb',
          title: '$category ${category == 'genre' ? index % 4 : index}',
          items: List<Artist>.generate(
            8,
            (int artistIndex) => Artist(
              itemId: '$category-$index-$artistIndex',
              provider: 'lb',
              name: 'Artist $artistIndex',
            ),
          ),
        ),
  ];

  test(
    'home samples distinct artist sections and shuffles their contents',
    () async {
      final List<RecommendationFolder> folders = artistFolders();
      final Set<String> selections = <String>{};
      for (int seed = 0; seed < 8; seed++) {
        final HomeController controller = HomeController(
          coreBridge: _HomeCoreBridge()..artistFolders = folders,
          random: math.Random(seed),
        );
        addTearDown(controller.dispose);
        await controller.load();
        await Future<void>.delayed(Duration.zero);
        final List<HomeArtistSection> sections = controller.artistSections;
        expect(sections, hasLength(4));
        expect(
          sections.map((section) => section.title.split(' ').first),
          <String>['genre', 'similar', 'genre', 'similar'],
        );
        expect(sections.map((section) => section.title).toSet(), hasLength(4));
        selections.add(sections.map((section) => section.title).join(','));
        for (final HomeArtistSection section in sections) {
          final List<Artist> original = folders
              .firstWhere((folder) => folder.title == section.title)
              .items
              .whereType<Artist>()
              .toList();
          expect(section.artists, unorderedEquals(original));
          expect(section.artists, isNot(orderedEquals(original)));
        }
        final List<HomeArtistSection> before = controller.artistSections;
        controller.refreshFavoriteStatuses();
        expect(
          controller.artistSections.map((section) => section.title),
          before.map((section) => section.title),
        );
        for (int index = 0; index < before.length; index++) {
          expect(
            controller.artistSections[index].artists,
            before[index].artists,
          );
        }
      }
      expect(selections.length, greaterThan(1));
    },
  );

  for (final bool isCompact in <bool>[true, false]) {
    testWidgets(
      'home interleaves artist sections before recent tracks on ${isCompact ? 'mobile' : 'desktop'}',
      (WidgetTester tester) async {
        tester.view.physicalSize = Size(isCompact ? 412 : 1100, 850);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final _HomeCoreBridge bridge = _HomeCoreBridge()
          ..artistFolders = artistFolders();
        final MashinCoreBridgeEvents events = MashinCoreBridgeEvents(
          coreBridge: bridge,
        );
        addTearDown(events.dispose);
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark(),
            home: Scaffold(
              body: isCompact
                  ? HomeScreenMobile(coreBridge: bridge, coreEvents: events)
                  : HomeScreenDesktop(coreBridge: bridge, coreEvents: events),
            ),
          ),
        );
        await tester.pumpAndSettle();
        tester.view.physicalSize = Size(isCompact ? 412 : 1100, 6000);
        await tester.pumpAndSettle();
        if (isCompact) {
          expect(
            tester
                .widget<MediaSlideSectionMobile<Track>>(
                  find.byType(MediaSlideSectionMobile<Track>),
                )
                .startsTrackRadio,
            isTrue,
          );
          final List<MediaRowSectionMobile<Track>> trackSections = tester
              .widgetList<MediaRowSectionMobile<Track>>(
                find.byType(MediaRowSectionMobile<Track>),
              )
              .toList();
          expect(trackSections, hasLength(2));
          expect(
            trackSections.every((section) => section.startsTrackRadio),
            isTrue,
          );
        } else {
          expect(
            tester
                .widget<MediaSlideSectionDesktop<Track>>(
                  find.byType(MediaSlideSectionDesktop<Track>),
                )
                .startsTrackRadio,
            isTrue,
          );
          final List<MediaRowSectionDesktop<Track>> trackSections = tester
              .widgetList<MediaRowSectionDesktop<Track>>(
                find.byType(MediaRowSectionDesktop<Track>),
              )
              .toList();
          expect(trackSections, hasLength(2));
          expect(
            trackSections.every((section) => section.startsTrackRadio),
            isTrue,
          );
        }
        final List<String> titles = isCompact
            ? tester
                  .widgetList<MediaRowSectionMobile<Artist>>(
                    find.byType(MediaRowSectionMobile<Artist>),
                  )
                  .map((section) => section.title)
                  .toList()
            : tester
                  .widgetList<MediaRowSectionDesktop<Artist>>(
                    find.byType(MediaRowSectionDesktop<Artist>),
                  )
                  .map((section) => section.title)
                  .toList();
        expect(titles, hasLength(5));
        expect(titles.first, 'Deine Lieblingskünstler:innen');
        expect(titles.skip(1).map((title) => title.split(' ').first), <String>[
          'genre',
          'similar',
          'genre',
          'similar',
        ]);
        expect(
          tester.getTopLeft(find.text('Kürzlich gespielt')).dy,
          greaterThan(tester.getTopLeft(find.text(titles.last)).dy),
        );
        expect(tester.takeException(), isNull);
      },
    );
  }

  testWidgets('desktop home keeps scroll extent stable after loading', (
    WidgetTester tester,
  ) async {
    tester.view.physicalSize = const Size(1100, 320);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final MashinCoreBridgeApi coreBridge = _HomeCoreBridge();
    final MashinCoreBridgeEvents coreEvents = MashinCoreBridgeEvents(
      coreBridge: coreBridge,
    );
    addTearDown(coreEvents.dispose);

    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: HomeScreenDesktop(
            coreBridge: coreBridge,
            coreEvents: coreEvents,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    final Finder scrollView = find.byType(CustomScrollView);
    final ScrollPosition position = tester
        .state<ScrollableState>(
          find
              .descendant(of: scrollView, matching: find.byType(Scrollable))
              .first,
        )
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
