import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_events.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/home/home_screen_desktop.dart';

class _HomeCoreBridge implements MashinCoreBridgeApi {
  @override
  Future<List<RecommendationFolder>> getRecommendations() async {
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

    return <RecommendationFolder>[
      RecommendationFolder(
        itemId: 'recommendations',
        provider: 'listenbrainz_recommendations--test',
        title: 'Empfehlungen',
        items: <MediaItem>[tracks.first],
      ),
      RecommendationFolder(
        itemId: 'genre_playlists',
        provider: 'listenbrainz_recommendations--test',
        title: 'Genres',
        items: genres,
      ),
      RecommendationFolder(
        itemId: 'top_tracks',
        provider: 'listenbrainz_recommendations--test',
        title: 'Tracks',
        items: tracks,
      ),
    ];
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
