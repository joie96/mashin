import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_models.dart' hide RepeatMode;
import 'package:mashin_flutter/features/playerbar/playerbar_overlay_mobile.dart';
import 'package:mashin_flutter/features/shared/widgets/media_image.dart';

const String _previousCover = 'data:image/previous;base64,';
const String _currentCover = 'data:image/current;base64,';
const String _nextCover = 'data:image/next;base64,';

QueueItem _item(String id, String cover) => QueueItem(
  queueItemId: id,
  mediaItem: MediaItem(
    itemId: id,
    provider: 'test',
    name: id,
    mediaType: MediaType.track,
    providerMappings: const <ProviderMapping>[],
    metadata: MediaItemMetadata(
      images: <MediaItemImage>[
        MediaItemImage(
          type: 'thumb',
          path: cover,
          provider: 'test',
          remotelyAccessible: false,
        ),
      ],
      genres: const <String>[],
    ),
  ),
);

void main() {
  final List<QueueItem> items = <QueueItem>[
    _item('previous', _previousCover),
    _item('current', _currentCover),
    _item('next', _nextCover),
  ];

  testWidgets('swipe previews adjacent artwork across the viewport', (
    WidgetTester tester,
  ) async {
    tester.view.physicalSize = const Size(400, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    int previousCalls = 0;
    int nextCalls = 0;
    double queueDragDistance = 0;
    Widget overlay(int index, {bool shouldShowMultiDeviceButton = true}) =>
        MaterialApp(
          theme: AppTheme.dark(),
          home: Scaffold(
            body: PlayerBarOverlayMobile(
              shouldShowMultiDeviceButton: shouldShowMultiDeviceButton,
              isOpen: true,
              openProgress: 1,
              trackTitle: 'Current',
              trackSubtitle: 'Artist',
              trackCoverUrl: <String>[
                _previousCover,
                _currentCover,
                _nextCover,
              ][index],
              currentTrack: null,
              playbackStateType: PlayState.playing,
              playbackState: const PlayerState(),
              playbackQueue: Queue(items: items, currentIndex: index),
              maxSeconds: 120,
              currentSeconds: 10,
              dontStopTheMusicEnabled: false,
              userDataController: null,
              onClose: () {},
              onOpenQueue: () {},
              onQueueDragUpdate: (double distance, _) {
                queueDragDistance += distance;
              },
              onQueueDragEnd: (_) {},
              onTogglePlayPause: () async {},
              onPrevious: () async {
                previousCalls++;
              },
              onNext: () async {
                nextCalls++;
              },
              onToggleShuffle: () async {},
              onToggleRepeatMode: () async {},
              onToggleDontStopTheMusic: () async {},
              onToggleMute: () async {},
              onSeek: (_) async {},
              onOpenAudioOptions: (_) {},
            ),
          ),
        );
    Finder cover(String source) => find.byWidgetPredicate(
      (Widget widget) => widget is MediaImage && widget.source == source,
    );

    await tester.pumpWidget(overlay(0));
    expect(find.byIcon(Icons.devices_outlined), findsOneWidget);
    final Offset start = tester.getCenter(cover(_previousCover));
    final double initialLeft = tester.getTopLeft(cover(_previousCover)).dx;
    final TestGesture gesture = await tester.startGesture(start);
    await gesture.moveBy(const Offset(-25, 0));
    await gesture.moveBy(const Offset(-120, 0));
    await tester.pump();

    expect(
      tester.getTopLeft(cover(_previousCover)).dx,
      closeTo(initialLeft - 120, 2),
    );
    expect(cover(_currentCover), findsOneWidget);
    expect(
      tester.getTopLeft(cover(_currentCover)).dx -
          tester.getTopLeft(cover(_previousCover)).dx,
      closeTo(400, 2),
    );
    expect(
      tester.getTopLeft(cover(_currentCover)).dx -
          tester.getTopRight(cover(_previousCover)).dx,
      closeTo(16, 2),
    );
    expect(nextCalls, 0);

    await gesture.moveBy(const Offset(-130, 0));
    await gesture.up();
    await tester.pump(const Duration(milliseconds: 80));
    final double pageBeforeNextGesture = tester
        .widget<PageView>(find.byType(PageView))
        .controller!
        .page!;
    expect(pageBeforeNextGesture, greaterThan(0.5));
    final TestGesture nextGesture = await tester.startGesture(
      tester.getCenter(cover(_currentCover)),
    );
    expect(nextCalls, 1);
    expect(
      tester.widget<PageView>(find.byType(PageView)).controller!.page!,
      closeTo(1, 0.01),
    );
    await nextGesture.moveBy(const Offset(-25, 0));
    await nextGesture.moveBy(const Offset(-250, 0));
    await tester.pump();
    expect(
      tester.widget<PageView>(find.byType(PageView)).controller!.page!,
      greaterThan(pageBeforeNextGesture),
    );
    await nextGesture.up();
    await tester.pumpAndSettle();
    expect(
      tester.widget<PageView>(find.byType(PageView)).controller!.page!,
      closeTo(2, 0.01),
    );
    expect(nextCalls, 2);
    expect(previousCalls, 0);
    expect(tester.getTopLeft(cover(_nextCover)).dx, closeTo(8, 2));

    await tester.pumpWidget(overlay(1));
    expect(tester.getTopLeft(cover(_nextCover)).dx, closeTo(8, 2));
    await tester.pumpWidget(overlay(2));
    expect(tester.getTopLeft(cover(_nextCover)).dx, closeTo(8, 2));
    await tester.pump();
    expect(tester.getTopLeft(cover(_nextCover)).dx, closeTo(8, 2));

    await tester.drag(cover(_nextCover), const Offset(-300, 0));
    await tester.pumpAndSettle();
    expect(nextCalls, 2);

    await tester.drag(cover(_nextCover), const Offset(40, 0));
    await tester.pumpAndSettle();
    expect(previousCalls, 0);

    await tester.drag(cover(_nextCover), const Offset(0, -80));
    await tester.pumpAndSettle();
    expect(queueDragDistance, greaterThan(0));

    await tester.drag(cover(_nextCover), const Offset(250, 0));
    await tester.pumpAndSettle();
    expect(previousCalls, 1);

    await tester.pumpWidget(overlay(1, shouldShowMultiDeviceButton: false));
    await tester.pumpAndSettle();
    expect(find.byIcon(Icons.devices_outlined), findsNothing);
    await tester.pumpWidget(overlay(1));
    await tester.pumpAndSettle();
    expect(find.byIcon(Icons.devices_outlined), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
