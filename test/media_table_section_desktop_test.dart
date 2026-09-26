import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_events.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/shared/widgets/media_image.dart';
import 'package:mashin_flutter/features/shared/widgets/media_table_section_desktop.dart';

class _UnusedCoreBridge implements MashinCoreBridgeApi {
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
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
}
