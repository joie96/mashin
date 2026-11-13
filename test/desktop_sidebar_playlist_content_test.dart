import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/shell/app_shell.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_events.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/core/core_userdata.dart';
import 'package:mashin/features/queue/queue_overlay_desktop.dart';

class _CurrentTrackFavoritesBridge extends Fake implements MashinCoreBridgeApi {
  bool isFavorite = false;
  final commands = <({bool isFavorite, Completer<void> completion})>[];

  @override
  Stream<Map<String, dynamic>> get events => const Stream.empty();

  @override
  Future<void> setUserDataFavorites(List<MediaItem> items, bool favorite) {
    final completion = Completer<void>();
    commands.add((isFavorite: favorite, completion: completion));
    return completion.future;
  }

  @override
  Future<Map<String, dynamic>> getUserData() async => {
    'has_loaded_favorites': true,
    'favorites': {
      'tracks': [
        if (isFavorite)
          {
            'item_id': 'current-track',
            'provider': 'library',
            'name': 'Current track',
          },
      ],
    },
  };
}

void main() {
  for (final isQueueOverlay in [false, true]) {
    testWidgets(
      'desktop current track favorite updates without playback changes, queue=$isQueueOverlay',
      (tester) async {
        tester.view.physicalSize = const Size(1600, 900);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final bridge = _CurrentTrackFavoritesBridge();
        final userData = UserData(coreBridge: bridge);
        final events = MashinCoreBridgeEvents(coreBridge: bridge);
        addTearDown(userData.dispose);
        addTearDown(events.dispose);
        await userData.refreshLibrarySnapshot();
        const track = Track(
          itemId: 'current-track',
          provider: 'library',
          name: 'Current track',
          duration: 180,
          artists: <Artist>[],
        );
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.light(),
            home: Scaffold(
              body: isQueueOverlay
                  ? QueueOverlayDesktop(
                      trackTitle: 'Current track',
                      trackSubtitle: '',
                      trackCoverUrl: null,
                      queueTrackCountText: '1 Track',
                      queueTotalDurationText: '3 Min.',
                      dontStopTheMusicEnabled: false,
                      queueItems: const [
                        QueueItem(queueItemId: 'current', mediaItem: track),
                      ],
                      currentQueueIndex: 0,
                      coreBridge: bridge,
                      coreEvents: events,
                      userDataController: userData,
                      onClose: () {},
                      onToggleDontStopTheMusic: () async {},
                    )
                  : DesktopCurrentTrackInfo(
                      trackTitle: 'Current track',
                      trackSubtitle: '',
                      currentTrackArtist: null,
                      trackCoverUrl: null,
                      currentTrack: track,
                      userDataController: userData,
                      onOpenArtist: null,
                    ),
            ),
          ),
        );

        await tester.tap(
          isQueueOverlay
              ? find.byIcon(Icons.favorite_border_outlined).last
              : find.byIcon(Icons.favorite_border_outlined).first,
        );
        await tester.pump();
        expect(bridge.commands.single.completion.isCompleted, isFalse);
        expect(bridge.commands.single.isFavorite, isTrue);
        expect(
          find.byIcon(Icons.favorite_outlined),
          findsNWidgets(isQueueOverlay ? 2 : 1),
        );

        await tester.tap(
          isQueueOverlay
              ? find.byIcon(Icons.favorite_outlined).last
              : find.byIcon(Icons.favorite_outlined).first,
        );
        await tester.pump();
        expect(bridge.commands.last.completion.isCompleted, isFalse);
        expect(bridge.commands.last.isFavorite, isFalse);
        expect(
          find.byIcon(Icons.favorite_border_outlined),
          findsNWidgets(isQueueOverlay ? 3 : 1),
        );

        bridge.commands.last.completion.complete();
        await tester.pump();
        bridge.commands.first.completion.complete();
        await tester.pump();
        await tester.pump();
        expect(
          find.byIcon(Icons.favorite_border_outlined),
          findsNWidgets(isQueueOverlay ? 3 : 1),
        );

        bridge.isFavorite = true;
        await userData.refreshLibrarySnapshot();
        await tester.pump();
        expect(
          find.byIcon(Icons.favorite_outlined),
          findsNWidgets(isQueueOverlay ? 2 : 1),
        );
        await tester.pumpWidget(const SizedBox.shrink());
        await tester.pump(const Duration(milliseconds: 400));
      },
    );
  }

  for (final scale in <double>[1, 2, 3]) {
    testWidgets('playlist prototype fits text at scale $scale', (tester) async {
      final controller = ScrollController();
      addTearDown(controller.dispose);

      Widget row(String title, String metadata, {Key? key}) => Padding(
        key: key,
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
        child: DesktopSidebarPlaylistContent(
          title: title,
          metadata: metadata,
          cover: const SizedBox.shrink(),
        ),
      );

      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.light(),
          home: MediaQuery(
            data: MediaQueryData(textScaler: TextScaler.linear(scale)),
            child: Scaffold(
              body: SizedBox(
                width: 250,
                height: 300,
                child: ListView.builder(
                  controller: controller,
                  padding: const EdgeInsets.symmetric(horizontal: 10),
                  prototypeItem: row('Playlist', '0 Titel'),
                  itemCount: 100,
                  itemBuilder: (_, index) => row(
                    'Playlist $index with a long title',
                    '1000 Titel • 100 Std. 59 Min.',
                    key: ValueKey(index),
                  ),
                ),
              ),
            ),
          ),
        ),
      );

      final firstRow = find.byKey(const ValueKey(0));
      final rowHeight = tester.getSize(firstRow).height;
      if (scale == 1) {
        expect(rowHeight, 52);
      } else {
        expect(rowHeight, greaterThan(52));
      }
      for (final text in tester.widgetList<Text>(
        find.descendant(of: firstRow, matching: find.byType(Text)),
      )) {
        final bounds = tester.getRect(find.byWidget(text));
        final rowBounds = tester.getRect(firstRow);
        expect(bounds.top, greaterThanOrEqualTo(rowBounds.top + 6));
        expect(bounds.bottom, lessThanOrEqualTo(rowBounds.bottom - 6));
      }
      expect(
        controller.position.maxScrollExtent,
        closeTo(100 * rowHeight - 300, 0.001),
      );
      controller.jumpTo(controller.position.maxScrollExtent);
      await tester.pump();
      expect(find.byKey(const ValueKey(99)), findsOneWidget);
      expect(tester.getSize(find.byKey(const ValueKey(99))).height, rowHeight);
      expect(tester.takeException(), isNull);
    });
  }
}
