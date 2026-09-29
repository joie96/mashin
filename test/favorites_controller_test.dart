import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_events.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/core/core_userdata.dart';
import 'package:mashin_flutter/features/favorites/favorites_controller.dart';
import 'package:mashin_flutter/features/favorites/favorites_screen_desktop.dart';

void main() {
  test(
    'passes the artist to the radio command without separate playback',
    () async {
      final coreBridge = _FakeFavoritesCoreBridge();
      final controller = FavoritesController(
        coreBridge: coreBridge,
        userDataController: _FakeUserDataController(),
      );
      addTearDown(controller.dispose);

      final artist = Artist(
        itemId: 'artist-1',
        provider: 'library',
        name: 'Artist',
      );
      await controller.startArtistRadio(artist);

      expect(coreBridge.playedItems, isEmpty);
      expect(coreBridge.radioSeeds, <MediaItem>[artist]);
    },
  );

  testWidgets('desktop favorites keeps scroll extent stable', (
    WidgetTester tester,
  ) async {
    tester.view.physicalSize = const Size(1100, 320);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final _FakeFavoritesCoreBridge coreBridge = _FakeFavoritesCoreBridge();
    final MashinCoreBridgeEvents coreEvents = MashinCoreBridgeEvents(
      coreBridge: coreBridge,
    );
    addTearDown(coreEvents.dispose);

    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: FavoritesScreenDesktop(
            coreBridge: coreBridge,
            coreEvents: coreEvents,
            userDataController: _FakeUserDataController(),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

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

class _FakeFavoritesCoreBridge extends Fake implements MashinCoreBridgeApi {
  List<MediaItem> playedItems = const <MediaItem>[];
  List<MediaItem> radioSeeds = const <MediaItem>[];

  @override
  Future<void> queuePlayMedia(
    List<MediaItem> items, {
    QueueOption option = QueueOption.play,
    int? startItem,
    bool shuffle = false,
    bool startDownload = true,
  }) async {
    playedItems = items;
  }

  @override
  Future<void> startSendspinRadio(List<MediaItem> items) async {
    radioSeeds = items;
  }
}

class _FakeUserDataController extends Fake implements UserData {
  @override
  bool get isLoadingPreferences => false;

  @override
  List<Track> get favoriteTracks => List<Track>.generate(
    12,
    (int index) => Track(
      itemId: 'track-$index',
      provider: 'test',
      name: 'Track $index',
      duration: 120,
      artists: const <Artist>[],
    ),
  );

  @override
  List<Album> get favoriteAlbums => const <Album>[];

  @override
  List<Playlist> get favoritePlaylists => const <Playlist>[];

  @override
  List<Artist> get favoriteArtists => const <Artist>[];

  @override
  void addListener(VoidCallback listener) {}

  @override
  void removeListener(VoidCallback listener) {}
}
