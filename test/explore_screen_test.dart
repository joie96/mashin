import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_events.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/core/core_userdata.dart';
import 'package:mashin/features/explore/explore_controller.dart';
import 'package:mashin/features/explore/explore_screen.dart';
import 'package:mashin/features/shared/widgets/media_row_section_desktop.dart';
import 'package:mashin/features/shared/widgets/media_row_section_mobile.dart';
import 'package:mashin/features/shared/widgets/media_slide_section_desktop.dart';
import 'package:mashin/features/shared/widgets/media_slide_section_mobile.dart';
import 'package:mashin/features/shared/widgets/media_table_section_desktop.dart';
import 'package:mashin/features/shared/widgets/media_table_section_mobile.dart';

class _ExploreBridge implements MashinCoreBridgeApi {
  bool shouldFail = false;
  bool shouldReturnEmpty = false;
  int recommendationCalls = 0;
  final List<(String, String)> playlistCalls = <(String, String)>[];
  Completer<RecommendationSnapshot>? pendingRecommendations;
  final eventController = StreamController<Map<String, dynamic>>.broadcast(
    sync: true,
  );

  @override
  Stream<Map<String, dynamic>> get events => eventController.stream;

  @override
  Future<RecommendationSnapshot> getRecommendations({
    required RecommendationView view,
  }) async {
    if (view == RecommendationView.home) {
      return const RecommendationSnapshot();
    }
    expect(view, RecommendationView.explore);
    recommendationCalls += 1;
    if (shouldFail) throw StateError('Unavailable');
    final Completer<RecommendationSnapshot>? pending = pendingRecommendations;
    if (pending != null) return pending.future;
    if (shouldReturnEmpty) return const RecommendationSnapshot();
    return RecommendationSnapshot(
      folders: <RecommendationFolder>[
        RecommendationFolder(
          itemId: 'user_exploration_weekly',
          provider: 'lb',
          title: 'Weekly',
          playlistTracks: _tracks('exploration'),
          items: <MediaItem>[
            const Playlist(
              itemId: 'exploration',
              provider: 'lb',
              name: 'Weekly Mix',
              isEditable: false,
            ),
          ],
        ),
        RecommendationFolder(
          itemId: 'similarusers_jam_weekly',
          provider: 'lb',
          title: 'Similar Users Jam',
          playlistTracks: _tracks('similarusers_jam_latest'),
          items: <MediaItem>[
            const Playlist(
              itemId: 'similarusers_jam_latest',
              provider: 'lb',
              name: 'Similar Listener Weekly Jams',
              isEditable: false,
            ),
          ],
        ),
        for (final String id in <String>[
          'sitewide_top_albums_weekly',
          'sitewide_releases_weekly',
        ])
          RecommendationFolder(
            itemId: id,
            provider: 'lb',
            title: id,
            items: <MediaItem>[
              Album(
                itemId: id,
                provider: 'library',
                name: id,
                artists: const <Artist>[],
              ),
            ],
          ),
        RecommendationFolder(
          itemId: 'sitewide_top_tracks_weekly',
          provider: 'lb',
          title: 'Charts',
          items: <MediaItem>[
            const Track(
              itemId: 'chart',
              provider: 'library',
              name: 'Chart Track',
              duration: 180,
              artists: <Artist>[],
            ),
          ],
        ),
        RecommendationFolder(
          itemId: 'sitewide_top_artists_weekly',
          provider: 'lb',
          title: 'Artists',
          items: <MediaItem>[
            const Artist(
              itemId: 'artist',
              provider: 'library',
              name: 'Chart Artist',
            ),
          ],
        ),
      ],
    );
  }

  @override
  Future<List<Track>> getPlaylistTracks(
    String itemId,
    String provider, {
    bool forceRefresh = false,
    bool allowDynamicTracks = false,
  }) async {
    playlistCalls.add((itemId, provider));
    throw StateError('Recommendation pages must not load playlist tracks');
  }

  List<Track> _tracks(String itemId) {
    return List<Track>.generate(
      8,
      (int index) => Track(
        itemId: '$itemId-track-$index',
        provider: 'lb',
        name: '$itemId Track $index',
        duration: 180,
        artists: const <Artist>[],
      ),
    );
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  test(
    'explore starts with skeletons and reads Rust after page recreation',
    () async {
      final bridge = _ExploreBridge();
      final userData = UserData(coreBridge: bridge);
      addTearDown(userData.dispose);
      addTearDown(bridge.eventController.close);
      final first = ExploreController(
        coreBridge: bridge,
        userDataController: userData,
      );
      expect(first.weeklyExploration, isEmpty);
      expect(first.isLoading, isTrue);
      expect(bridge.recommendationCalls, 0);
      await first.load();
      expect(first.weeklyExploration, hasLength(8));
      first.dispose();

      final reopened = ExploreController(
        coreBridge: bridge,
        userDataController: userData,
      );
      addTearDown(reopened.dispose);
      expect(reopened.isLoading, isTrue);
      expect(reopened.isFolderLoading('user_exploration_weekly'), isTrue);
      expect(reopened.weeklyExploration, isEmpty);
      await reopened.load();
      expect(bridge.recommendationCalls, 2);
      expect(reopened.isLoading, isFalse);
      expect(reopened.weeklyExploration, hasLength(8));
    },
  );

  test('explore reads current Rust data when its page reopens', () async {
    final bridge = _ExploreBridge()..shouldReturnEmpty = true;
    final userData = UserData(coreBridge: bridge);
    addTearDown(userData.dispose);
    addTearDown(bridge.eventController.close);
    final first = ExploreController(
      coreBridge: bridge,
      userDataController: userData,
    );
    expect(first.weeklyTracks, isEmpty);
    await first.load();
    first.dispose();

    bridge.shouldReturnEmpty = false;
    bridge.eventController.add({'type': 'recommendations_changed'});
    await Future<void>.delayed(Duration.zero);

    final reopened = ExploreController(
      coreBridge: bridge,
      userDataController: userData,
    );
    addTearDown(reopened.dispose);
    expect(reopened.isLoading, isTrue);
    expect(reopened.weeklyTracks, isEmpty);
    await reopened.load();
    expect(reopened.isLoading, isFalse);
    expect(reopened.weeklyTracks.single.itemId, 'chart');
    expect(bridge.recommendationCalls, 2);
  });

  test('explore maps all five weekly folders', () async {
    final _ExploreBridge bridge = _ExploreBridge();
    final ExploreController controller = ExploreController(coreBridge: bridge);
    addTearDown(controller.dispose);
    await controller.load();
    expect(controller.weeklyExploration, hasLength(8));
    expect(
      controller.weeklyExploration.map((Track track) => track.itemId),
      unorderedEquals(
        List<String>.generate(8, (int index) => 'exploration-track-$index'),
      ),
    );
    expect(controller.weeklyAlbums.single.itemId, 'sitewide_top_albums_weekly');
    expect(controller.similarUsersJam, hasLength(8));
    expect(
      controller.similarUsersJam.map((Track track) => track.itemId),
      unorderedEquals(
        List<String>.generate(
          8,
          (int index) => 'similarusers_jam_latest-track-$index',
        ),
      ),
    );
    expect(bridge.playlistCalls, isEmpty);
    expect(controller.weeklyTracks.single.itemId, 'chart');
    expect(controller.weeklyArtists.single.itemId, 'artist');
    expect(controller.isLoading, isFalse);
    expect(controller.hasLoadError, isFalse);
  });

  test('exploration and similar users tracks shuffle once per page', () async {
    final ExploreController controller = ExploreController(
      coreBridge: _ExploreBridge(),
      random: math.Random(42),
    );
    addTearDown(controller.dispose);
    final math.Random expectedRandom = math.Random(42);
    final List<String> originalOrder = List<String>.generate(
      8,
      (int index) => 'exploration-track-$index',
    );
    final List<String> expectedOrder = List<String>.of(originalOrder)
      ..shuffle(expectedRandom);
    final List<String> originalJamOrder = List<String>.generate(
      8,
      (int index) => 'similarusers_jam_latest-track-$index',
    );
    final List<String> expectedJamOrder = List<String>.of(originalJamOrder)
      ..shuffle(expectedRandom);
    expect(expectedOrder, isNot(orderedEquals(originalOrder)));
    expect(expectedJamOrder, isNot(orderedEquals(originalJamOrder)));

    for (int loadIndex = 0; loadIndex < 2; loadIndex++) {
      await controller.load();
      final List<String> actualOrder = controller.weeklyExploration
          .map((Track track) => track.itemId)
          .toList(growable: false);
      expect(actualOrder, expectedOrder);
      expect(
        controller.similarUsersJam.map((Track track) => track.itemId),
        expectedJamOrder,
      );
    }
  });

  test('failed refresh preserves loaded explore contents', () async {
    final _ExploreBridge bridge = _ExploreBridge();
    final ExploreController controller = ExploreController(coreBridge: bridge);
    addTearDown(controller.dispose);
    await controller.load();
    final List<Album> albums = controller.weeklyAlbums;
    final List<Track> jams = controller.similarUsersJam;
    bridge.shouldFail = true;
    await controller.load();
    expect(controller.weeklyAlbums, same(albums));
    expect(controller.similarUsersJam, same(jams));
    expect(controller.hasLoadError, isTrue);
    expect(controller.isLoading, isFalse);
  });

  test('empty weekly response produces empty sections', () async {
    final _ExploreBridge bridge = _ExploreBridge()..shouldReturnEmpty = true;
    final ExploreController controller = ExploreController(coreBridge: bridge);
    addTearDown(controller.dispose);
    await controller.load();
    expect(controller.weeklyExploration, isEmpty);
    expect(controller.weeklyAlbums, isEmpty);
    expect(controller.similarUsersJam, isEmpty);
    expect(controller.weeklyTracks, isEmpty);
    expect(controller.weeklyArtists, isEmpty);
    expect(controller.hasLoadError, isFalse);
  });

  testWidgets('explore reports failed loading without a refresh button', (
    WidgetTester tester,
  ) async {
    final _ExploreBridge bridge = _ExploreBridge()..shouldFail = true;
    final MashinCoreBridgeEvents events = MashinCoreBridgeEvents(
      coreBridge: bridge,
    );
    addTearDown(events.dispose);
    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: ExploreScreen(coreBridge: bridge, coreEvents: events),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(
      find.text('Empfehlungen konnten nicht geladen werden.'),
      findsOneWidget,
    );
    expect(find.byTooltip('Aktualisieren'), findsNothing);
    expect(bridge.recommendationCalls, 1);
    expect(tester.takeException(), isNull);
  });

  for (final bool isCompact in <bool>[true, false]) {
    testWidgets(
      'empty weekly data ends loading without an error on ${isCompact ? 'mobile' : 'desktop'}',
      (WidgetTester tester) async {
        tester.view.physicalSize = Size(isCompact ? 412 : 1200, 850);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final Completer<RecommendationSnapshot> pending =
            Completer<RecommendationSnapshot>();
        final _ExploreBridge bridge = _ExploreBridge()
          ..pendingRecommendations = pending;
        final MashinCoreBridgeEvents events = MashinCoreBridgeEvents(
          coreBridge: bridge,
        );
        addTearDown(events.dispose);
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark(),
            home: Scaffold(
              body: ExploreScreen(coreBridge: bridge, coreEvents: events),
            ),
          ),
        );
        if (isCompact) {
          expect(
            tester
                .widget<MediaSlideSectionMobile<Track>>(
                  find.byType(MediaSlideSectionMobile<Track>),
                )
                .isLoading,
            isTrue,
          );
        } else {
          expect(
            tester
                .widget<MediaSlideSectionDesktop<Track>>(
                  find.byType(MediaSlideSectionDesktop<Track>),
                )
                .isLoading,
            isTrue,
          );
        }
        expect(find.byTooltip('Aktualisieren'), findsNothing);
        pending.complete(const RecommendationSnapshot());
        await tester.pumpAndSettle();
        expect(
          find.text('Keine Weekly Exploration verfügbar.'),
          findsOneWidget,
        );
        expect(
          find.text('Empfehlungen konnten nicht geladen werden.'),
          findsNothing,
        );
        expect(find.byTooltip('Aktualisieren'), findsNothing);
        await tester.ensureVisible(find.text('Andere Nutzer hören'));
        await tester.pumpAndSettle();
        expect(
          find.text('Kein Weekly Jam ähnlicher Nutzer verfügbar.'),
          findsOneWidget,
        );
        await tester.ensureVisible(find.text('Artists der Woche'));
        await tester.pumpAndSettle();
        expect(find.text('Keine Artist-Charts verfügbar.'), findsOneWidget);
        expect(tester.takeException(), isNull);
      },
    );

    testWidgets(
      'explore orders weekly sections on ${isCompact ? 'mobile' : 'desktop'}',
      (WidgetTester tester) async {
        tester.view.physicalSize = Size(isCompact ? 412 : 1200, 2200);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final _ExploreBridge bridge = _ExploreBridge();
        final MashinCoreBridgeEvents events = MashinCoreBridgeEvents(
          coreBridge: bridge,
        );
        addTearDown(events.dispose);
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark(),
            home: Scaffold(
              body: ExploreScreen(coreBridge: bridge, coreEvents: events),
            ),
          ),
        );
        await tester.pumpAndSettle();
        final List<String> headings = <String>[
          'Entdecke etwas Neues',
          'Alben der Woche',
          'Andere Nutzer hören',
          'Artists der Woche',
          'Songs der Woche',
        ];
        double previous = -1;
        for (final String heading in headings) {
          final double position = tester.getTopLeft(find.text(heading)).dy;
          expect(position, greaterThan(previous));
          previous = position;
        }
        expect(
          find.byType(
            isCompact
                ? MediaSlideSectionMobile<Track>
                : MediaSlideSectionDesktop<Track>,
          ),
          findsOneWidget,
        );
        expect(
          find.byType(
            isCompact
                ? MediaRowSectionMobile<Album>
                : MediaRowSectionDesktop<Album>,
          ),
          findsOneWidget,
        );
        expect(find.text('Neuheiten'), findsNothing);
        expect(find.text('sitewide_releases_weekly'), findsNothing);
        final Finder heading = find.text('Entdecken');
        final Finder slider = find.byType(
          isCompact
              ? MediaSlideSectionMobile<Track>
              : MediaSlideSectionDesktop<Track>,
        );
        final Finder albums = find.byType(
          isCompact
              ? MediaRowSectionMobile<Album>
              : MediaRowSectionDesktop<Album>,
        );
        final Finder jams = find.byType(
          isCompact
              ? MediaTableSectionMobile<Track>
              : MediaTableSectionDesktop<Track>,
        );
        final Finder tracks = find.byType(
          isCompact
              ? MediaRowSectionMobile<Track>
              : MediaRowSectionDesktop<Track>,
        );
        final Finder artists = find.byType(
          isCompact
              ? MediaRowSectionMobile<Artist>
              : MediaRowSectionDesktop<Artist>,
        );
        expect(
          tester.getTopLeft(heading),
          Offset(isCompact ? 18 : 20, isCompact ? 20 : 18),
        );
        expect(tester.widget<Text>(heading).style?.fontWeight, FontWeight.w700);
        expect(tester.getTopLeft(slider).dx, isCompact ? 18 : 20);
        expect(
          tester.getTopLeft(slider).dy - tester.getBottomLeft(heading).dy,
          closeTo(20, 0.1),
        );
        final List<Finder> sections = <Finder>[
          slider,
          albums,
          jams,
          artists,
          tracks,
        ];
        for (int index = 1; index < sections.length; index++) {
          if (isCompact && (index == 2 || index == 3)) continue;
          expect(
            tester.getTopLeft(sections[index]).dy -
                tester.getBottomLeft(sections[index - 1]).dy,
            closeTo(isCompact ? 32 : 20, 0.1),
          );
        }
        expect(find.byTooltip('Aktualisieren'), findsNothing);
        expect(find.text('Weekly Mix'), findsNothing);
        expect(find.text('Similar Listener Weekly Jams'), findsNothing);
        expect(find.textContaining('exploration Track '), findsWidgets);
        if (isCompact) {
          final MediaTableSectionMobile<Track> table = tester.widget(jams);
          expect(table.startsTrackRadio, isFalse);
          expect(
            tester
                .widget<MediaSlideSectionMobile<Track>>(slider)
                .startsTrackRadio,
            isTrue,
          );
          expect(
            tester
                .widget<MediaRowSectionMobile<Track>>(tracks)
                .startsTrackRadio,
            isTrue,
          );
          expect(table.maxItemsPerPage, 3);
          expect(table.items, hasLength(8));
        } else {
          final MediaTableSectionDesktop<Track> table = tester.widget(jams);
          expect(table.startsTrackRadio, isFalse);
          expect(
            tester
                .widget<MediaSlideSectionDesktop<Track>>(slider)
                .startsTrackRadio,
            isTrue,
          );
          expect(
            tester
                .widget<MediaRowSectionDesktop<Track>>(tracks)
                .startsTrackRadio,
            isTrue,
          );
          expect(table.maxItemsPerPage, 5);
          expect(table.items, hasLength(8));
        }
        expect(tester.takeException(), isNull);
      },
    );
  }
}
