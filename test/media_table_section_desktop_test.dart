import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_events.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/shared/selection/selection_controller.dart';
import 'package:mashin_flutter/features/shared/widgets/media_image.dart';
import 'package:mashin_flutter/features/shared/widgets/media_row_section_desktop.dart';
import 'package:mashin_flutter/features/shared/widgets/media_row_section_mobile.dart';
import 'package:mashin_flutter/features/shared/widgets/media_slide_section_desktop.dart';
import 'package:mashin_flutter/features/shared/widgets/media_slide_section_mobile.dart';
import 'package:mashin_flutter/features/shared/widgets/media_table_section_desktop.dart';
import 'package:mashin_flutter/features/shared/widgets/media_table_section_mobile.dart';

class _UnusedCoreBridge implements MashinCoreBridgeApi {
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class _PlaybackCoreBridge extends _UnusedCoreBridge {
  final List<List<MediaItem>> radioRequests = <List<MediaItem>>[];
  final List<List<MediaItem>> playRequests = <List<MediaItem>>[];

  @override
  Future<void> startRadio(List<MediaItem> mediaItems) async {
    radioRequests.add(mediaItems);
  }

  @override
  Future<void> queuePlayMedia(
    List<MediaItem> mediaItems, {
    QueueOption option = QueueOption.replace,
    int? startItem,
    bool shuffle = false,
    String? providerInstance,
  }) async {
    playRequests.add(mediaItems);
  }
}

class _ControlledDeferredScrollPhysics extends ClampingScrollPhysics {
  const _ControlledDeferredScrollPhysics({
    required this.shouldDefer,
    super.parent,
  });

  final ValueNotifier<bool> shouldDefer;

  @override
  _ControlledDeferredScrollPhysics applyTo(ScrollPhysics? ancestor) =>
      _ControlledDeferredScrollPhysics(
        shouldDefer: shouldDefer,
        parent: buildParent(ancestor),
      );

  @override
  bool recommendDeferredLoading(
    double velocity,
    ScrollMetrics metrics,
    BuildContext context,
  ) => shouldDefer.value;
}

void main() {
  // Primary playback contracts across mobile and desktop section layouts.
  for (final String layout in <String>[
    'mobile slider',
    'desktop slider',
    'mobile row',
    'desktop row',
    'mobile table',
    'desktop table',
  ]) {
    for (final bool startsTrackRadio in <bool>[false, true]) {
      testWidgets('$layout primary playback radio=$startsTrackRadio', (
        WidgetTester tester,
      ) async {
        tester.view.physicalSize = const Size(1200, 800);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final _PlaybackCoreBridge bridge = _PlaybackCoreBridge();
        final MashinCoreBridgeEvents events = MashinCoreBridgeEvents(
          coreBridge: bridge,
        );
        addTearDown(events.dispose);
        final List<Track> tracks = List<Track>.generate(
          3,
          (int index) => Track(
            itemId: 'playback-$index',
            provider: 'test',
            name: 'Playback $index',
            duration: 120,
            artists: const <Artist>[],
          ),
        );
        final Widget section = switch (layout) {
          'mobile slider' => MediaSlideSectionMobile<Track>(
            title: 'Tracks',
            items: tracks,
            isLoading: false,
            emptyText: 'Empty',
            coreBridge: bridge,
            coreEvents: events,
            startsTrackRadio: startsTrackRadio,
          ),
          'desktop slider' => MediaSlideSectionDesktop<Track>(
            title: 'Tracks',
            items: tracks,
            isLoading: false,
            emptyText: 'Empty',
            coreBridge: bridge,
            coreEvents: events,
            startsTrackRadio: startsTrackRadio,
          ),
          'mobile row' => MediaRowSectionMobile<Track>(
            title: 'Tracks',
            items: tracks,
            isLoading: false,
            emptyText: 'Empty',
            coreBridge: bridge,
            coreEvents: events,
            startsTrackRadio: startsTrackRadio,
          ),
          'desktop row' => MediaRowSectionDesktop<Track>(
            title: 'Tracks',
            items: tracks,
            isLoading: false,
            emptyText: 'Empty',
            coreBridge: bridge,
            coreEvents: events,
            startsTrackRadio: startsTrackRadio,
          ),
          'mobile table' => CustomScrollView(
            slivers: <Widget>[
              MediaTableSectionMobile<Track>(
                title: 'Tracks',
                items: tracks,
                isLoading: false,
                emptyText: 'Empty',
                coreBridge: bridge,
                coreEvents: events,
                startsTrackRadio: startsTrackRadio,
              ),
            ],
          ),
          _ => MediaTableSectionDesktop<Track>(
            title: 'Tracks',
            items: tracks,
            isLoading: false,
            emptyText: 'Empty',
            coreBridge: bridge,
            coreEvents: events,
            startsTrackRadio: startsTrackRadio,
          ),
        };
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark(),
            home: Scaffold(body: section),
          ),
        );
        await tester.pumpAndSettle();
        await tester.tap(find.byIcon(Icons.play_arrow).first);
        await tester.pump();
        if (startsTrackRadio) {
          expect(bridge.radioRequests.single, <MediaItem>[tracks.first]);
          expect(bridge.playRequests, isEmpty);
        } else {
          expect(bridge.playRequests.single, tracks);
          expect(bridge.radioRequests, isEmpty);
        }
        bridge.radioRequests.clear();
        bridge.playRequests.clear();
        if (layout.endsWith('table')) {
          await tester.tap(find.text('Playback 1'));
          if (layout == 'desktop table') {
            await tester.pump(const Duration(milliseconds: 50));
            await tester.tap(find.text('Playback 1'));
          }
          await tester.pumpAndSettle();
          if (startsTrackRadio) {
            expect(bridge.radioRequests.single, <MediaItem>[tracks[1]]);
            expect(bridge.playRequests, isEmpty);
          } else {
            expect(bridge.playRequests.single, tracks.sublist(1));
            expect(bridge.radioRequests, isEmpty);
          }
        } else if (startsTrackRadio && layout.startsWith('mobile')) {
          await tester.tap(find.text('Playback 0'));
          await tester.pump();
          expect(bridge.radioRequests.single, <MediaItem>[tracks.first]);
          expect(bridge.playRequests, isEmpty);
        }
        expect(tester.takeException(), isNull);
      });
    }
  }

  testWidgets('rapid desktop wheel scroll loads newly visible covers', (
    WidgetTester tester,
  ) async {
    final MashinCoreBridgeApi coreBridge = _UnusedCoreBridge();
    final MashinCoreBridgeEvents coreEvents = MashinCoreBridgeEvents(
      coreBridge: coreBridge,
    );
    addTearDown(coreEvents.dispose);
    final List<Track> tracks = List<Track>.generate(
      100,
      (int index) => Track(
        itemId: 'wheel-track-$index',
        provider: 'test',
        name: 'Wheel track $index',
        duration: 120,
        artists: const <Artist>[],
        image: const MediaItemImage(
          type: 'thumb',
          path:
              'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Hlr8AAAAASUVORK5CYII=',
          provider: 'test',
          remotelyAccessible: false,
        ),
      ),
    );

    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: CustomScrollView(
            slivers: <Widget>[
              MediaTableSectionDesktop<Track>(
                title: ' ',
                items: tracks,
                isLoading: false,
                emptyText: 'Leer',
                maxItemsPerPage: 0,
                coreBridge: coreBridge,
                coreEvents: coreEvents,
              ),
            ],
          ),
        ),
      ),
    );

    await tester.sendEventToBinding(
      PointerScrollEvent(
        position: tester.getCenter(find.byType(CustomScrollView)),
        scrollDelta: const Offset(0, 1600),
      ),
    );
    await tester.pump();

    final Finder visibleTrack = find.text('Wheel track 25');
    expect(visibleTrack, findsOneWidget);
    expect(find.byType(MediaTableCover), findsWidgets);
    expect(find.byType(MediaImage), findsWidgets);

    for (int index = 0; index < 2; index++) {
      await tester.sendEventToBinding(
        PointerScrollEvent(
          position: tester.getCenter(find.byType(CustomScrollView)),
          scrollDelta: const Offset(0, 120),
        ),
      );
      await tester.pump();
    }
    expect(find.byType(MediaImage), findsWidgets);
    expect(tester.takeException(), isNull);
  });

  testWidgets('unpaged covers load despite deferred scroll recommendation', (
    WidgetTester tester,
  ) async {
    final MashinCoreBridgeApi coreBridge = _UnusedCoreBridge();
    final MashinCoreBridgeEvents coreEvents = MashinCoreBridgeEvents(
      coreBridge: coreBridge,
    );
    final ValueNotifier<bool> shouldDefer = ValueNotifier<bool>(true);
    addTearDown(coreEvents.dispose);
    addTearDown(shouldDefer.dispose);
    final Track track = Track(
      itemId: 'track-with-cover',
      provider: 'test',
      name: 'Track with cover',
      duration: 120,
      artists: const <Artist>[],
      image: const MediaItemImage(
        type: 'thumb',
        path:
            'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Hlr8AAAAASUVORK5CYII=',
        provider: 'test',
        remotelyAccessible: false,
      ),
    );

    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: CustomScrollView(
            physics: _ControlledDeferredScrollPhysics(shouldDefer: shouldDefer),
            slivers: <Widget>[
              MediaTableSectionDesktop<Track>(
                title: ' ',
                items: <Track>[track],
                isLoading: false,
                emptyText: 'Leer',
                maxItemsPerPage: 0,
                coreBridge: coreBridge,
                coreEvents: coreEvents,
              ),
            ],
          ),
        ),
      ),
    );

    final Finder cover = find.byType(MediaTableCover);
    final Finder image = find.descendant(
      of: cover,
      matching: find.byType(MediaImage),
    );
    expect(image, findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('paged desktop table keeps navigation and expansion', (
    WidgetTester tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final MashinCoreBridgeApi coreBridge = _UnusedCoreBridge();
    final MashinCoreBridgeEvents coreEvents = MashinCoreBridgeEvents(
      coreBridge: coreBridge,
    );
    addTearDown(coreEvents.dispose);
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

    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: SingleChildScrollView(
            child: MediaTableSectionDesktop<Track>(
              title: 'Titel',
              items: tracks,
              isLoading: false,
              emptyText: 'Leer',
              maxItemsPerPage: 10,
              coreBridge: coreBridge,
              coreEvents: coreEvents,
            ),
          ),
        ),
      ),
    );

    expect(find.text('Track 0'), findsOneWidget);
    expect(find.text('Track 10'), findsNothing);

    await tester.tap(find.byTooltip('Nächste Seite'));
    await tester.pumpAndSettle();
    expect(find.text('Track 10'), findsOneWidget);
    expect(find.text('Track 0'), findsNothing);

    await tester.tap(find.byTooltip('Alle anzeigen'));
    await tester.pumpAndSettle();
    expect(find.text('Track 0'), findsOneWidget);
    expect(find.text('Track 11'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('unpaged desktop table builds rows lazily in page scroll', (
    WidgetTester tester,
  ) async {
    final MashinCoreBridgeApi coreBridge = _UnusedCoreBridge();
    final MashinCoreBridgeEvents coreEvents = MashinCoreBridgeEvents(
      coreBridge: coreBridge,
    );
    addTearDown(coreEvents.dispose);
    final List<Track> tracks = List<Track>.generate(
      100,
      (int index) => Track(
        itemId: 'track-$index',
        provider: 'test',
        name: 'Track $index',
        duration: 120,
        artists: const <Artist>[],
      ),
    );

    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: CustomScrollView(
            slivers: <Widget>[
              const SliverToBoxAdapter(
                child: SizedBox(height: 200, child: Text('Playlist header')),
              ),
              MediaTableSectionDesktop<Track>(
                title: ' ',
                items: tracks,
                isLoading: false,
                emptyText: 'Leer',
                maxItemsPerPage: 0,
                coreBridge: coreBridge,
                coreEvents: coreEvents,
              ),
            ],
          ),
        ),
      ),
    );

    expect(tester.takeException(), isNull);
    expect(find.text('Track 0'), findsOneWidget);
    expect(find.text('Track 99'), findsNothing);

    await tester.drag(find.byType(CustomScrollView), const Offset(0, -7000));
    await tester.pumpAndSettle();

    expect(tester.takeException(), isNull);
    expect(find.text('Track 99'), findsOneWidget);
    expect(find.text('Playlist header'), findsNothing);
  });

  testWidgets('queue sliver preserves scroll controller and selection', (
    WidgetTester tester,
  ) async {
    final MashinCoreBridgeApi coreBridge = _UnusedCoreBridge();
    final MashinCoreBridgeEvents coreEvents = MashinCoreBridgeEvents(
      coreBridge: coreBridge,
    );
    final ScrollController scrollController = ScrollController();
    addTearDown(coreEvents.dispose);
    addTearDown(scrollController.dispose);
    final List<QueueItem> queueItems = List<QueueItem>.generate(
      100,
      (int index) => QueueItem(
        queueItemId: 'queue-$index',
        mediaItem: Track(
          itemId: 'track-$index',
          provider: 'test',
          name: 'Queue track $index',
          duration: 120,
          artists: const <Artist>[],
        ),
      ),
    );
    List<QueueItem> selectedItems = <QueueItem>[];

    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: Column(
            children: <Widget>[
              const Text('Warteschlange'),
              Expanded(
                child: CustomScrollView(
                  controller: scrollController,
                  slivers: <Widget>[
                    MediaTableSectionDesktop<QueueItem>(
                      title: ' ',
                      items: queueItems,
                      isLoading: false,
                      emptyText: 'Leer',
                      maxItemsPerPage: 0,
                      coreBridge: coreBridge,
                      coreEvents: coreEvents,
                      onSelectionChanged: (List<QueueItem> items) =>
                          selectedItems = items,
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );

    expect(find.text('Warteschlange'), findsOneWidget);
    expect(find.text('Queue track 99'), findsNothing);
    await tester.tap(find.text('Queue track 0'));
    await tester.pump();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyA);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pump();
    expect(selectedItems.length, 100);

    scrollController.jumpTo(100 * 64);
    await tester.pumpAndSettle();
    expect(find.text('Queue track 99'), findsOneWidget);
    expect(find.text('Warteschlange'), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pump();
    expect(selectedItems, isEmpty);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyA);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pump();
    expect(selectedItems.length, 100);
    expect(tester.takeException(), isNull);
  });

  testWidgets('closing queue selection during route build waits for frame', (
    WidgetTester tester,
  ) async {
    final MashinCoreBridgeApi coreBridge = _UnusedCoreBridge();
    final MashinCoreBridgeEvents coreEvents = MashinCoreBridgeEvents(
      coreBridge: coreBridge,
    );
    addTearDown(coreEvents.dispose);
    final QueueItem queueItem = QueueItem(
      queueItemId: 'queue-1',
      mediaItem: Track(
        itemId: 'track-1',
        provider: 'test',
        name: 'Queue track',
        duration: 120,
        artists: const <Artist>[],
      ),
    );

    Widget buildPage({required bool closeDuringBuild}) => MaterialApp(
      theme: AppTheme.dark(),
      home: Scaffold(
        body: Column(
          children: <Widget>[
            Expanded(
              child: CustomScrollView(
                slivers: <Widget>[
                  MediaTableSectionDesktop<QueueItem>(
                    title: ' ',
                    items: <QueueItem>[queueItem],
                    isLoading: false,
                    emptyText: 'Leer',
                    maxItemsPerPage: 0,
                    coreBridge: coreBridge,
                    coreEvents: coreEvents,
                  ),
                ],
              ),
            ),
            Builder(
              builder: (BuildContext context) {
                if (closeDuringBuild) {
                  SelectionController.instance.onClose();
                }
                return const SizedBox.shrink();
              },
            ),
          ],
        ),
      ),
    );

    await tester.pumpWidget(buildPage(closeDuringBuild: false));
    await tester.tap(find.text('Queue track'));
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyA);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pump();
    expect(SelectionController.instance.isVisible, isTrue);

    await tester.pumpWidget(buildPage(closeDuringBuild: true));
    expect(tester.takeException(), isNull);
    expect(SelectionController.instance.isVisible, isFalse);
    await tester.pump(const Duration(milliseconds: 50));
  });
}
