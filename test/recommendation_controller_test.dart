import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_symbols_icons/symbols.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_events.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/explore/explore_controller.dart';
import 'package:mashin/features/explore/explore_screen_desktop.dart';
import 'package:mashin/features/explore/explore_screen_mobile.dart';
import 'package:mashin/features/home/home_controller.dart';
import 'package:mashin/features/home/home_screen_desktop.dart';
import 'package:mashin/features/home/home_screen_mobile.dart';
import 'package:mashin/features/shared/context_menu/context_menu_controller.dart';
import 'package:mashin/features/shared/selection/selection_controller.dart';
import 'package:mashin/features/shared/selection/mobile_section_selection.dart';
import 'package:mashin/features/shared/widgets/media_list_section_desktop.dart';
import 'package:mashin/features/shared/widgets/desktop_item_action.dart';
import 'package:mashin/features/shared/widgets/media_list_section_mobile.dart';
import 'package:mashin/features/shared/widgets/media_link_text.dart';
import 'package:mashin/features/shared/widgets/detail_provider_switcher.dart';
import 'package:mashin/features/shared/widgets/media_row_section_desktop.dart';
import 'package:mashin/features/shared/widgets/media_row_section_mobile.dart';
import 'package:mashin/features/shared/widgets/media_slide_section_desktop.dart';
import 'package:mashin/features/shared/widgets/media_slide_section_mobile.dart';
import 'package:mashin/features/shared/widgets/media_table_section_desktop.dart';
import 'package:mashin/features/shared/widgets/media_table_section_mobile.dart';
import 'package:mashin/features/shared/widgets/media_track_row_skeleton.dart';
import 'package:mashin/features/shared/widgets/mood_playlist_cover.dart';
import 'package:mashin/features/shared/widgets/media_image.dart';

void main() {
  for (final isDark in [false, true]) {
    for (final width in [280.0, 380.0]) {
      testWidgets(
        'shared track skeleton follows table geometry: dark=$isDark width=$width',
        (tester) async {
          for (final isFirst in [true, false]) {
            await tester.pumpWidget(
              MaterialApp(
                theme: isDark ? AppTheme.dark() : AppTheme.light(),
                home: Scaffold(
                  body: Align(
                    alignment: Alignment.topLeft,
                    child: SizedBox(
                      width: width,
                      child: MediaTrackRowSkeleton(isFirst: isFirst),
                    ),
                  ),
                ),
              ),
            );
            await tester.pumpAndSettle();
            final row = find.byType(MediaTrackRowSkeleton);
            final colors = tester.element(row).mashinColors;
            final boxes = find.descendant(
              of: row,
              matching: find.byWidgetPredicate(
                (widget) =>
                    widget is Container &&
                    widget.decoration is BoxDecoration &&
                    (widget.decoration as BoxDecoration).color ==
                        colors.bgTertiary,
              ),
            );
            expect(boxes, findsNWidgets(6));
            expect(tester.getSize(row), Size(width, 64));
            expect(tester.getSize(boxes.first), const Size(52, 52));
            expect(
              tester.getTopLeft(boxes.first) - tester.getTopLeft(row),
              const Offset(8, 6),
            );
            expect(
              tester.getSize(boxes.at(1)),
              Size(width == 280 ? 78 : 176, 15),
            );
            expect(
              tester.getSize(boxes.at(2)),
              Size(width == 280 ? 78 : 124, 12),
            );
            expect(
              tester.getTopLeft(boxes.at(2)).dy -
                  tester.getBottomLeft(boxes.at(1)).dy,
              4,
            );
            expect(tester.getSize(boxes.at(3)), const Size(34, 12));
            expect(tester.getSize(boxes.at(4)), const Size(16, 16));
            expect(tester.getSize(boxes.at(5)), const Size(18, 18));
            expect(find.byIcon(Icons.music_note_outlined), findsOneWidget);
            final decoration =
                tester
                        .widget<DecoratedBox>(
                          find
                              .descendant(
                                of: row,
                                matching: find.byType(DecoratedBox),
                              )
                              .first,
                        )
                        .decoration
                    as BoxDecoration;
            expect(decoration.color, Colors.transparent);
            expect(
              decoration.border,
              isFirst
                  ? const Border()
                  : Border(top: BorderSide(color: colors.border)),
            );
            expect(tester.takeException(), isNull);
          }
        },
      );
    }
  }

  for (final isMobile in [false, true]) {
    for (final isDark in [false, true]) {
      testWidgets(
        'track list matches mobile table appearance: mobile=$isMobile dark=$isDark',
        (tester) async {
          final bridge = _RecommendationBridge();
          addTearDown(bridge.eventsController.close);
          final events = MashinCoreBridgeEvents(coreBridge: bridge);
          addTearDown(events.dispose);
          final tracks = List.generate(
            3,
            (index) => Track.fromJson({
              'item_id': 'appearance-$index',
              'provider': 'source',
              'name': 'Appearance $index',
              'duration': 245,
            }),
          );
          final theme = isDark ? AppTheme.dark() : AppTheme.light();
          Widget host(Widget child) => MaterialApp(
            theme: theme,
            home: Scaffold(
              body: Align(
                alignment: Alignment.topLeft,
                child: SizedBox(width: 380, child: child),
              ),
            ),
          );
          await tester.pumpWidget(
            host(
              CustomScrollView(
                slivers: [
                  MediaTableSectionMobile<Track>(
                    title: '',
                    items: tracks,
                    isLoading: false,
                    emptyText: 'Empty',
                    maxItemsPerPage: 3,
                    coreBridge: bridge,
                    coreEvents: events,
                  ),
                ],
              ),
            ),
          );
          await tester.pumpAndSettle();
          final referenceCover = tester.widget<MediaTableCover>(
            find.byType(MediaTableCover).first,
          );
          final referenceDecoration =
              tester
                      .widget<Ink>(
                        find
                            .ancestor(
                              of: find.byType(MediaTableCover).at(1),
                              matching: find.byType(Ink),
                            )
                            .first,
                      )
                      .decoration
                  as BoxDecoration;
          await tester.pumpWidget(
            host(
              isMobile
                  ? MediaListSectionMobile<Track>(
                      title: '',
                      items: tracks,
                      isLoading: false,
                      emptyText: 'Empty',
                      coreBridge: bridge,
                      coreEvents: events,
                    )
                  : MediaListSectionDesktop<Track>(
                      title: '',
                      items: tracks,
                      isLoading: false,
                      emptyText: 'Empty',
                      coreBridge: bridge,
                      coreEvents: events,
                    ),
            ),
          );
          await tester.pumpAndSettle();
          for (var index = 0; index < tracks.length; index++) {
            final coverFinder = find.byType(MediaTableCover).at(index);
            final cover = tester.widget<MediaTableCover>(coverFinder);
            expect(cover.size, referenceCover.size);
            expect(cover.radius, referenceCover.radius);
            final rowFinder = find
                .ancestor(of: coverFinder, matching: find.byType(Material))
                .first;
            final row = tester.widget<Material>(rowFinder);
            expect(row.color, referenceDecoration.color);
            expect(
              row.shape,
              index == 0 ? const Border() : referenceDecoration.border,
            );
            final rowBounds = tester.getRect(rowFinder);
            expect(rowBounds.height, 64);
            expect(rowBounds.top, index * 64);
            expect(
              tester.getTopLeft(coverFinder) - rowBounds.topLeft,
              const Offset(8, 6),
            );
          }
          expect(tester.takeException(), isNull);
        },
      );
    }
  }

  for (final outcome in ['success', 'failure', 'new-owner', 'ordinary']) {
    testWidgets('removal menu selection lifecycle: $outcome', (tester) async {
      final selection = SelectionController.instance;
      final owner = Object();
      final newerOwner = Object();
      var closeCount = 0;
      void activate(Object activeOwner) => selection.activate(
        owner: activeOwner,
        registration: SelectionSessionRegistration(
          allSelected: false,
          selectedCount: 2,
          onToggleAll: () {},
          onMenuTap: null,
          onClose: () {
            closeCount++;
            selection.deactivate(activeOwner);
          },
        ),
      );
      addTearDown(selection.onClose);
      activate(owner);
      final completion = Completer<void>();
      final action = ContextMenuAction(
        label: 'Remove',
        icon: Icons.delete_outline,
        clearsSelectionOnSuccess: outcome != 'ordinary',
        onSelected: () => completion.future,
      );
      final execution = action.execute();
      expect(selection.isVisible, isTrue);
      if (outcome == 'new-owner') activate(newerOwner);
      if (outcome == 'failure') {
        final assertion = expectLater(execution, throwsStateError);
        completion.completeError(StateError('Removal failed'));
        await assertion;
      } else {
        completion.complete();
        await execution;
      }
      expect(selection.isVisible, outcome != 'success');
      expect(
        closeCount,
        outcome == 'success' || outcome == 'new-owner' ? 1 : 0,
      );
    });
  }

  for (final control in ['list', 'row', 'table']) {
    for (final isCompact in [false, true]) {
      testWidgets(
        'removal menu clears section selection: $control compact=$isCompact',
        (tester) async {
          tester.view.physicalSize = Size(isCompact ? 800 : 1200, 800);
          tester.view.devicePixelRatio = 1;
          addTearDown(tester.view.resetPhysicalSize);
          addTearDown(tester.view.resetDevicePixelRatio);
          final bridge = _RecommendationBridge();
          addTearDown(bridge.eventsController.close);
          final events = MashinCoreBridgeEvents(coreBridge: bridge);
          addTearDown(events.dispose);
          final tracks = List.generate(
            4,
            (index) => Track.fromJson({
              'provider': 'source',
              'item_id': 'remove-$index',
              'name': 'Remove $index',
            }),
          );
          final items = ValueNotifier(tracks);
          addTearDown(items.dispose);
          final completion = Completer<void>();
          List<Track> selected = [];
          await tester.pumpWidget(
            MaterialApp(
              theme: AppTheme.light().copyWith(
                platform: TargetPlatform.windows,
              ),
              home: Scaffold(
                body: Builder(
                  builder: (context) {
                    void showMenu() => unawaited(
                      const AppContextMenuController().showContextMenu(
                        context: context,
                        globalPosition: const Offset(100, 100),
                        actions: [
                          ContextMenuAction(
                            label: 'Remove selected',
                            icon: Icons.delete_outline,
                            clearsSelectionOnSuccess: true,
                            onSelected: () async {
                              await completion.future;
                              items.value = tracks.skip(2).toList();
                            },
                          ),
                        ],
                      ),
                    );
                    return ValueListenableBuilder(
                      valueListenable: items,
                      builder: (_, currentItems, _) => SingleChildScrollView(
                        child: switch (control) {
                          'list' => MediaListSectionDesktop<Track>(
                            title: 'Tracks',
                            items: currentItems,
                            isLoading: false,
                            emptyText: 'Empty',
                            onSelectionChanged: (value) => selected = value,
                            onSelectionMenuTap: showMenu,
                          ),
                          'row' => MediaRowSectionDesktop<Track>(
                            title: 'Tracks',
                            items: currentItems,
                            coreBridge: bridge,
                            isLoading: false,
                            emptyText: 'Empty',
                            onSelectionChanged: (value) => selected = value,
                            onSelectionMenuTap: showMenu,
                          ),
                          _ => MediaTableSectionDesktop<Track>(
                            title: 'Tracks',
                            items: currentItems,
                            coreBridge: bridge,
                            coreEvents: events,
                            isLoading: false,
                            emptyText: 'Empty',
                            onSelectionChanged: (value) => selected = value,
                            onSelectionMenuTap: showMenu,
                          ),
                        },
                      ),
                    );
                  },
                ),
              ),
            ),
          );
          await tester.pumpAndSettle();
          await tester.tap(
            find.text('Remove 0'),
            kind: PointerDeviceKind.mouse,
          );
          await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
          await tester.tap(
            find.text('Remove 1'),
            kind: PointerDeviceKind.mouse,
          );
          await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
          await tester.pumpAndSettle();
          expect(selected, tracks.take(2).toList());
          SelectionController.instance.onMenuTap();
          await tester.pumpAndSettle();
          await tester.tap(find.text('Remove selected'));
          await tester.pumpAndSettle();
          expect(selected, tracks.take(2).toList());
          expect(SelectionController.instance.isVisible, isTrue);
          completion.complete();
          await tester.pumpAndSettle();
          expect(selected, isEmpty);
          expect(SelectionController.instance.isVisible, isFalse);
          expect(SelectionController.instance.selectedCount, 0);
          expect(find.byIcon(Icons.check), findsNothing);
          await tester.tap(
            find.text('Remove 2'),
            kind: PointerDeviceKind.mouse,
          );
          await tester.pumpAndSettle();
          expect(selected, [tracks[2]]);
          await tester.pumpWidget(const SizedBox.shrink());
          expect(SelectionController.instance.isVisible, isFalse);
          expect(tester.takeException(), isNull);
        },
      );
    }
  }

  testWidgets('desktop item action excludes only primary action pointers', (
    tester,
  ) async {
    final handled = <bool>[];
    await tester.pumpWidget(
      Directionality(
        textDirection: TextDirection.ltr,
        child: Center(
          child: Listener(
            onPointerDown: (event) =>
                handled.add(DesktopItemAction.handlesPointer(event)),
            child: const DesktopItemAction(
              child: SizedBox(
                width: 100,
                height: 100,
                child: ColoredBox(color: Colors.red),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.byType(ColoredBox), kind: PointerDeviceKind.mouse);
    await tester.tap(
      find.byType(ColoredBox),
      kind: PointerDeviceKind.mouse,
      buttons: kSecondaryMouseButton,
    );
    expect(handled, [true, false]);
  });

  const moodIcons = <String, IconData>{
    'relax': Symbols.self_improvement,
    'focus': Symbols.mindfulness,
    'energy': Symbols.power,
    'melancholy': Symbols.sentiment_sad,
    'love': Symbols.favorite,
    'party': Symbols.celebration,
    'happy': Symbols.sentiment_excited,
    'nostalgic': Symbols.local_cafe,
    'angry': Symbols.sentiment_extremely_dissatisfied,
    'epic': Symbols.mode_heat,
  };
  for (final isDark in [false, true]) {
    testWidgets('mood cover glyphs render from bundled font, dark=$isDark', (
      tester,
    ) async {
      final fontLoader =
          FontLoader(
            'packages/material_symbols_icons/MaterialSymbolsOutlined',
          )..addFont(
            rootBundle.load(
              'packages/material_symbols_icons/lib/fonts/MaterialSymbolsOutlined.ttf',
            ),
          );
      await fontLoader.load();
      final boundaryKey = GlobalKey();
      final moodKeys = moodIcons.keys.toList();
      await tester.pumpWidget(
        MaterialApp(
          theme: isDark ? AppTheme.dark() : AppTheme.light(),
          home: Scaffold(
            body: RepaintBoundary(
              key: boundaryKey,
              child: SizedBox(
                width: 600,
                height: 240,
                child: Column(
                  children: [
                    for (final start in [0, 5])
                      Row(
                        children: [
                          for (final moodKey in moodKeys.skip(start).take(5))
                            SizedBox.square(
                              dimension: 120,
                              child: buildMoodPlaylistCover(
                                Playlist.fromJson({
                                  'provider':
                                      'lastfm_recommendations_enhanced--test',
                                  'item_id': 'global_mood_playlist_$moodKey',
                                  'name': moodKey,
                                }),
                              ),
                            ),
                        ],
                      ),
                  ],
                ),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final boundary =
          boundaryKey.currentContext!.findRenderObject()!
              as RenderRepaintBoundary;
      final image = await tester.runAsync(() => boundary.toImage());
      final pixels = await tester.runAsync(
        () => image!.toByteData(format: ui.ImageByteFormat.rawRgba),
      );
      expect(pixels, isNotNull);
      final foregroundColors = find
          .byType(MoodPlaylistCover)
          .evaluate()
          .map(
            (element) => tester
                .widget<Icon>(
                  find.descendant(
                    of: find.byWidget(element.widget),
                    matching: find.byType(Icon),
                  ),
                )
                .color!,
          );
      var moodIndex = 0;
      for (final foreground in foregroundColors) {
        final originX = (moodIndex % 5) * 120;
        final originY = (moodIndex ~/ 5) * 120;
        final red = (foreground.r * 255).round();
        final green = (foreground.g * 255).round();
        final blue = (foreground.b * 255).round();
        var glyphPixels = 0;
        for (var vertical = originY; vertical < originY + 120; vertical++) {
          for (
            var horizontal = originX;
            horizontal < originX + 120;
            horizontal++
          ) {
            final offset = (vertical * image!.width + horizontal) * 4;
            if ((pixels!.getUint8(offset) - red).abs() <= 2 &&
                (pixels.getUint8(offset + 1) - green).abs() <= 2 &&
                (pixels.getUint8(offset + 2) - blue).abs() <= 2)
              glyphPixels++;
          }
        }
        expect(
          glyphPixels,
          greaterThan(20),
          reason: 'Blank glyph: ${moodKeys[moodIndex]}',
        );
        moodIndex++;
      }
      image!.dispose();
      expect(moodIndex, 10);
      expect(tester.takeException(), isNull);
    });
  }
  for (final mood in moodIcons.entries) {
    for (final isDark in [false, true]) {
      testWidgets(
        'mood cover maps ${mood.key} with theme contrast, dark=$isDark',
        (tester) async {
          final playlist = Playlist.fromJson({
            'provider': 'lastfm_recommendations_enhanced--test',
            'item_id': 'global_mood_playlist_${mood.key}',
            'name': 'Translated mood name',
          });
          final cover = buildMoodPlaylistCover(playlist);
          expect(cover, isNotNull);
          expect(cover!.icon, mood.value);
          await tester.pumpWidget(
            MaterialApp(
              theme: isDark ? AppTheme.dark() : AppTheme.light(),
              home: Scaffold(
                body: Center(
                  child: SizedBox.square(dimension: 146, child: cover),
                ),
              ),
            ),
          );
          final icon = tester.widget<Icon>(find.byIcon(mood.value));
          final background = tester.widget<ColoredBox>(
            find.descendant(
              of: find.byType(MoodPlaylistCover),
              matching: find.byType(ColoredBox),
            ),
          );
          final iconLuminance = icon.color!.computeLuminance();
          final backgroundLuminance = background.color.computeLuminance();
          final contrast =
              (isDark ? iconLuminance + 0.05 : backgroundLuminance + 0.05) /
              (isDark ? backgroundLuminance + 0.05 : iconLuminance + 0.05);
          expect(contrast, greaterThanOrEqualTo(3));
          expect(
            tester.getSize(find.byType(MoodPlaylistCover)),
            const Size(146, 146),
          );
          expect(tester.takeException(), isNull);
        },
      );
    }
  }
  test(
    'mood cover leaves unrelated and unknown playlist artwork unchanged',
    () {
      for (final itemId in [
        'global_genre_playlist_relax',
        'global_mood_playlist_unknown',
      ]) {
        expect(
          buildMoodPlaylistCover(
            Playlist.fromJson({
              'provider': 'source',
              'item_id': itemId,
              'name': 'Relax',
            }),
          ),
          isNull,
        );
      }
    },
  );

  for (final isMobile in [false, true]) {
    testWidgets(
      'mood row replaces existing cover and retains actions, mobile=$isMobile',
      (tester) async {
        final bridge = _RecommendationBridge();
        final events = MashinCoreBridgeEvents(coreBridge: bridge);
        addTearDown(events.dispose);
        addTearDown(bridge.eventsController.close);
        final playlist = Playlist.fromJson({
          'provider': 'lastfm_recommendations_enhanced--test',
          'item_id': 'global_mood_playlist_relax',
          'name': 'Relax mix',
          'metadata': {
            'images': [
              {
                'type': 'thumb',
                'path':
                    'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jCuoAAAAASUVORK5CYII=',
                'provider': 'url',
              },
            ],
          },
        });
        expect(playlist.imageUriForSize(256), isNotNull);
        Playlist? openedPlaylist;
        Playlist? menuPlaylist;
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.light().copyWith(
              platform: isMobile
                  ? TargetPlatform.android
                  : TargetPlatform.windows,
            ),
            home: Scaffold(
              body: isMobile
                  ? MediaRowSectionMobile<Playlist>(
                      title: 'Moods',
                      isLoading: false,
                      items: [playlist],
                      emptyText: 'Empty',
                      coreBridge: bridge,
                      coreEvents: events,
                      showPlaylistDetails: false,
                      playlistCoverBuilder: buildMoodPlaylistCover,
                      onItemTap: (item) => openedPlaylist = item,
                    )
                  : MediaRowSectionDesktop<Playlist>(
                      title: 'Moods',
                      isLoading: false,
                      items: [playlist],
                      emptyText: 'Empty',
                      coreBridge: bridge,
                      coreEvents: events,
                      showPlaylistDetails: false,
                      playlistCoverBuilder: buildMoodPlaylistCover,
                      onPlaylistTap: (item) => openedPlaylist = item,
                      onRowMoreAnchorTap: (item, index, anchor) =>
                          menuPlaylist = item,
                    ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(find.byType(MoodPlaylistCover), findsOneWidget);
        expect(find.byType(MediaImage), findsNothing);
        if (!isMobile) {
          final mouse = await tester.createGesture(
            kind: PointerDeviceKind.mouse,
          );
          await mouse.addPointer();
          await mouse.moveTo(tester.getCenter(find.byType(MoodPlaylistCover)));
          await tester.pumpAndSettle();
          await tester.tap(find.byIcon(Icons.more_vert_outlined).first);
          expect(menuPlaylist, same(playlist));
          addTearDown(mouse.removePointer);
        }
        await tester.tap(find.byIcon(Icons.play_arrow_sharp).first);
        await tester.pumpAndSettle();
        expect(bridge.playedItems, [playlist]);
        await tester.tap(find.text('Relax mix'));
        expect(openedPlaylist, same(playlist));
        await tester.pumpWidget(const SizedBox());
        await tester.pumpAndSettle();
        expect(SelectionController.instance.isVisible, isFalse);
        expect(tester.takeException(), isNull);
      },
    );
  }

  testWidgets(
    'desktop list pages slide in navigation direction without changing height',
    (tester) async {
      tester.view.physicalSize = const Size(500, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final tracks = List.generate(
        6,
        (index) => Track.fromJson({
          'provider': 'source',
          'item_id': 'slide-$index',
          'name': 'Slide $index',
        }),
      );
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.light().copyWith(platform: TargetPlatform.windows),
          home: Scaffold(
            body: MediaListSectionDesktop<Track>(
              title: 'Tracks',
              items: tracks,
              isLoading: false,
              emptyText: 'Empty',
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final section = find.byType(MediaListSectionDesktop<Track>);
      final height = tester.getSize(section).height;
      Finder pageSlide(String title) => find
          .ancestor(
            of: find.text(title),
            matching: find.byType(SlideTransition),
          )
          .first;
      await tester.tap(find.byTooltip('Nächste Seite'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 80));
      final incoming = tester.widget<SlideTransition>(pageSlide('Slide 3'));
      final outgoing = tester.widget<SlideTransition>(pageSlide('Slide 0'));
      expect(incoming.position.value.dx, greaterThan(0));
      expect(outgoing.position.value.dx, lessThan(0));
      expect(tester.getSize(section).height, height);
      expect(
        tester
            .widget<IconButton>(
              find.byWidgetPredicate(
                (widget) =>
                    widget is IconButton && widget.tooltip == 'Vorherige Seite',
              ),
            )
            .onPressed,
        isNotNull,
      );
      expect(
        tester
            .widget<IconButton>(
              find.byWidgetPredicate(
                (widget) =>
                    widget is IconButton && widget.tooltip == 'Nächste Seite',
              ),
            )
            .onPressed,
        isNull,
      );
      await tester.pumpAndSettle();
      expect(find.text('Slide 0'), findsNothing);
      expect(
        tester.widget<SlideTransition>(pageSlide('Slide 3')).position.value,
        Offset.zero,
      );
      await tester.tap(find.byTooltip('Vorherige Seite'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 80));
      expect(
        tester.widget<SlideTransition>(pageSlide('Slide 0')).position.value.dx,
        lessThan(0),
      );
      expect(
        tester.widget<SlideTransition>(pageSlide('Slide 3')).position.value.dx,
        greaterThan(0),
      );
      await tester.pumpAndSettle();
      expect(find.text('Slide 3'), findsNothing);
      expect(tester.getSize(section).height, height);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'desktop list accepts repeated paging and direction changes during animation',
    (tester) async {
      tester.view.physicalSize = const Size(500, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final tracks = List.generate(
        12,
        (index) => Track.fromJson({
          'provider': 'source',
          'item_id': 'rapid-$index',
          'name': 'Rapid $index',
        }),
      );
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.light().copyWith(platform: TargetPlatform.windows),
          home: Scaffold(
            body: MediaListSectionDesktop<Track>(
              title: 'Tracks',
              items: tracks,
              isLoading: false,
              emptyText: 'Empty',
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      IconButton pagingButton(String tooltip) => tester.widget<IconButton>(
        find.byWidgetPredicate(
          (widget) => widget is IconButton && widget.tooltip == tooltip,
        ),
      );
      for (final tooltip in [
        'Nächste Seite',
        'Nächste Seite',
        'Vorherige Seite',
        'Nächste Seite',
        'Nächste Seite',
      ]) {
        expect(pagingButton(tooltip).onPressed, isNotNull);
        await tester.tap(find.byTooltip(tooltip));
        await tester.pump(const Duration(milliseconds: 40));
      }
      expect(pagingButton('Nächste Seite').onPressed, isNull);
      expect(pagingButton('Vorherige Seite').onPressed, isNotNull);
      await tester.tap(find.byTooltip('Vorherige Seite'));
      await tester.pump(const Duration(milliseconds: 40));
      expect(pagingButton('Nächste Seite').onPressed, isNotNull);
      await tester.pumpAndSettle();
      for (var index = 0; index < tracks.length; index++) {
        expect(
          find.text('Rapid $index'),
          index >= 6 && index < 9 ? findsOneWidget : findsNothing,
        );
      }
      for (var index = 0; index < 2; index++) {
        await tester.tap(find.byTooltip('Vorherige Seite'));
        await tester.pump(const Duration(milliseconds: 40));
      }
      expect(pagingButton('Vorherige Seite').onPressed, isNull);
      expect(pagingButton('Nächste Seite').onPressed, isNotNull);
      await tester.pumpAndSettle();
      expect(find.text('Rapid 0'), findsOneWidget);
      expect(find.text('Rapid 6'), findsNothing);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'desktop track title navigates without selecting even with control',
    (tester) async {
      final track = Track.fromJson({
        'provider': 'source',
        'item_id': 'track',
        'name': 'Track title',
        'album': {
          'provider': 'source',
          'item_id': 'album',
          'name': 'Album title',
        },
      });
      final withoutAlbum = Track.fromJson({
        'provider': 'source',
        'item_id': 'no-album',
        'name': 'No album',
      });
      final invalidAlbum = Track.fromJson({
        'provider': 'source',
        'item_id': 'invalid-album',
        'name': 'Invalid album',
        'album': {
          'provider': 'source',
          'item_id': '',
          'name': 'Invalid target',
        },
      });
      Album? openedAlbum;
      List<Track> selected = [];
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.light().copyWith(platform: TargetPlatform.windows),
          home: Scaffold(
            body: MediaListSectionDesktop<Track>(
              title: 'Tracks',
              items: [track, withoutAlbum, invalidAlbum],
              isLoading: false,
              emptyText: 'Empty',
              onAlbumTap: (album) => openedAlbum = album,
              onSelectionChanged: (items) => selected = items,
            ),
          ),
        ),
      );
      final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
      await mouse.addPointer(location: Offset.zero);
      await mouse.moveTo(tester.getCenter(find.text('Track title')));
      await tester.pumpAndSettle();
      final title = tester.widget<Text>(find.text('Track title'));
      expect(title.style?.decoration, TextDecoration.underline);
      expect(
        find.descendant(
          of: find.byType(MediaLinkText),
          matching: find.text('Album title'),
        ),
        findsNothing,
      );
      await tester.tap(find.text('Track title'));
      await tester.pumpAndSettle();
      expect(openedAlbum, same(track.album));
      expect(selected, isEmpty);
      openedAlbum = null;
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.tap(find.text('Track title'));
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await tester.pumpAndSettle();
      expect(openedAlbum, same(track.album));
      expect(selected, isEmpty);
      openedAlbum = null;
      await tester.pumpAndSettle();
      await tester.tap(find.text('No album'));
      await tester.pumpAndSettle();
      expect(openedAlbum, isNull);
      expect(selected, [withoutAlbum]);
      await tester.tap(find.text('Invalid album'));
      await tester.pumpAndSettle();
      expect(openedAlbum, isNull);
      expect(selected, [invalidAlbum]);
      expect(
        tester
            .widgetList<MediaLinkText>(find.byType(MediaLinkText))
            .where((link) => link.onItemTap != null),
        hasLength(1),
      );
      await mouse.removePointer();
      await tester.pumpWidget(const SizedBox());
      await tester.pumpAndSettle();
      expect(SelectionController.instance.isVisible, isFalse);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'desktop track list replaces menus with selection checkboxes and supports control and escape',
    (tester) async {
      final tracks = List.generate(
        3,
        (index) => Track.fromJson({
          'provider': 'source',
          'item_id': 'track-$index',
          'name': 'Track $index',
        }),
      );
      List<Track> selected = [];
      List<Track> contextSelection = [];
      var menuCount = 0;
      final bridge = _RecommendationBridge();
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.light().copyWith(platform: TargetPlatform.windows),
          home: Scaffold(
            body: MediaListSectionDesktop<Track>(
              title: 'Tracks',
              items: tracks,
              isLoading: false,
              emptyText: 'Empty',
              coreBridge: bridge,
              onItemMoreAnchorTap: (_, _, _) => menuCount++,
              onItemSelectionContextMenu: (items, _) =>
                  contextSelection = items,
              onSelectionChanged: (items) => selected = items,
            ),
          ),
        ),
      );
      expect(find.byIcon(Icons.more_vert_outlined), findsNWidgets(3));
      await tester.tap(find.byIcon(Icons.more_vert_outlined).first);
      await tester.pumpAndSettle();
      expect(menuCount, 1);
      expect(selected, [tracks[0]]);
      await tester.tap(find.byType(MediaTableCover).first);
      await tester.pumpAndSettle();
      expect(selected, [tracks[0]]);
      expect(bridge.playedItems, tracks);
      await tester.tap(find.text('Track 0'));
      await tester.pumpAndSettle();
      expect(selected, [tracks[0]]);
      expect(find.byIcon(Icons.more_vert_outlined), findsNothing);
      expect(find.byIcon(Icons.check), findsOneWidget);
      expect(SelectionController.instance.isVisible, isTrue);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.tap(find.text('Track 1'));
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await tester.pumpAndSettle();
      expect(selected, tracks.take(2).toList());
      expect(find.byIcon(Icons.check), findsNWidgets(2));
      final rightClick = await tester.startGesture(
        tester.getCenter(find.text('Track 0')),
        kind: PointerDeviceKind.mouse,
        buttons: kSecondaryMouseButton,
      );
      await rightClick.up();
      await tester.pumpAndSettle();
      expect(contextSelection, tracks.take(2).toList());
      expect(selected, tracks.take(2).toList());
      final checkedBox = find
          .ancestor(
            of: find.byIcon(Icons.check).first,
            matching: find.byType(GestureDetector),
          )
          .first;
      await tester.tap(checkedBox);
      await tester.pumpAndSettle();
      expect(selected, [tracks[1]]);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(selected, isEmpty);
      expect(find.byIcon(Icons.check), findsNothing);
      expect(find.byIcon(Icons.more_vert_outlined), findsNWidgets(3));
      expect(SelectionController.instance.isVisible, isFalse);
      await tester.tap(find.text('Track 2'));
      await tester.pumpAndSettle();
      await tester.tap(find.byIcon(Icons.check));
      await tester.pumpAndSettle();
      expect(selected, isEmpty);
      expect(find.byIcon(Icons.more_vert_outlined), findsNWidgets(3));
      await tester.tap(find.text('Track 0'));
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyA);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await tester.pumpAndSettle();
      expect(selected, tracks);
      SelectionController.instance.onClose();
      await tester.pumpAndSettle();
      expect(selected, isEmpty);
      expect(find.byIcon(Icons.more_vert_outlined), findsNWidgets(3));
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'desktop track selection follows identities through refresh and clears on removal and disposal',
    (tester) async {
      final tracks = List.generate(
        2,
        (index) => Track.fromJson({
          'provider': 'source',
          'item_id': 'refresh-$index',
          'name': 'Refresh $index',
        }),
      );
      final items = ValueNotifier<List<Track>>(tracks);
      addTearDown(items.dispose);
      List<Track> selected = [];
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.light().copyWith(platform: TargetPlatform.windows),
          home: Scaffold(
            body: ValueListenableBuilder<List<Track>>(
              valueListenable: items,
              builder: (_, currentItems, _) => MediaListSectionDesktop<Track>(
                title: 'Refresh',
                items: currentItems,
                isLoading: false,
                emptyText: 'Empty',
                onSelectionChanged: (items) => selected = items,
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Refresh 1'));
      await tester.pumpAndSettle();
      items.value = [tracks[1], tracks[0]];
      await tester.pumpAndSettle();
      expect(selected, [tracks[1]]);
      expect(SelectionController.instance.selectedCount, 1);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.tap(find.text('Refresh 1'));
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await tester.pumpAndSettle();
      expect(selected, isEmpty);
      expect(find.byIcon(Icons.more_vert_outlined), findsNWidgets(2));
      await tester.tap(find.text('Refresh 1'));
      await tester.pumpAndSettle();
      items.value = [tracks[0]];
      await tester.pumpAndSettle();
      expect(selected, isEmpty);
      expect(SelectionController.instance.isVisible, isFalse);
      expect(find.byIcon(Icons.more_vert_outlined), findsOneWidget);
      await tester.tap(find.text('Refresh 0'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Refresh 0'));
      await tester.pumpAndSettle();
      expect(selected, [tracks[0]]);
      await tester.tap(find.text('Refresh 0'));
      await tester.pumpAndSettle();
      expect(SelectionController.instance.isVisible, isTrue);
      await tester.pumpWidget(const SizedBox());
      await tester.pumpAndSettle();
      expect(SelectionController.instance.isVisible, isFalse);
      expect(tester.takeException(), isNull);
    },
  );

  for (final isRediscovery in [false, true]) {
    testWidgets(
      'home track list selection menu plays every selected track, rediscovery=$isRediscovery',
      (tester) async {
        final bridge = _RecommendationBridge();
        final events = MashinCoreBridgeEvents(coreBridge: bridge);
        addTearDown(events.dispose);
        addTearDown(bridge.eventsController.close);
        bridge.result.complete(
          RecommendationSnapshot.fromJson({
            'is_loading': false,
            'has_load_error': false,
            'folders': [
              _folder(
                isRediscovery
                    ? 'user_rediscovery_tracks'
                    : 'user_top_tracks_monthly',
                'track',
                ['top0', 'top1', 'top2'],
              ),
            ],
          }),
        );
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.light().copyWith(platform: TargetPlatform.windows),
            home: Scaffold(
              body: HomeScreenDesktop(coreBridge: bridge, coreEvents: events),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await tester.scrollUntilVisible(
          find.byWidgetPredicate(
            (widget) =>
                widget is MediaListSectionDesktop<Track> &&
                widget.title ==
                    (isRediscovery
                        ? 'Lange nicht gehört'
                        : 'Deine meistgehörten Tracks'),
          ),
          300,
          scrollable: find.byType(Scrollable).first,
        );
        await tester.ensureVisible(find.text('top0'));
        await tester.pumpAndSettle();
        await tester.tap(find.text('top0'));
        await tester.pumpAndSettle();
        await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
        await tester.tap(find.text('top1'));
        await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
        await tester.pumpAndSettle();
        expect(SelectionController.instance.selectedCount, 2);
        SelectionController.instance.onMenuTap();
        await tester.pumpAndSettle();
        await tester.tap(find.text('Abspielen'));
        await tester.pumpAndSettle();
        expect(bridge.playedItems.map((item) => item.itemId), ['top0', 'top1']);
        await tester.pumpWidget(const SizedBox());
        await tester.pumpAndSettle();
        expect(SelectionController.instance.isVisible, isFalse);
        expect(tester.takeException(), isNull);
      },
    );
  }

  for (final trigger in ['right-click', 'button', 'custom']) {
    for (final isNearEdge in [false, true]) {
      testWidgets(
        'desktop $trigger menu uses root-overlay coordinates, nearEdge=$isNearEdge',
        (tester) async {
          tester.view.physicalSize = const Size(1600, 900);
          tester.view.devicePixelRatio = 1;
          addTearDown(tester.view.resetPhysicalSize);
          addTearDown(tester.view.resetDevicePixelRatio);
          var selectedCount = 0;
          final actions = [
            ContextMenuAction(
              label: 'Test action',
              icon: Icons.play_arrow,
              onSelected: () => selectedCount++,
            ),
          ];
          await tester.pumpWidget(
            MaterialApp(
              theme: AppTheme.light(),
              home: Scaffold(
                body: Stack(
                  children: [
                    Positioned(
                      left: isNearEdge ? 550 : 120,
                      top: isNearEdge ? 280 : 80,
                      width: 1000,
                      height: 600,
                      child: Navigator(
                        onGenerateRoute: (_) => MaterialPageRoute<void>(
                          builder: (pageContext) => Scaffold(
                            body: Listener(
                              behavior: HitTestBehavior.opaque,
                              onPointerDown: (event) {
                                if (trigger == 'right-click' &&
                                    (event.buttons & kSecondaryMouseButton) !=
                                        0) {
                                  unawaited(
                                    const AppContextMenuController()
                                        .showContextMenu(
                                          context: pageContext,
                                          globalPosition: event.position,
                                          actions: actions,
                                        ),
                                  );
                                }
                              },
                              child: Stack(
                                fit: StackFit.expand,
                                children: [
                                  Positioned(
                                    left: isNearEdge ? 900 : 100,
                                    top: isNearEdge ? 550 : 150,
                                    child: Builder(
                                      builder: (anchorContext) => IconButton(
                                        icon: const Icon(
                                          Icons.more_vert_outlined,
                                        ),
                                        onPressed: () {
                                          if (trigger == 'custom') {
                                            unawaited(
                                              const AppContextMenuController()
                                                  .showCustomContextMenu(
                                                    context: pageContext,
                                                    anchorContext:
                                                        anchorContext,
                                                    width: 240,
                                                    child: const SizedBox(
                                                      height: 80,
                                                      child: Text(
                                                        'Test action',
                                                      ),
                                                    ),
                                                  ),
                                            );
                                          } else {
                                            unawaited(
                                              const AppContextMenuController()
                                                  .showContextMenu(
                                                    context: pageContext,
                                                    anchorContext:
                                                        anchorContext,
                                                    actions: actions,
                                                  ),
                                            );
                                          }
                                        },
                                      ),
                                    ),
                                  ),
                                ],
                              ),
                            ),
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          );
          await tester.pumpAndSettle();
          final pointerPosition = isNearEdge
              ? const Offset(1540, 860)
              : const Offset(400, 300);
          final anchorCenter = tester.getCenter(find.byType(IconButton));
          if (trigger == 'right-click') {
            final mouse = await tester.createGesture(
              kind: PointerDeviceKind.mouse,
              buttons: kSecondaryMouseButton,
            );
            await mouse.down(pointerPosition);
            await mouse.up();
            await mouse.removePointer();
          } else {
            await tester.tap(find.byType(IconButton));
          }
          await tester.pumpAndSettle();
          final menu = trigger == 'custom'
              ? find
                    .ancestor(
                      of: find.text('Test action'),
                      matching: find.byType(SizedBox),
                    )
                    .first
              : find
                    .ancestor(
                      of: find.text('Test action'),
                      matching: find.byType(Positioned),
                    )
                    .first;
          final expectedPosition = trigger == 'right-click'
              ? pointerPosition
              : anchorCenter + const Offset(4, 4);
          final menuRect = tester.getRect(menu);
          if (isNearEdge) {
            expect(menuRect.left, lessThan(expectedPosition.dx));
            expect(menuRect.top, lessThan(expectedPosition.dy));
            expect(menuRect.left, greaterThanOrEqualTo(8));
            expect(menuRect.top, greaterThanOrEqualTo(8));
            expect(menuRect.right, lessThanOrEqualTo(1592));
            expect(menuRect.bottom, lessThanOrEqualTo(892));
          } else {
            expect(menuRect.topLeft, expectedPosition);
          }
          if (trigger == 'custom') {
            await tester.tapAt(const Offset(10, 10));
          } else {
            await tester.tap(find.text('Test action'));
          }
          await tester.pumpAndSettle();
          expect(selectedCount, trigger == 'custom' ? 0 : 1);
          expect(tester.takeException(), isNull);
        },
      );
    }
  }

  for (final control in ['list', 'table', 'row']) {
    for (final usesAnchor in [false, true]) {
      testWidgets(
        'desktop $control separates actions from selection, anchor=$usesAnchor',
        (tester) async {
          final bridge = _RecommendationBridge();
          addTearDown(bridge.eventsController.close);
          final events = MashinCoreBridgeEvents(coreBridge: bridge);
          addTearDown(events.dispose);
          final track = Track.fromJson({
            'provider': 'source',
            'item_id': 'action-track',
            'name': 'Action track',
            'duration': 180,
            'album': {
              'provider': 'source',
              'item_id': 'action-album',
              'name': 'Action album',
            },
            'artists': [
              {
                'provider': 'source',
                'item_id': 'action-artist',
                'name': 'Action artist',
              },
            ],
          });
          List<Track> selected = [];
          var selectionChanges = 0;
          var menus = 0;
          var favorites = 0;
          var navigations = 0;
          void onSelection(List<Track> items) {
            selected = items;
            selectionChanges++;
          }

          void onMenu() {
            expect(selected, [track]);
            menus++;
          }

          final Widget section = switch (control) {
            'list' => MediaListSectionDesktop<Track>(
              title: 'Tracks',
              items: [track],
              isLoading: false,
              emptyText: 'Empty',
              coreBridge: bridge,
              onAlbumTap: (_) => navigations++,
              onSelectionChanged: onSelection,
              onItemMoreAnchorTap: usesAnchor ? (_, _, _) => onMenu() : null,
              onItemContextMenu: (_, _, _) => onMenu(),
            ),
            'table' => MediaTableSectionDesktop<Track>(
              title: 'Tracks',
              items: [track],
              isLoading: false,
              emptyText: 'Empty',
              coreBridge: bridge,
              coreEvents: events,
              onAlbumTap: (_) => navigations++,
              onArtistTap: (_) => navigations++,
              onSelectionChanged: onSelection,
              onRowFavoriteTap: (_, _) => favorites++,
              onRowMoreTap: usesAnchor ? null : (_, _) => onMenu(),
              onRowMoreAnchorTap: usesAnchor ? (_, _, _) => onMenu() : null,
              onRowContextMenu: (_, _, _) => onMenu(),
            ),
            _ => MediaRowSectionDesktop<Track>(
              title: 'Tracks',
              items: [track],
              isLoading: false,
              emptyText: 'Empty',
              coreBridge: bridge,
              onAlbumTap: (_) => navigations++,
              onArtistTap: (_) => navigations++,
              onSelectionChanged: onSelection,
              onRowFavoriteTap: (_, _) => favorites++,
              onRowMoreTap: usesAnchor ? null : (_, _) => onMenu(),
              onRowMoreAnchorTap: usesAnchor ? (_, _, _) => onMenu() : null,
              onRowContextMenu: (_, _, _) => onMenu(),
            ),
          };
          await tester.pumpWidget(
            MaterialApp(
              theme: AppTheme.light().copyWith(
                platform: TargetPlatform.windows,
              ),
              home: Scaffold(body: SingleChildScrollView(child: section)),
            ),
          );
          await tester.pumpAndSettle();
          final mouse = await tester.createGesture(
            kind: PointerDeviceKind.mouse,
          );
          addTearDown(mouse.removePointer);
          await mouse.addPointer(location: Offset.zero);
          Future<void> click(Finder target, {bool secondary = false}) async {
            final position = tester.getCenter(target);
            await mouse.moveTo(position);
            await tester.pumpAndSettle();
            await tester.tapAt(
              position,
              kind: PointerDeviceKind.mouse,
              buttons: secondary ? kSecondaryMouseButton : kPrimaryMouseButton,
            );
            await tester.pumpAndSettle();
          }

          Future<void> checkActions() async {
            final changesBefore = selectionChanges;
            await click(find.byIcon(Icons.play_arrow_sharp).last);
            expect(bridge.playedItems, [track]);
            if (control != 'list') {
              await click(find.byIcon(Icons.favorite_border_outlined).last);
              await click(find.text('Action artist'));
              expect(favorites, greaterThan(0));
            }
            await click(
              find.text(control == 'list' ? 'Action track' : 'Action album'),
            );
            expect(navigations, greaterThan(0));
            expect(selectionChanges, changesBefore);
          }

          await checkActions();
          expect(selected, isEmpty);
          await click(find.byIcon(Icons.more_vert_outlined).last);
          expect(menus, 1);
          expect(selected, [track]);
          await checkActions();
          expect(selected, [track]);
          await tester.sendKeyEvent(LogicalKeyboardKey.escape);
          await tester.pumpAndSettle();
          expect(selected, isEmpty);
          final background = find.text(switch (control) {
            'list' => '3:00',
            'table' => '1',
            _ => 'Action track',
          }).last;
          await click(background);
          expect(selected, [track]);
          await click(background);
          expect(selected, [track]);
          await tester.sendKeyEvent(LogicalKeyboardKey.escape);
          await tester.pumpAndSettle();
          final cover = find
              .ancestor(
                of: find.byIcon(Icons.play_arrow_sharp).last,
                matching: find.byType(ClipRRect),
              )
              .first;
          final coverRect = tester.getRect(cover);
          final coverPosition = Offset(coverRect.left + 4, coverRect.center.dy);
          await mouse.moveTo(coverPosition);
          await tester.pumpAndSettle();
          await tester.tapAt(coverPosition, kind: PointerDeviceKind.mouse);
          await tester.pumpAndSettle();
          expect(selected, [track]);
          await click(find.byIcon(Icons.check).last);
          expect(selected, isEmpty);
          await click(
            find.byIcon(Icons.play_arrow_sharp).last,
            secondary: true,
          );
          expect(selected, [track]);
          expect(menus, 2);
          await tester.pumpWidget(const SizedBox.shrink());
          expect(SelectionController.instance.isVisible, isFalse);
          expect(tester.takeException(), isNull);
        },
      );
    }
  }

  for (final control in ['list', 'table', 'row']) {
    for (final isDark in [false, true]) {
      testWidgets(
        'desktop $control overlays hover on selection, dark=$isDark',
        (tester) async {
          final theme = (isDark ? AppTheme.dark() : AppTheme.light()).copyWith(
            platform: TargetPlatform.windows,
          );
          final colors = theme.extension<MashinColors>()!;
          final bridge = _RecommendationBridge();
          addTearDown(bridge.eventsController.close);
          final events = MashinCoreBridgeEvents(coreBridge: bridge);
          addTearDown(events.dispose);
          final track = Track.fromJson({
            'provider': 'source',
            'item_id': 'hover-track',
            'name': 'Hover track',
          });
          List<Track> selected = [];
          final boundaryKey = GlobalKey();
          final Widget section = switch (control) {
            'list' => MediaListSectionDesktop<Track>(
              title: 'Tracks',
              items: [track],
              isLoading: false,
              emptyText: 'Empty',
              onSelectionChanged: (items) => selected = items,
            ),
            'table' => MediaTableSectionDesktop<Track>(
              title: 'Tracks',
              items: [track],
              isLoading: false,
              emptyText: 'Empty',
              coreBridge: bridge,
              coreEvents: events,
              onSelectionChanged: (items) => selected = items,
            ),
            _ => MediaRowSectionDesktop<Track>(
              title: 'Tracks',
              items: [track],
              isLoading: false,
              emptyText: 'Empty',
              coreBridge: bridge,
              onSelectionChanged: (items) => selected = items,
            ),
          };
          await tester.pumpWidget(
            MaterialApp(
              theme: theme,
              home: RepaintBoundary(
                key: boundaryKey,
                child: Scaffold(body: SingleChildScrollView(child: section)),
              ),
            ),
          );
          await tester.pumpAndSettle();
          final mouse = await tester.createGesture(
            kind: PointerDeviceKind.mouse,
          );
          addTearDown(mouse.removePointer);
          await mouse.addPointer(location: const Offset(790, 590));
          await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
          await mouse.moveTo(tester.getCenter(find.text('Hover track')));
          await mouse.down(tester.getCenter(find.text('Hover track')));
          await mouse.up();
          await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
          await mouse.moveTo(const Offset(790, 590));
          await tester.pumpAndSettle();
          expect(selected, [track]);

          final frame = find
              .ancestor(
                of: find.text('Hover track'),
                matching: find.byWidgetPredicate((widget) {
                  if (widget is Material)
                    return widget.color == colors.selection;
                  if (widget is! Container) return false;
                  final decoration = widget.decoration;
                  return decoration is BoxDecoration &&
                      decoration.color == colors.selection;
                }),
              )
              .first;
          final rect = tester.getRect(frame);
          final samplePosition = Offset(rect.left + 2, rect.center.dy);

          Future<Color> backgroundColor() async {
            final boundary =
                boundaryKey.currentContext!.findRenderObject()!
                    as RenderRepaintBoundary;
            final image = await tester.runAsync(() => boundary.toImage());
            final pixels = await tester.runAsync(
              () => image!.toByteData(format: ui.ImageByteFormat.rawRgba),
            );
            final position = boundary.globalToLocal(samplePosition);
            final offset =
                (position.dy.floor() * image!.width + position.dx.floor()) * 4;
            final color = Color.fromARGB(
              pixels!.getUint8(offset + 3),
              pixels.getUint8(offset),
              pixels.getUint8(offset + 1),
              pixels.getUint8(offset + 2),
            );
            image.dispose();
            return color;
          }

          Future<void> expectBackground(Color expected) async {
            final actual = await backgroundColor();
            expect(actual.a, closeTo(expected.a, 1 / 255));
            expect(actual.r, closeTo(expected.r, 1 / 255));
            expect(actual.g, closeTo(expected.g, 1 / 255));
            expect(actual.b, closeTo(expected.b, 1 / 255));
          }

          await expectBackground(colors.selection);
          await mouse.moveTo(samplePosition);
          await tester.pumpAndSettle();
          await expectBackground(
            Color.alphaBlend(theme.hoverColor, colors.selection),
          );
          expect(selected, [track]);
          await mouse.moveTo(const Offset(790, 590));
          await tester.pumpAndSettle();
          await expectBackground(colors.selection);
          expect(selected, [track]);
          await tester.pumpWidget(const SizedBox.shrink());
          expect(SelectionController.instance.isVisible, isFalse);
          expect(tester.takeException(), isNull);
        },
      );
    }
  }

  for (final control in ['table', 'row', 'slide']) {
    testWidgets(
      'desktop $control favorite and context buttons scale without background or resizing',
      (tester) async {
        final track = Track.fromJson({
          'provider': 'source',
          'item_id': 'track',
          'name': 'Track',
        });
        final bridge = _RecommendationBridge();
        addTearDown(bridge.eventsController.close);
        final events = MashinCoreBridgeEvents(coreBridge: bridge);
        addTearDown(events.dispose);
        var favoriteCount = 0;
        var menuCount = 0;
        final Widget section = switch (control) {
          'table' => MediaTableSectionDesktop<Track>(
            title: 'Tracks',
            items: [track],
            isLoading: false,
            emptyText: 'Empty',
            coreBridge: bridge,
            coreEvents: events,
            onRowFavoriteTap: (item, index) => favoriteCount++,
            onRowMoreAnchorTap: (item, index, anchor) => menuCount++,
          ),
          'row' => MediaRowSectionDesktop<Track>(
            title: 'Tracks',
            items: [track],
            isLoading: false,
            emptyText: 'Empty',
            coreBridge: bridge,
            coreEvents: events,
            onRowFavoriteTap: (item, index) => favoriteCount++,
            onRowMoreAnchorTap: (item, index, anchor) => menuCount++,
          ),
          _ => MediaSlideSectionDesktop<Track>(
            title: 'Tracks',
            items: [track],
            isLoading: false,
            emptyText: 'Empty',
            coreBridge: bridge,
            coreEvents: events,
            onRowFavoriteTap: (item, index) => favoriteCount++,
            onRowMoreAnchorTap: (item, index, anchor) => menuCount++,
          ),
        };
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.light().copyWith(platform: TargetPlatform.windows),
            home: Scaffold(
              body: SingleChildScrollView(
                child: SizedBox(width: 800, child: section),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
        addTearDown(mouse.removePointer);
        await mouse.addPointer(location: Offset.zero);
        for (final iconData in [
          Icons.favorite_border_outlined,
          Icons.more_vert_outlined,
        ]) {
          final icon = find.byIcon(iconData).last;
          final button = find
              .ancestor(of: icon, matching: find.byType(SizedBox))
              .first;
          final scale = find
              .ancestor(of: icon, matching: find.byType(AnimatedScale))
              .first;
          final initialSize = tester.getSize(button);
          await mouse.moveTo(tester.getCenter(icon));
          await tester.pumpAndSettle();
          expect(tester.widget<AnimatedScale>(scale).scale, 1.24);
          expect(tester.getSize(button), initialSize);
          expect(
            find.descendant(of: button, matching: find.byType(DecoratedBox)),
            findsNothing,
          );
          await tester.tap(icon);
          await mouse.moveTo(Offset.zero);
          await tester.pumpAndSettle();
          expect(tester.widget<AnimatedScale>(scale).scale, 1);
          expect(
            find.descendant(of: button, matching: find.byType(DecoratedBox)),
            findsNothing,
          );
        }
        expect(favoriteCount, 1);
        expect(menuCount, 1);
        expect(tester.takeException(), isNull);
      },
    );
  }

  testWidgets(
    'desktop track context button scales without background or resizing',
    (tester) async {
      final track = Track.fromJson({
        'provider': 'source',
        'item_id': 'track',
        'name': 'Track',
      });
      var menuCount = 0;
      Future<void> showList({required bool isEnabled}) async {
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.light().copyWith(platform: TargetPlatform.windows),
            home: Scaffold(
              body: SizedBox(
                width: 380,
                child: MediaListSectionDesktop<Track>(
                  title: '',
                  items: [track],
                  isLoading: false,
                  emptyText: 'Empty',
                  enableSelection: false,
                  onItemMoreAnchorTap: isEnabled
                      ? (item, index, anchor) => menuCount++
                      : null,
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
      }

      await showList(isEnabled: true);
      final button = find.byWidgetPredicate(
        (widget) =>
            widget is SectionHeaderIconButton &&
            widget.icon == Icons.more_vert_outlined,
      );
      final scale = find.descendant(
        of: button,
        matching: find.byType(AnimatedScale),
      );
      final initialSize = tester.getSize(button);
      final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
      addTearDown(mouse.removePointer);
      await mouse.addPointer(location: Offset.zero);
      await mouse.moveTo(tester.getCenter(button));
      await tester.pumpAndSettle();
      expect(tester.widget<AnimatedScale>(scale).scale, 1.24);
      expect(tester.getSize(button), initialSize);
      final background = find.descendant(
        of: button,
        matching: find.byWidgetPredicate(
          (widget) => widget is Container && widget.decoration is BoxDecoration,
        ),
      );
      expect(
        (tester.widget<Container>(background).decoration! as BoxDecoration)
            .color,
        Colors.transparent,
      );
      expect(
        tester
            .widget<InkWell>(
              find.descendant(of: button, matching: find.byType(InkWell)),
            )
            .hoverColor,
        Colors.transparent,
      );
      await tester.tap(button);
      expect(menuCount, 1);
      await mouse.moveTo(Offset.zero);
      await tester.pumpAndSettle();
      expect(tester.widget<AnimatedScale>(scale).scale, 1);
      await showList(isEnabled: false);
      await mouse.moveTo(tester.getCenter(button));
      await tester.pumpAndSettle();
      expect(tester.widget<AnimatedScale>(scale).scale, 1);
      expect(
        (tester.widget<Container>(background).decoration! as BoxDecoration)
            .color,
        Colors.transparent,
      );
      expect(tester.takeException(), isNull);
    },
  );

  for (final isTrackSection in [true, false]) {
    testWidgets(
      'desktop list expands all items downward and collapses to paging, tracks=$isTrackSection',
      (tester) async {
        tester.view.physicalSize = const Size(1500, 900);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final bridge = _RecommendationBridge();
        addTearDown(bridge.eventsController.close);
        final items = List<MediaItem>.generate(
          14,
          (index) => isTrackSection
              ? Track.fromJson({
                  'provider': 'source',
                  'item_id': 'item-$index',
                  'name': 'Item $index',
                })
              : Playlist.fromJson({
                  'provider': 'source',
                  'item_id': 'item-$index',
                  'name': 'Item $index',
                }),
        );
        Future<void> showList(List<MediaItem> visibleItems) async {
          final section = isTrackSection
              ? MediaListSectionDesktop<Track>(
                  title: 'Items',
                  items: visibleItems.cast<Track>(),
                  isLoading: false,
                  emptyText: 'Empty',
                  coreBridge: bridge,
                )
              : MediaListSectionDesktop<Playlist>(
                  title: 'Items',
                  items: visibleItems.cast<Playlist>(),
                  isLoading: false,
                  emptyText: 'Empty',
                  coreBridge: bridge,
                );
          await tester.pumpWidget(
            MaterialApp(
              theme: AppTheme.light(),
              home: Scaffold(
                body: SingleChildScrollView(
                  child: SizedBox(width: 800, child: section),
                ),
              ),
            ),
          );
          await tester.pumpAndSettle();
        }

        await showList(items);
        expect(find.text('Item 13'), findsNothing);
        await tester.tap(find.byTooltip('Nächste Seite'));
        await tester.pumpAndSettle();
        await tester.tap(find.byTooltip('Alle anzeigen'));
        await tester.pumpAndSettle();
        for (final item in items) {
          expect(find.text(item.name), findsOneWidget);
        }
        expect(find.byTooltip('Nächste Seite'), findsNothing);
        expect(find.byTooltip('Vorherige Seite'), findsNothing);
        expect(
          tester.getTopLeft(find.text('Item 13')).dy,
          greaterThan(tester.getTopLeft(find.text('Item 0')).dy),
        );
        if (isTrackSection) {
          final lastRow = find
              .ancestor(
                of: find.text('Item 13'),
                matching: find.byType(Material),
              )
              .first;
          final lastCover = find.descendant(
            of: lastRow,
            matching: find.byType(MediaTableCover),
          );
          tester.widget<MediaTableCover>(lastCover).onPlayTap!();
          await tester.pumpAndSettle();
          expect(bridge.playedItems, [items.last]);
        }
        await tester.ensureVisible(find.byTooltip('Weniger anzeigen'));
        await tester.tap(find.byTooltip('Weniger anzeigen'));
        await tester.pumpAndSettle();
        expect(find.text('Item 0'), findsOneWidget);
        expect(find.text('Item 13'), findsNothing);
        expect(find.byTooltip('Nächste Seite'), findsOneWidget);
        await tester.ensureVisible(find.byTooltip('Nächste Seite'));
        await tester.tap(find.byTooltip('Nächste Seite'));
        await tester.pump();
        await tester.tap(find.byTooltip('Alle anzeigen'));
        await tester.pumpAndSettle();
        expect(find.text('Item 13'), findsOneWidget);
        expect(find.byTooltip('Nächste Seite'), findsNothing);
        await showList(items.take(4).toList());
        expect(find.byTooltip('Weniger anzeigen'), findsNothing);
        expect(find.byTooltip('Alle anzeigen'), findsNothing);
        await showList(items);
        expect(find.text('Item 13'), findsNothing);
        expect(find.byTooltip('Alle anzeigen'), findsOneWidget);
        expect(tester.takeException(), isNull);
      },
    );
  }

  testWidgets(
    'desktop list expansion appends pages with staggered vertical reveals and cancels on collapse',
    (tester) async {
      tester.view.physicalSize = const Size(500, 1000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final tracks = List.generate(
        9,
        (index) => Track.fromJson({
          'provider': 'source',
          'item_id': 'reveal-$index',
          'name': 'Reveal $index',
        }),
      );
      final items = ValueNotifier<List<Track>>(tracks);
      addTearDown(items.dispose);
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.light().copyWith(platform: TargetPlatform.windows),
          home: Scaffold(
            body: SingleChildScrollView(
              child: ValueListenableBuilder<List<Track>>(
                valueListenable: items,
                builder: (_, visibleItems, _) => MediaListSectionDesktop<Track>(
                  title: 'Tracks',
                  items: visibleItems,
                  isLoading: false,
                  emptyText: 'Empty',
                ),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final section = find.byType(MediaListSectionDesktop<Track>);
      final content = find
          .descendant(of: section, matching: find.byType(AnimatedContainer))
          .first;
      final collapsedHeight = tester.getSize(content).height;
      await tester.tap(find.byTooltip('Alle anzeigen'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 100));
      expect(find.text('Reveal 3'), findsNothing);
      expect(tester.getSize(content).height, collapsedHeight);
      await tester.pump(const Duration(milliseconds: 140));
      expect(find.text('Reveal 3'), findsOneWidget);
      expect(find.text('Reveal 6'), findsNothing);
      await tester.pump(const Duration(milliseconds: 40));
      await tester.pump(const Duration(milliseconds: 20));
      Opacity revealOpacity(String title) => tester.widget<Opacity>(
        find
            .ancestor(
              of: find
                  .ancestor(
                    of: find.text(title),
                    matching: find.byType(Transform),
                  )
                  .first,
              matching: find.byType(Opacity),
            )
            .first,
      );
      expect(revealOpacity('Reveal 3').opacity, greaterThan(0));
      expect(revealOpacity('Reveal 3').opacity, lessThan(1));
      expect(revealOpacity('Reveal 4').opacity, 0);
      final translation = tester.widget<Transform>(
        find
            .ancestor(
              of: find.text('Reveal 3'),
              matching: find.byType(Transform),
            )
            .first,
      );
      expect(translation.transform.getTranslation().y, lessThan(0));
      await tester.pumpAndSettle();
      expect(find.text('Reveal 8'), findsOneWidget);
      expect(revealOpacity('Reveal 3').opacity, 1);
      expect(tester.getSize(content).height, greaterThan(collapsedHeight));
      items.value = [
        ...tracks,
        Track.fromJson({
          'provider': 'source',
          'item_id': 'reveal-9',
          'name': 'Reveal 9',
        }),
      ];
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 240));
      await tester.pumpAndSettle();
      expect(find.text('Reveal 9'), findsOneWidget);
      await tester.ensureVisible(find.byTooltip('Weniger anzeigen'));
      await tester.tap(find.byTooltip('Weniger anzeigen'));
      await tester.pumpAndSettle();
      expect(tester.getSize(content).height, collapsedHeight);
      await tester.ensureVisible(find.byTooltip('Alle anzeigen'));
      await tester.tap(find.byTooltip('Alle anzeigen'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 50));
      await tester.tap(find.byTooltip('Weniger anzeigen'));
      await tester.pumpAndSettle();
      expect(find.text('Reveal 3'), findsNothing);
      expect(tester.getSize(content).height, collapsedHeight);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('mobile track table uses the shared reference skeleton', (
    tester,
  ) async {
    final bridge = _RecommendationBridge();
    addTearDown(bridge.eventsController.close);
    final events = MashinCoreBridgeEvents(coreBridge: bridge);
    addTearDown(events.dispose);
    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.light(),
        home: Scaffold(
          body: CustomScrollView(
            slivers: [
              MediaTableSectionMobile<Track>(
                title: '',
                items: const [],
                isLoading: true,
                emptyText: 'Empty',
                maxItemsPerPage: 3,
                coreBridge: bridge,
                coreEvents: events,
              ),
            ],
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final skeletons = find.byType(MediaTrackRowSkeleton);
    expect(skeletons, findsAtLeastNWidgets(3));
    for (final skeleton in tester.widgetList<MediaTrackRowSkeleton>(
      skeletons,
    )) {
      expect(skeleton.durationColumnWidth, 34);
      expect(skeleton.providerColumnWidth, 20);
      expect(skeleton.menuColumnWidth, 40);
      expect(skeleton.menuGap, 10);
      expect(skeleton.trailingPadding, 8);
    }
    for (var index = 0; index < 3; index++) {
      expect(tester.getSize(skeletons.at(index)).height, 64);
    }
    expect(tester.takeException(), isNull);
  });

  for (final isMobile in [false, true]) {
    testWidgets(
      'list section type keeps skeleton and content heights consistent, mobile=$isMobile',
      (tester) async {
        final track = Track.fromJson({
          'provider': 'source',
          'item_id': 'track',
          'name': 'Track',
        });
        for (final isTrackSection in [false, true]) {
          for (final isLoading in [true, false]) {
            final Widget section;
            if (isMobile) {
              section = isTrackSection
                  ? MediaListSectionMobile<Track>(
                      title: '',
                      items: isLoading ? const [] : [track],
                      isLoading: isLoading,
                      emptyText: 'Empty',
                    )
                  : MediaListSectionMobile<MediaItem>(
                      title: '',
                      items: isLoading ? const [] : [track],
                      isLoading: isLoading,
                      emptyText: 'Empty',
                    );
            } else {
              section = isTrackSection
                  ? MediaListSectionDesktop<Track>(
                      title: '',
                      items: isLoading ? const [] : [track],
                      isLoading: isLoading,
                      emptyText: 'Empty',
                    )
                  : MediaListSectionDesktop<MediaItem>(
                      title: '',
                      items: isLoading ? const [] : [track],
                      isLoading: isLoading,
                      emptyText: 'Empty',
                    );
            }
            await tester.pumpWidget(
              MaterialApp(
                theme: AppTheme.light(),
                home: Scaffold(
                  body: SingleChildScrollView(
                    child: SizedBox(width: 380, child: section),
                  ),
                ),
              ),
            );
            await tester.pumpAndSettle();
            final expectedHeight = isMobile
                ? (isTrackSection ? 192.0 : 166.0)
                : (isTrackSection ? 224.0 : 200.0);
            expect(
              tester.getSize(find.byWidget(section)).height,
              expectedHeight,
            );
            expect(
              find.byType(MediaTableCover),
              !isLoading && isTrackSection ? findsOneWidget : findsNothing,
            );
            if (isLoading && isTrackSection) {
              final skeletons = find.byType(MediaTrackRowSkeleton);
              expect(skeletons, findsAtLeastNWidgets(3));
              for (final skeleton in tester.widgetList<MediaTrackRowSkeleton>(
                skeletons,
              )) {
                expect(skeleton.durationColumnWidth, isMobile ? 42 : 36);
                expect(skeleton.providerColumnWidth, isMobile ? 48 : 40);
                expect(skeleton.menuColumnWidth, isMobile ? 48 : 32);
                expect(skeleton.menuGap, 0);
                expect(skeleton.trailingPadding, 0);
              }
              final covers = find.byWidgetPredicate(
                (widget) =>
                    widget is Container &&
                    widget.constraints?.maxWidth == 52 &&
                    widget.constraints?.maxHeight == 52,
              );
              expect(covers, findsAtLeastNWidgets(3));
              for (var index = 0; index < 3; index++) {
                final cover = covers.at(index);
                final coverDecoration =
                    tester.widget<Container>(cover).decoration as BoxDecoration;
                expect(coverDecoration.borderRadius, BorderRadius.circular(8));
                final row = find
                    .ancestor(
                      of: cover,
                      matching: find.byWidgetPredicate(
                        (widget) =>
                            widget is DecoratedBox &&
                            widget.decoration is BoxDecoration &&
                            (widget.decoration as BoxDecoration).border != null,
                      ),
                    )
                    .first;
                final decoration =
                    tester.widget<DecoratedBox>(row).decoration
                        as BoxDecoration;
                expect(
                  find.descendant(
                    of: cover,
                    matching: find.byIcon(Icons.music_note_outlined),
                  ),
                  findsOneWidget,
                );
                for (final (height, radius) in [
                  (15.0, 6.0),
                  (12.0, 6.0),
                  (12.0, 4.0),
                  (16.0, 8.0),
                  (18.0, 9.0),
                ]) {
                  expect(
                    find.descendant(
                      of: row,
                      matching: find.byWidgetPredicate(
                        (widget) =>
                            widget is Container &&
                            widget.constraints?.maxHeight == height &&
                            widget.decoration is BoxDecoration &&
                            (widget.decoration as BoxDecoration).borderRadius ==
                                BorderRadius.circular(radius),
                      ),
                    ),
                    findsOneWidget,
                  );
                }
                expect(decoration.color, Colors.transparent);
                expect(decoration.borderRadius, isNull);
                expect(
                  decoration.border,
                  index == 0
                      ? const Border()
                      : Border(
                          top: BorderSide(
                            color: tester.element(row).mashinColors.border,
                          ),
                        ),
                );
                final bounds = tester.getRect(row);
                expect(bounds.height, 64);
                expect(bounds.top, index * 64);
                expect(
                  tester.getTopLeft(cover) - bounds.topLeft,
                  const Offset(8, 6),
                );
              }
            }
            expect(tester.takeException(), isNull);
          }
        }
      },
    );
  }

  for (final isMobile in [false, true]) {
    testWidgets(
      'generic list consistently uses compact fallback rows, mobile=$isMobile',
      (tester) async {
        final items = <MediaItem>[
          Track.fromJson({
            'provider': 'source',
            'item_id': 'track',
            'name': 'Track',
          }),
          Album.fromJson({
            'provider': 'source',
            'item_id': 'album',
            'name': 'Album',
          }),
          Artist.fromJson({
            'provider': 'source',
            'item_id': 'artist',
            'name': 'Artist',
          }),
          Playlist.fromJson({
            'provider': 'source',
            'item_id': 'playlist',
            'name': 'Playlist',
          }),
        ];
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.light(),
            home: Scaffold(
              body: SizedBox(
                width: 700,
                child: isMobile
                    ? MediaListSectionMobile<MediaItem>(
                        title: '',
                        items: items,
                        isLoading: false,
                        emptyText: 'Empty',
                      )
                    : MediaListSectionDesktop<MediaItem>(
                        title: '',
                        items: items,
                        isLoading: false,
                        emptyText: 'Empty',
                      ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        for (final item in items) {
          if (isMobile && find.text(item.name).evaluate().isEmpty) {
            await tester.drag(find.byType(PageView), const Offset(-650, 0));
            await tester.pumpAndSettle();
          }
          expect(find.text(item.name), findsOneWidget);
        }
        expect(find.byType(MediaTableCover), findsNothing);
        expect(tester.takeException(), isNull);
      },
    );
  }

  testWidgets(
    'desktop list pages complete three-row blocks and adapts to width and refresh',
    (tester) async {
      tester.view.physicalSize = const Size(1500, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final tracks = List.generate(
        14,
        (index) => Track.fromJson({
          'provider': 'source',
          'item_id': 'track-$index',
          'name': 'Track $index',
          'duration': 180,
        }),
      );
      int? menuIndex;
      final bridge = _RecommendationBridge();
      addTearDown(bridge.eventsController.close);
      Future<void> showList(double width, List<Track> items) async {
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.light(),
            home: Scaffold(
              body: Align(
                alignment: Alignment.topLeft,
                child: SizedBox(
                  width: width,
                  child: MediaListSectionDesktop<Track>(
                    title: 'Tracks',
                    items: items,
                    isLoading: false,
                    emptyText: 'Empty',
                    coreBridge: bridge,
                    onItemMoreAnchorTap: (item, index, anchor) =>
                        menuIndex = index,
                  ),
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
      }

      await showList(800, tracks);
      expect(find.byType(MediaTableCover), findsNWidgets(6));
      final playButton = find.byWidgetPredicate(
        (widget) =>
            widget is SectionHeaderIconButton &&
            widget.icon == Icons.play_arrow,
      );
      final shuffleButton = find.byWidgetPredicate(
        (widget) =>
            widget is SectionHeaderIconButton && widget.icon == Icons.shuffle,
      );
      final previousButton = find.byWidgetPredicate(
        (widget) => widget is IconButton && widget.tooltip == 'Vorherige Seite',
      );
      final nextButton = find.byWidgetPredicate(
        (widget) => widget is IconButton && widget.tooltip == 'Nächste Seite',
      );
      final headerPositions = [
        playButton,
        shuffleButton,
        previousButton,
        nextButton,
      ].map(tester.getCenter).toList();
      for (var index = 1; index < headerPositions.length; index++) {
        expect(headerPositions[index].dy, headerPositions.first.dy);
        expect(
          headerPositions[index].dx,
          greaterThan(headerPositions[index - 1].dx),
        );
      }
      expect(find.byType(SectionHeaderVerticalDivider), findsOneWidget);
      final firstPosition = tester.getTopLeft(find.text('Track 0'));
      final nextColumnPosition = tester.getTopLeft(find.text('Track 3'));
      expect(nextColumnPosition.dx, greaterThan(firstPosition.dx));
      expect(nextColumnPosition.dy, firstPosition.dy);
      final sectionHeight = tester
          .getSize(find.byType(MediaListSectionDesktop<Track>))
          .height;
      await tester.tap(find.byTooltip('Nächste Seite'));
      await tester.pumpAndSettle();
      expect(find.text('Track 0'), findsNothing);
      expect(find.text('Track 6'), findsOneWidget);
      final seventh = find
          .ancestor(of: find.text('Track 7'), matching: find.byType(Material))
          .first;
      await tester.tap(
        find.descendant(
          of: seventh,
          matching: find.byTooltip('Weitere Optionen'),
        ),
      );
      expect(menuIndex, 7);
      await tester.tap(find.byTooltip('Nächste Seite'));
      await tester.pumpAndSettle();
      expect(find.byType(MediaTableCover), findsNWidgets(2));
      expect(find.text('Track 13'), findsOneWidget);
      expect(
        tester.getSize(find.byType(MediaListSectionDesktop<Track>)).height,
        sectionHeight,
      );
      expect(
        tester
            .widget<IconButton>(
              find.byWidgetPredicate(
                (widget) =>
                    widget is IconButton && widget.tooltip == 'Nächste Seite',
              ),
            )
            .onPressed,
        isNull,
      );
      await tester.tap(find.byTooltip('Vorherige Seite'));
      await tester.pumpAndSettle();
      expect(find.text('Track 6'), findsOneWidget);
      await showList(855, tracks);
      expect(find.byType(MediaTableCover), findsNWidgets(6));
      await showList(856, tracks);
      expect(find.byType(MediaTableCover), findsNWidgets(9));
      await showList(1200, tracks);
      expect(find.byType(MediaTableCover), findsNWidgets(12));
      await showList(800, tracks.take(4).toList());
      expect(find.byType(MediaTableCover), findsNWidgets(4));
      expect(find.text('Track 0'), findsOneWidget);
      expect(find.byTooltip('Nächste Seite'), findsNothing);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'desktop playlist list no longer truncates results after nine items',
    (tester) async {
      final playlists = List.generate(
        14,
        (index) => Playlist.fromJson({
          'provider': 'source',
          'item_id': 'playlist-$index',
          'name': 'Playlist $index',
        }),
      );
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.light(),
          home: Scaffold(
            body: SizedBox(
              width: 700,
              child: MediaListSectionDesktop<Playlist>(
                title: 'Playlists',
                items: playlists,
                isLoading: false,
                emptyText: 'Empty',
              ),
            ),
          ),
        ),
      );
      expect(find.text('Playlist 8'), findsOneWidget);
      expect(find.text('Playlist 9'), findsNothing);
      await tester.tap(find.byTooltip('Nächste Seite'));
      await tester.pumpAndSettle();
      expect(find.text('Playlist 13'), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );

  for (final isMobile in [false, true]) {
    for (final isGeneric in [false, true]) {
      testWidgets(
        'playlist lists never select and retain navigation, mobile=$isMobile generic=$isGeneric',
        (tester) async {
          final playlists = List.generate(
            3,
            (index) => Playlist.fromJson({
              'provider': 'source',
              'item_id': 'playlist-$index',
              'name': 'Playlist $index',
            }),
          );
          MediaItem? openedItem;
          var nonemptySelections = 0;
          Widget buildList<T extends MediaItem>(List<T> items) => isMobile
              ? MediaListSectionMobile<T>(
                  title: 'Playlists',
                  items: items,
                  isLoading: false,
                  emptyText: 'Empty',
                  onItemTap: (item) => openedItem = item,
                  onSelectionMenuTap: (_) => nonemptySelections++,
                )
              : MediaListSectionDesktop<T>(
                  title: 'Playlists',
                  items: items,
                  isLoading: false,
                  emptyText: 'Empty',
                  onItemTap: (item) => openedItem = item,
                  onSelectionChanged: (items) {
                    if (items.isNotEmpty) nonemptySelections++;
                  },
                );
          await tester.pumpWidget(
            MaterialApp(
              theme: AppTheme.light().copyWith(
                platform: isMobile
                    ? TargetPlatform.android
                    : TargetPlatform.windows,
              ),
              home: Scaffold(
                body: isGeneric
                    ? buildList<MediaItem>(playlists)
                    : buildList<Playlist>(playlists),
              ),
            ),
          );
          await tester.pumpAndSettle();
          await tester.tap(find.text('Playlist 0'));
          expect(openedItem, same(playlists.first));
          await tester.longPress(find.text('Playlist 1'));
          await tester.pumpAndSettle();
          await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
          await tester.tap(find.text('Playlist 2'));
          await tester.sendKeyDownEvent(LogicalKeyboardKey.keyA);
          await tester.sendKeyUpEvent(LogicalKeyboardKey.keyA);
          await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
          await tester.pumpAndSettle();
          expect(SelectionController.instance.isVisible, isFalse);
          expect(nonemptySelections, 0);
          expect(find.byType(MobileSelectableSectionItem), findsNothing);
          await tester.pumpWidget(const SizedBox());
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
        },
      );
    }
  }

  for (final isTable in [true, false]) {
    for (final isDark in [false, true]) {
      testWidgets(
        'mobile track selection animates ink feedback: table=$isTable dark=$isDark',
        (tester) async {
          tester.view.physicalSize = const Size(390, 844);
          tester.view.devicePixelRatio = 1;
          addTearDown(tester.view.resetPhysicalSize);
          addTearDown(tester.view.resetDevicePixelRatio);
          final bridge = _RecommendationBridge();
          addTearDown(bridge.eventsController.close);
          final events = MashinCoreBridgeEvents(coreBridge: bridge);
          addTearDown(events.dispose);
          final track = Track.fromJson({
            'provider': 'source',
            'item_id': 'selection-animation',
            'name': 'Animated track',
          });
          final boundaryKey = GlobalKey();
          await tester.pumpWidget(
            MaterialApp(
              theme: isDark ? AppTheme.dark() : AppTheme.light(),
              home: RepaintBoundary(
                key: boundaryKey,
                child: Scaffold(
                  body: isTable
                      ? CustomScrollView(
                          slivers: [
                            MediaTableSectionMobile<Track>(
                              title: '',
                              items: [track],
                              isLoading: false,
                              emptyText: 'Empty',
                              maxItemsPerPage: 3,
                              coreBridge: bridge,
                              coreEvents: events,
                            ),
                          ],
                        )
                      : MediaListSectionMobile<Track>(
                          title: '',
                          items: [track],
                          isLoading: false,
                          emptyText: 'Empty',
                          coreBridge: bridge,
                          coreEvents: events,
                        ),
                ),
              ),
            ),
          );
          await tester.pumpAndSettle();
          final title = find.text('Animated track');
          final row = find
              .ancestor(of: title, matching: find.byType(InkWell))
              .first;
          final inkWell = tester.widget<InkWell>(row);
          final colors = tester.element(row).mashinColors;
          expect(inkWell.onLongPress, isNotNull);
          expect(inkWell.splashColor, colors.selection.withValues(alpha: 0.42));
          expect(
            inkWell.highlightColor,
            colors.selection.withValues(alpha: 0.18),
          );
          final bounds = tester.getRect(row);
          final samplePosition = bounds.topLeft + const Offset(74, 56);
          Future<Color> backgroundColor() async {
            final boundary =
                boundaryKey.currentContext!.findRenderObject()!
                    as RenderRepaintBoundary;
            final image = await tester.runAsync(() => boundary.toImage());
            final pixels = await tester.runAsync(
              () => image!.toByteData(format: ui.ImageByteFormat.rawRgba),
            );
            final position = boundary.globalToLocal(samplePosition);
            final offset =
                (position.dy.floor() * image!.width + position.dx.floor()) * 4;
            final color = Color.fromARGB(
              pixels!.getUint8(offset + 3),
              pixels.getUint8(offset),
              pixels.getUint8(offset + 1),
              pixels.getUint8(offset + 2),
            );
            image.dispose();
            return color;
          }

          final initialColor = await backgroundColor();
          final gesture = await tester.startGesture(tester.getCenter(title));
          await tester.pump();
          await tester.pump(const Duration(milliseconds: 100));
          await tester.pump(const Duration(milliseconds: 200));
          expect(SelectionController.instance.isVisible, isFalse);
          final pressedColor = await backgroundColor();
          expect(pressedColor, isNot(initialColor));
          await tester.pump(const Duration(milliseconds: 400));
          await gesture.up();
          await tester.pumpAndSettle();
          expect(SelectionController.instance.selectedCount, 1);
          expect(tester.getRect(row), bounds);
          final selectedColor = await backgroundColor();
          expect(selectedColor, isNot(initialColor));
          expect(selectedColor, isNot(pressedColor));
          expect(bridge.playedItems, isEmpty);
          SelectionController.instance.onClose();
          await tester.pumpAndSettle();
          expect(await backgroundColor(), initialColor);
          expect(SelectionController.instance.isVisible, isFalse);
          await tester.pumpWidget(const SizedBox());
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
        },
      );
    }
  }

  testWidgets(
    'mobile table retains centered checkbox selection and menu restoration',
    (tester) async {
      tester.view.physicalSize = const Size(390, 844);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final bridge = _RecommendationBridge();
      final events = MashinCoreBridgeEvents(coreBridge: bridge);
      addTearDown(events.dispose);
      addTearDown(bridge.eventsController.close);
      final tracks = List.generate(
        2,
        (index) => Track.fromJson({
          'provider': 'source',
          'item_id': 'track-$index',
          'name': 'Track $index',
        }),
      );
      List<Track>? selectedItems;
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.light(),
          home: Scaffold(
            body: CustomScrollView(
              slivers: [
                MediaTableSectionMobile<Track>(
                  title: 'Tracks',
                  items: tracks,
                  isLoading: false,
                  emptyText: 'Empty',
                  coreBridge: bridge,
                  coreEvents: events,
                  onSelectionMenuTap: (items) => selectedItems = items,
                ),
              ],
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final menuIcons = find.byIcon(Icons.more_vert_outlined);
      expect(menuIcons, findsNWidgets(2));
      final centers = List.generate(
        2,
        (index) => tester.getCenter(menuIcons.at(index)),
      );
      await tester.longPress(find.text('Track 0'));
      await tester.pumpAndSettle();
      expect(menuIcons, findsNothing);
      expect(find.byType(MobileSelectionCheckBox), findsNWidgets(2));
      for (var index = 0; index < 2; index++) {
        expect(
          tester.getCenter(find.byType(MobileSelectionCheckBox).at(index)),
          centers[index],
        );
      }
      await tester.tapAt(
        tester.getCenter(find.byType(MobileSelectionCheckBox).at(1)),
      );
      await tester.pumpAndSettle();
      expect(SelectionController.instance.selectedCount, 2);
      SelectionController.instance.onMenuTap();
      expect(selectedItems, tracks);
      SelectionController.instance.onClose();
      await tester.pumpAndSettle();
      expect(find.byType(MobileSelectionCheckBox), findsNothing);
      expect(menuIcons, findsNWidgets(2));
      await tester.pumpWidget(const SizedBox());
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'mobile track list uses swipeable three-row blocks with playback and menus',
    (tester) async {
      tester.view.physicalSize = const Size(390, 844);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final tracks = List.generate(
        7,
        (index) => Track.fromJson({
          'provider': 'source',
          'item_id': 'track-$index',
          'name': 'Track $index',
          'duration': 245,
          'artists': [
            {'provider': 'source', 'item_id': 'artist', 'name': 'Artist'},
          ],
        }),
      );
      final bridge = _RecommendationBridge();
      addTearDown(bridge.eventsController.close);
      List<Track>? menuItems;
      Track? navigatedTrack;
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.light(),
          home: Scaffold(
            body: MediaListSectionMobile<Track>(
              title: 'Tracks',
              items: tracks,
              isLoading: false,
              emptyText: 'Empty',
              coreBridge: bridge,
              onItemTap: (track) => navigatedTrack = track,
              onSelectionMenuTap: (items) => menuItems = items,
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final pageView = tester.widget<PageView>(find.byType(PageView));
      expect(pageView.controller!.viewportFraction, 0.92);
      expect(tester.getSize(find.byType(PageView)).height, 192);
      await tester.tap(
        find.byWidgetPredicate(
          (widget) =>
              widget is SectionHeaderIconButton &&
              widget.icon == Icons.play_arrow,
        ),
      );
      await tester.pumpAndSettle();
      expect(bridge.playedItems, tracks);
      await tester.tap(find.text('Track 1'));
      await tester.pumpAndSettle();
      expect(bridge.playedItems, tracks.skip(1).toList());
      expect(navigatedTrack, isNull);
      final first = find
          .ancestor(of: find.text('Track 0'), matching: find.byType(Material))
          .first;
      await tester.tap(
        find.descendant(
          of: first,
          matching: find.byTooltip('Weitere Optionen'),
        ),
      );
      await tester.pumpAndSettle();
      expect(menuItems, [tracks.first]);
      final menuCenters = List.generate(
        3,
        (index) =>
            tester.getCenter(find.byTooltip('Weitere Optionen').at(index)),
      );
      Finder rowCheckbox(int index) => find.descendant(
        of: find
            .ancestor(
              of: find.text('Track $index'),
              matching: find.byType(Material),
            )
            .first,
        matching: find.byType(MobileSelectionCheckBox),
      );
      final rowBounds = tester.getRect(first);
      await tester.longPress(find.text('Track 0'));
      await tester.pumpAndSettle();
      expect(SelectionController.instance.selectedCount, 1);
      expect(find.byTooltip('Weitere Optionen'), findsNothing);
      expect(find.byType(MobileSelectableSectionItem), findsNothing);
      expect(tester.getRect(first), rowBounds);
      for (var index = 0; index < 3; index++) {
        final checkbox = rowCheckbox(index);
        expect(checkbox, findsOneWidget);
        expect(tester.getCenter(checkbox), menuCenters[index]);
        expect(tester.getSize(checkbox), const Size(20, 20));
        expect(
          tester.widget<MobileSelectionCheckBox>(checkbox).isChecked,
          index == 0,
        );
      }
      await tester.tapAt(tester.getCenter(rowCheckbox(1)));
      await tester.pumpAndSettle();
      expect(SelectionController.instance.selectedCount, 2);
      SelectionController.instance.onMenuTap();
      expect(menuItems, tracks.take(2).toList());
      await tester.tapAt(tester.getCenter(rowCheckbox(0)));
      await tester.pumpAndSettle();
      expect(SelectionController.instance.selectedCount, 1);
      await tester.tapAt(tester.getCenter(rowCheckbox(1)));
      await tester.pumpAndSettle();
      expect(SelectionController.instance.isVisible, isFalse);
      expect(find.byType(MobileSelectionCheckBox), findsNothing);
      expect(find.byTooltip('Weitere Optionen'), findsAtLeastNWidgets(3));
      for (var index = 0; index < 3; index++) {
        expect(
          tester.getCenter(find.byTooltip('Weitere Optionen').at(index)),
          menuCenters[index],
        );
      }
      await tester.longPress(find.text('Track 0'));
      await tester.pumpAndSettle();
      await tester.tapAt(tester.getCenter(find.text('Track 2')));
      await tester.pumpAndSettle();
      expect(SelectionController.instance.selectedCount, 2);
      SelectionController.instance.onClose();
      await tester.pumpAndSettle();
      expect(SelectionController.instance.isVisible, isFalse);
      expect(find.byType(MobileSelectionCheckBox), findsNothing);
      expect(find.byTooltip('Weitere Optionen'), findsAtLeastNWidgets(3));
      await tester.drag(find.byType(PageView), const Offset(-350, 0));
      await tester.pumpAndSettle();
      expect(pageView.controller!.page, closeTo(1, 0.01));
      final fourth = find
          .ancestor(of: find.text('Track 3'), matching: find.byType(Material))
          .first;
      expect(fourth, findsOneWidget);
      final cover = find.descendant(
        of: fourth,
        matching: find.byType(MediaTableCover),
      );
      tester.widget<MediaTableCover>(cover).onPlayTap!();
      await tester.pumpAndSettle();
      expect(bridge.playedItems, tracks.skip(3).toList());
      expect(tester.takeException(), isNull);
    },
  );

  for (final isMobile in [false, true]) {
    testWidgets(
      'track list shows metadata and independent cover/menu actions, mobile=$isMobile',
      (tester) async {
        final track = Track.fromJson({
          'item_id': 'track',
          'provider': 'source',
          'name': 'A long track title',
          'duration': 245,
          'artists': [
            {'item_id': 'artist', 'provider': 'source', 'name': 'Track artist'},
          ],
        });
        final bridge = _RecommendationBridge();
        addTearDown(bridge.eventsController.close);
        var menuCount = 0;
        var openCount = 0;
        for (final width in [280.0, 380.0, 520.0]) {
          await tester.pumpWidget(
            MaterialApp(
              theme: AppTheme.light(),
              home: Scaffold(
                body: Align(
                  alignment: Alignment.topLeft,
                  child: SizedBox(
                    width: width,
                    child: isMobile
                        ? MediaListSectionMobile<Track>(
                            title: '',
                            items: [track],
                            isLoading: false,
                            emptyText: 'Empty',
                            coreBridge: bridge,
                            onItemTap: (item) => openCount++,
                            onSelectionMenuTap: (items) {
                              expect(items, [track]);
                              menuCount++;
                            },
                          )
                        : MediaListSectionDesktop<Track>(
                            title: '',
                            items: [track],
                            isLoading: false,
                            emptyText: 'Empty',
                            coreBridge: bridge,
                            onItemTap: (item) => openCount++,
                            onItemMoreAnchorTap: (item, index, anchor) {
                              expect(item, same(track));
                              menuCount++;
                            },
                            enableSelection: false,
                          ),
                  ),
                ),
              ),
            ),
          );
          await tester.pumpAndSettle();
          expect(find.text('Track artist'), findsOneWidget);
          expect(find.text('4:05'), findsOneWidget);
          if (!isMobile) {
            expect(
              tester.getSize(find.byType(MediaItemProviderIcon)),
              const Size(40, 48),
            );
            expect(
              tester
                  .getSize(
                    find
                        .ancestor(
                          of: find.text('4:05'),
                          matching: find.byType(SizedBox),
                        )
                        .first,
                  )
                  .width,
              36,
            );
            expect(
              tester
                  .getSize(
                    find
                        .ancestor(
                          of: find.byTooltip('Weitere Optionen'),
                          matching: find.byType(SizedBox),
                        )
                        .first,
                  )
                  .width,
              32,
            );
          }
          expect(
            tester
                .widget<MediaItemProviderIcon>(
                  find.byType(MediaItemProviderIcon),
                )
                .item,
            same(track),
          );
          expect(
            tester
                .widget<MediaTableCover>(find.byType(MediaTableCover))
                .coverUrl,
            track.imageUriForSize(80),
          );
          await tester.tap(find.byTooltip('Weitere Optionen'));
          await tester.tap(find.text('A long track title'));
          expect(tester.takeException(), isNull);
        }
        expect(menuCount, 3);
        expect(openCount, 3);
        expect(bridge.playedItems, isEmpty);
        tester
            .widget<MediaTableCover>(find.byType(MediaTableCover))
            .onPlayTap!();
        await tester.pumpAndSettle();
        expect(bridge.playedItems, [track]);
      },
    );
  }

  for (final isHome in [true, false]) {
    for (final isCompact in [false, true]) {
      testWidgets(
        'Last.fm controls render without overflow, home=$isHome compact=$isCompact',
        (tester) async {
          tester.view.physicalSize = isCompact
              ? const Size(390, 844)
              : const Size(1600, 900);
          tester.view.devicePixelRatio = 1;
          addTearDown(tester.view.resetPhysicalSize);
          addTearDown(tester.view.resetDevicePixelRatio);
          final bridge = _RecommendationBridge();
          final events = MashinCoreBridgeEvents(coreBridge: bridge);
          addTearDown(events.dispose);
          addTearDown(bridge.eventsController.close);
          bridge.result.complete(
            RecommendationSnapshot.fromJson({
              'is_loading': false,
              'has_load_error': false,
              'folders': [
                if (isHome)
                  for (final genre in ['rock', 'pop', 'jazz'])
                    _folder('user_genre_artists_$genre', 'artist', [
                      '$genre-artist',
                    ])
                else ...[
                  _folder('global_genre_albums_rock', 'album', ['rock-album']),
                  _folder('global_genre_artists_rock', 'artist', [
                    'rock-artist',
                  ]),
                ],
              ],
            }),
          );
          await tester.pumpWidget(
            MaterialApp(
              theme: AppTheme.light(),
              home: Scaffold(
                body: isHome
                    ? isCompact
                          ? HomeScreenMobile(
                              coreBridge: bridge,
                              coreEvents: events,
                            )
                          : HomeScreenDesktop(
                              coreBridge: bridge,
                              coreEvents: events,
                            )
                    : isCompact
                    ? ExploreScreenMobile(
                        coreBridge: bridge,
                        coreEvents: events,
                      )
                    : ExploreScreenDesktop(
                        coreBridge: bridge,
                        coreEvents: events,
                      ),
              ),
            ),
          );
          await tester.pumpAndSettle();
          final sections = <(String, Type)>[
            if (isHome) ...[
              (
                'Unsere Empfehlungen für dich',
                isCompact
                    ? MediaSlideSectionMobile<Track>
                    : MediaSlideSectionDesktop<Track>,
              ),
              (
                'Genres, die du liebst',
                isCompact
                    ? MediaListSectionMobile<Playlist>
                    : MediaListSectionDesktop<Playlist>,
              ),
              (
                'Playlists, die du lieben wirst',
                isCompact
                    ? MediaRowSectionMobile<Playlist>
                    : MediaRowSectionDesktop<Playlist>,
              ),
              (
                'Deine meistgehörten Tracks',
                isCompact
                    ? MediaListSectionMobile<Track>
                    : MediaListSectionDesktop<Track>,
              ),
              (
                'Deine meistgehörten Künstler:innen',
                isCompact
                    ? MediaRowSectionMobile<Artist>
                    : MediaRowSectionDesktop<Artist>,
              ),
              (
                'Künstler-Empfehlungen',
                isCompact
                    ? MediaRowSectionMobile<Artist>
                    : MediaRowSectionDesktop<Artist>,
              ),
              (
                'Deine meistgehörten Alben',
                isCompact
                    ? MediaRowSectionMobile<Album>
                    : MediaRowSectionDesktop<Album>,
              ),
              (
                'Album-Empfehlungen',
                isCompact
                    ? MediaRowSectionMobile<Album>
                    : MediaRowSectionDesktop<Album>,
              ),
              (
                'Lange nicht gehört',
                isCompact
                    ? MediaListSectionMobile<Track>
                    : MediaListSectionDesktop<Track>,
              ),
              for (final genre in ['rock', 'pop', 'jazz'])
                (
                  'user_genre_artists_$genre',
                  isCompact
                      ? MediaRowSectionMobile<Artist>
                      : MediaRowSectionDesktop<Artist>,
                ),
            ] else ...[
              (
                'Das hören andere',
                isCompact
                    ? MediaRowSectionMobile<Playlist>
                    : MediaRowSectionDesktop<Playlist>,
              ),
              (
                'Track-Charts weltweit',
                isCompact
                    ? MediaListSectionMobile<Track>
                    : MediaListSectionDesktop<Track>,
              ),
              (
                'Künstler-Charts weltweit',
                isCompact
                    ? MediaRowSectionMobile<Artist>
                    : MediaRowSectionDesktop<Artist>,
              ),
              (
                'Track-Charts Deutschland',
                isCompact
                    ? MediaListSectionMobile<Track>
                    : MediaListSectionDesktop<Track>,
              ),
              (
                'Künstler-Charts Deutschland',
                isCompact
                    ? MediaRowSectionMobile<Artist>
                    : MediaRowSectionDesktop<Artist>,
              ),
              (
                'Musik für deine Stimmung',
                isCompact
                    ? MediaRowSectionMobile<Playlist>
                    : MediaRowSectionDesktop<Playlist>,
              ),
              (
                'Genres entdecken',
                isCompact
                    ? MediaListSectionMobile<Playlist>
                    : MediaListSectionDesktop<Playlist>,
              ),
              (
                'global_genre_artists_rock',
                isCompact
                    ? MediaRowSectionMobile<Artist>
                    : MediaRowSectionDesktop<Artist>,
              ),
              (
                'Jahrzehnt-Playlists',
                isCompact
                    ? MediaRowSectionMobile<Playlist>
                    : MediaRowSectionDesktop<Playlist>,
              ),
              (
                'Alben weltweit beliebter Künstler:innen',
                isCompact
                    ? MediaRowSectionMobile<Album>
                    : MediaRowSectionDesktop<Album>,
              ),
              (
                'Alben in Deutschland beliebter Künstler:innen',
                isCompact
                    ? MediaRowSectionMobile<Album>
                    : MediaRowSectionDesktop<Album>,
              ),
              (
                'global_genre_albums_rock',
                isCompact
                    ? MediaRowSectionMobile<Album>
                    : MediaRowSectionDesktop<Album>,
              ),
            ],
          ];
          if (!isHome) {
            final sectionTitles = tester
                .widget<CustomScrollView>(find.byType(CustomScrollView))
                .slivers
                .whereType<SliverPadding>()
                .map((padding) => padding.child)
                .whereType<SliverToBoxAdapter>()
                .map((sliver) => sliver.child)
                .where(
                  (widget) =>
                      widget is MediaListSectionDesktop<Track> ||
                      widget is MediaListSectionMobile<Track> ||
                      widget is MediaRowSectionDesktop<Artist> ||
                      widget is MediaRowSectionMobile<Artist> ||
                      widget is MediaRowSectionDesktop<Playlist> ||
                      widget is MediaRowSectionMobile<Playlist> ||
                      widget is MediaListSectionDesktop<Playlist> ||
                      widget is MediaListSectionMobile<Playlist> ||
                      widget is MediaRowSectionDesktop<Album> ||
                      widget is MediaRowSectionMobile<Album>,
                )
                .map(
                  (widget) => switch (widget) {
                    MediaListSectionDesktop<Track> section => section.title,
                    MediaListSectionMobile<Track> section => section.title,
                    MediaRowSectionDesktop<Artist> section => section.title,
                    MediaRowSectionMobile<Artist> section => section.title,
                    MediaRowSectionDesktop<Playlist> section => section.title,
                    MediaRowSectionMobile<Playlist> section => section.title,
                    MediaListSectionDesktop<Playlist> section => section.title,
                    MediaListSectionMobile<Playlist> section => section.title,
                    MediaRowSectionDesktop<Album> section => section.title,
                    MediaRowSectionMobile<Album> section => section.title,
                    _ => throw StateError('Unexpected Explore section'),
                  },
                );
            expect(sectionTitles, sections.map((section) => section.$1));
            final playlistRows = tester
                .widget<CustomScrollView>(find.byType(CustomScrollView))
                .slivers
                .whereType<SliverPadding>()
                .map((padding) => padding.child)
                .whereType<SliverToBoxAdapter>()
                .map((sliver) => sliver.child);
            for (final row in playlistRows) {
              switch (row) {
                case MediaRowSectionDesktop<Playlist> section:
                  expect(
                    section.playlistCoverBuilder != null,
                    section.title == 'Musik für deine Stimmung',
                  );
                case MediaRowSectionMobile<Playlist> section:
                  expect(
                    section.playlistCoverBuilder != null,
                    section.title == 'Musik für deine Stimmung',
                  );
              }
            }
          }
          for (final (title, controlType) in sections) {
            for (
              var step = 0;
              step < 60 && find.text(title).evaluate().isEmpty;
              step++
            ) {
              await tester.drag(
                find.byType(CustomScrollView),
                const Offset(0, -300),
              );
              await tester.pumpAndSettle();
            }
            expect(
              find.text(title),
              findsOneWidget,
              reason: 'Missing section: $title',
            );
            await tester.ensureVisible(find.text(title));
            await tester.pumpAndSettle();
            expect(
              find.ancestor(
                of: find.text(title).first,
                matching: find.byType(controlType),
              ),
              findsOneWidget,
            );
            expect(tester.takeException(), isNull);
          }
          await tester.pumpWidget(const SizedBox.shrink());
        },
      );
    }
  }

  for (final view in RecommendationView.values) {
    test(
      '${view.name} preserves native item order across favorites, reload and new controllers',
      () async {
        final bridge = _RecommendationBridge();
        final home = view == RecommendationView.home
            ? HomeController(coreBridge: bridge)
            : null;
        final explore = view == RecommendationView.explore
            ? ExploreController(coreBridge: bridge)
            : null;
        HomeController? reopenedHome;
        ExploreController? reopenedExplore;
        addTearDown(() async {
          home?.dispose();
          explore?.dispose();
          reopenedHome?.dispose();
          reopenedExplore?.dispose();
          await bridge.eventsController.close();
        });
        final identities = view == RecommendationView.home
            ? <(String, String)>[
                ('user_recommendations', 'track'),
                ('user_genre_playlists_monthly', 'playlist'),
                ('user_artist_playlists_monthly', 'playlist'),
                ('user_artist_recommendations', 'artist'),
                ('user_album_recommendations', 'album'),
                ('user_rediscovery_tracks', 'track'),
                for (final genre in ['rock', 'pop', 'jazz'])
                  ('user_genre_artists_$genre', 'artist'),
              ]
            : <(String, String)>[
                ('global_artist_radio_playlists', 'playlist'),
                ('global_mood_playlists', 'playlist'),
                ('global_genre_playlists', 'playlist'),
                ('global_popular_artist_albums', 'album'),
                ('de_popular_artist_albums', 'album'),
                for (final genre in ['rock', 'pop', 'jazz']) ...[
                  ('global_genre_artists_$genre', 'artist'),
                  ('global_genre_albums_$genre', 'album'),
                ],
              ];
        final folders = [
          for (final (identity, type) in identities)
            _folder(identity, type, ['third', 'first', 'second']),
        ];
        Map<String, List<MediaItem>> homeItems(HomeController controller) => {
          'user_recommendations': controller.recommendationItems,
          'user_genre_playlists_monthly': controller.genrePlaylists,
          'user_artist_playlists_monthly': controller.artistPlaylists,
          'user_artist_recommendations': controller.artistRecommendations,
          'user_album_recommendations': controller.albumRecommendations,
          'user_rediscovery_tracks': controller.rediscoveryTracks,
          for (final section in controller.artistSections)
            section.title: section.artists,
        };
        void expectOrder(
          HomeController? homeController,
          ExploreController? exploreController,
        ) {
          for (final (identity, _) in identities) {
            final items = homeController != null
                ? homeItems(homeController)[identity]!
                : exploreController!.itemsForFolder<MediaItem>(identity);
            expect(items.map((item) => item.itemId), [
              'third',
              'first',
              'second',
            ], reason: identity);
          }
        }

        final loading = home?.load() ?? explore!.load();
        await bridge.requested.future;
        bridge.emitFolders(view, folders);
        expectOrder(home, explore);
        home?.refreshFavoriteStatuses();
        explore?.refreshFavoriteStatuses();
        expectOrder(home, explore);
        bridge.result.complete(
          RecommendationSnapshot.fromJson({
            'folders': folders,
            'is_loading': false,
            'has_load_error': false,
          }),
        );
        await loading;
        await Future<void>.delayed(Duration.zero);
        await (home?.load(refresh: true) ?? explore!.load(refresh: true));
        await Future<void>.delayed(Duration.zero);
        expectOrder(home, explore);
        if (view == RecommendationView.home) {
          reopenedHome = HomeController(coreBridge: bridge);
          await reopenedHome.load();
        } else {
          reopenedExplore = ExploreController(coreBridge: bridge);
          await reopenedExplore.load();
        }
        await Future<void>.delayed(Duration.zero);
        expectOrder(reopenedHome, reopenedExplore);
      },
    );
  }

  test(
    'home exposes all personal Last.fm sections in provider order',
    () async {
      final bridge = _RecommendationBridge();
      final controller = HomeController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      await controller.load();
      await bridge.requested.future;
      final folders = [
        _folder('user_recommendations', 'track', ['third', 'first', 'second']),
        _folder(
          'user_genre_playlists_monthly',
          'playlist',
          List.generate(10, (index) => 'genre-$index'),
        ),
        _folder('user_artist_playlists_monthly', 'playlist', ['artist-radio']),
        _folder('user_artist_recommendations', 'artist', [
          'recommended-artist',
        ]),
        _folder('user_top_albums_monthly', 'album', ['top-album']),
        _folder('user_album_recommendations', 'album', ['recommended-album']),
        _folder('user_rediscovery_tracks', 'track', ['forgotten']),
        for (final genre in ['rock', 'pop', 'jazz'])
          _folder('user_genre_artists_$genre', 'artist', ['$genre-artist']),
      ];
      bridge.emitFolders(RecommendationView.home, folders);
      expect(controller.recommendationItems.map((item) => item.itemId), [
        'third',
        'first',
        'second',
      ]);
      expect(controller.genrePlaylists, hasLength(10));
      expect(controller.artistPlaylists.single.itemId, 'artist-radio');
      expect(
        controller.artistRecommendations.single.itemId,
        'recommended-artist',
      );
      expect(controller.topAlbums.single.itemId, 'top-album');
      expect(
        controller.albumRecommendations.single.itemId,
        'recommended-album',
      );
      expect(controller.rediscoveryTracks.single.itemId, 'forgotten');
      expect(controller.artistSections.map((section) => section.title), [
        'user_genre_artists_rock',
        'user_genre_artists_pop',
        'user_genre_artists_jazz',
      ]);
      var changes = 0;
      controller.personalSectionsListenable.addListener(() => changes++);
      bridge.emitFolders(RecommendationView.home, folders);
      expect(changes, 0);
      bridge.emitFolders(RecommendationView.home, [
        for (final folder in folders) {...folder, 'is_loading': true},
      ], isLoading: true);
      expect(changes, 1);
      expect(
        controller.albumRecommendations.single.itemId,
        'recommended-album',
      );
      bridge.emitFolders(RecommendationView.home, [
        for (final folder in folders)
          {...folder, 'items': [], 'has_load_error': true},
      ], hasLoadError: true);
      expect(controller.artistSections, hasLength(3));
      expect(controller.rediscoveryTracks.single.itemId, 'forgotten');
      bridge.emitFolders(RecommendationView.home, []);
      expect(controller.artistRecommendations, isEmpty);
      expect(controller.topAlbums, isEmpty);
      expect(controller.albumRecommendations, isEmpty);
      expect(controller.rediscoveryTracks, isEmpty);
      expect(controller.artistSections, isEmpty);
      bridge.result.complete(const RecommendationSnapshot());
      await Future<void>.delayed(Duration.zero);
    },
  );

  test(
    'explore exposes playlists and all dynamic genre folders without shuffling',
    () async {
      final bridge = _RecommendationBridge();
      final controller = ExploreController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      final loading = controller.load();
      await bridge.requested.future;
      final folders = [
        _folder('global_top_tracks', 'track', ['third', 'first', 'second']),
        for (final identity in [
          'global_genre_playlists',
          'global_artist_radio_playlists',
          'global_mood_playlists',
          'global_decade_playlists',
        ])
          _folder(identity, 'playlist', [identity]),
        for (final genre in ['rock', 'pop', 'jazz']) ...[
          _folder('global_genre_albums_$genre', 'album', ['$genre-album']),
          _folder('global_genre_artists_$genre', 'artist', ['$genre-artist']),
        ],
      ];
      bridge.emitFolders(RecommendationView.explore, folders);
      expect(
        controller
            .itemsForFolder<Track>('global_top_tracks')
            .map((item) => item.itemId),
        ['third', 'first', 'second'],
      );
      expect(
        controller.foldersWithPrefix('global_genre_albums_'),
        hasLength(3),
      );
      expect(
        controller.foldersWithPrefix('global_genre_artists_'),
        hasLength(3),
      );
      expect(
        controller
            .itemsForFolder<Playlist>('global_mood_playlists')
            .single
            .itemId,
        'global_mood_playlists',
      );
      bridge.emitFolders(RecommendationView.explore, [
        for (final folder in folders)
          {...folder, 'items': [], 'has_load_error': true},
      ], hasLoadError: true);
      expect(
        controller.itemsForFolder<Playlist>('global_mood_playlists'),
        hasLength(1),
      );
      expect(
        controller.foldersWithPrefix('global_genre_albums_').first.items,
        hasLength(1),
      );
      bridge.emitFolders(RecommendationView.explore, []);
      expect(controller.foldersWithPrefix('global_genre_albums_'), isEmpty);
      bridge.result.complete(const RecommendationSnapshot());
      await loading;
    },
  );

  test(
    'home refreshes history after thirty seconds of playback and track end',
    () async {
      var now = DateTime(2026);
      final bridge = _RecommendationBridge();
      final controller = HomeController(coreBridge: bridge, now: () => now);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      bridge.emitPlayback('first');
      expect(bridge.recentReadCount, 0);
      await controller.load();
      bridge.result.complete(const RecommendationSnapshot());
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 1);
      bridge.emitPlayback('first');
      now = now.add(const Duration(seconds: 29));
      bridge.emitPlayback('first', positionMillis: 29000);
      expect(bridge.recentReadCount, 1);
      now = now.add(const Duration(seconds: 1));
      bridge.emitPlayback('second');
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 2);
      expect(controller.recentListens.single.name, 'History 2');
      now = now.add(const Duration(seconds: 1));
      bridge.emitPlayback('third');
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 2);
      now = now.add(const Duration(seconds: 30));
      bridge.emitPlayback('third', playbackState: PlayState.idle);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 3);
    },
  );

  test(
    'home ignores paused time and throttles after another history refresh',
    () async {
      var now = DateTime(2026);
      final bridge = _RecommendationBridge();
      final controller = HomeController(coreBridge: bridge, now: () => now);
      addTearDown(bridge.eventsController.close);
      await controller.load();
      bridge.result.complete(const RecommendationSnapshot());
      await Future<void>.delayed(Duration.zero);
      bridge.emitPlayback('first');
      now = now.add(const Duration(seconds: 10));
      bridge.emitPlayback('first', playbackState: PlayState.paused);
      now = now.add(const Duration(seconds: 60));
      bridge.emitPlayback('first');
      now = now.add(const Duration(seconds: 19));
      bridge.emitPlayback('second');
      expect(bridge.recentReadCount, 1);
      now = now.add(const Duration(seconds: 30));
      bridge.emitPlayback('second', positionMillis: 30000);
      expect(bridge.recentReadCount, 1);
      bridge.eventsController.add({'type': 'provider_instances_changed'});
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 2);
      now = now.add(const Duration(seconds: 1));
      bridge.emitPlayback('third');
      expect(bridge.recentReadCount, 2);
      now = now.add(const Duration(seconds: 30));
      bridge.emitPlayback('third', playbackState: PlayState.idle);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 3);
      controller.dispose();
      now = now.add(const Duration(seconds: 60));
      bridge.emitPlayback('fourth');
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 3);
    },
  );

  test(
    'home refreshes history for repeat one without refreshing every position',
    () async {
      var now = DateTime(2026);
      final bridge = _RecommendationBridge();
      final controller = HomeController(coreBridge: bridge, now: () => now);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      await controller.load();
      bridge.result.complete(const RecommendationSnapshot());
      await Future<void>.delayed(Duration.zero);
      bridge.emitPlayback('repeated', durationMillis: 60000);
      for (var repeat = 0; repeat < 2; repeat++) {
        now = now.add(const Duration(seconds: 59));
        bridge.emitPlayback(
          'repeated',
          positionMillis: 59000,
          durationMillis: 60000,
        );
        expect(bridge.recentReadCount, repeat + 1);
        now = now.add(const Duration(seconds: 1));
        bridge.emitPlayback('repeated', durationMillis: 60000);
        await Future<void>.delayed(Duration.zero);
        expect(bridge.recentReadCount, repeat + 2);
        bridge.emitPlayback('repeated', durationMillis: 60000);
        expect(bridge.recentReadCount, repeat + 2);
      }
    },
  );

  test(
    'home coalesces track ends during a pending history read and retains failed data',
    () async {
      var now = DateTime(2026);
      final pending = Completer<List<MediaItem>>();
      final bridge = _RecommendationBridge(
        recentLoader: (count) {
          if (count == 1) return pending.future;
          return Future.error(StateError('offline'));
        },
      );
      final controller = HomeController(coreBridge: bridge, now: () => now);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      await controller.load();
      bridge.result.complete(const RecommendationSnapshot());
      bridge.emitPlayback('first');
      now = now.add(const Duration(seconds: 30));
      bridge.emitPlayback('second');
      now = now.add(const Duration(seconds: 30));
      bridge.emitPlayback('third');
      expect(bridge.recentReadCount, 1);
      pending.complete([
        Track.fromJson({
          'provider': 'source',
          'item_id': 'cached',
          'name': 'Retained',
        }),
      ]);
      await Future<void>.delayed(Duration.zero);
      expect(bridge.recentReadCount, 2);
      expect(controller.recentListens.single.name, 'Retained');
      expect(controller.isLoadingRecentListens, isFalse);
    },
  );

  test(
    'home retains early cached recent tracks when the pending read fails',
    () async {
      for (final hasTracks in [true, false]) {
        final recent = Completer<List<MediaItem>>();
        final bridge = _RecommendationBridge(recentResult: recent);
        final controller = HomeController(coreBridge: bridge);
        addTearDown(() async {
          controller.dispose();
          await bridge.eventsController.close();
        });
        await controller.load();
        await bridge.requested.future;
        bridge.eventsController.add({
          'type': 'catalog_updated',
          'query': {
            'RecentlyPlayed': {'limit': 40},
          },
          'value': {
            'Tracks': [
              if (hasTracks)
                {
                  'provider': 'source',
                  'item_id': 'recent',
                  'name': 'Cached recent',
                },
            ],
          },
        });
        final cached = controller.recentListens;
        if (hasTracks) {
          expect(cached.single.name, 'Cached recent');
        } else {
          expect(cached, isEmpty);
        }
        expect(controller.isLoadingRecentListens, isFalse);
        expect(recent.isCompleted, isFalse);
        recent.completeError(StateError('offline'));
        bridge.result.complete(const RecommendationSnapshot());
        await Future<void>.delayed(Duration.zero);
        expect(identical(controller.recentListens, cached), isTrue);
        expect(controller.isLoadingRecentListens, isFalse);
      }
    },
  );

  test(
    'home retains unknown or failed sections and accepts authoritative empty',
    () async {
      final bridge = _RecommendationBridge();
      final controller = HomeController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      await controller.load();
      await bridge.requested.future;
      bridge.emit(RecommendationView.home);
      final tracks = controller.topTracks;
      bridge.emitEmpty(RecommendationView.home, isLoading: true);
      expect(identical(controller.topTracks, tracks), isTrue);
      bridge.emitEmpty(RecommendationView.home, hasLoadError: true);
      expect(identical(controller.topTracks, tracks), isTrue);
      bridge.emitEmpty(RecommendationView.home);
      expect(controller.topTracks, isEmpty);
      bridge.result.complete(const RecommendationSnapshot());
      await Future<void>.delayed(Duration.zero);
    },
  );

  test(
    'explore retains unknown or failed sections and accepts authoritative empty',
    () async {
      final bridge = _RecommendationBridge();
      final controller = ExploreController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      final loading = controller.load();
      await bridge.requested.future;
      bridge.emit(RecommendationView.explore);
      final tracks = controller.itemsForFolder<Track>('global_top_tracks');
      bridge.emitEmpty(RecommendationView.explore, isLoading: true);
      expect(
        hasSameMediaListContent(
          controller.itemsForFolder<Track>('global_top_tracks'),
          tracks,
        ),
        isTrue,
      );
      bridge.emitEmpty(RecommendationView.explore, hasLoadError: true);
      expect(
        hasSameMediaListContent(
          controller.itemsForFolder<Track>('global_top_tracks'),
          tracks,
        ),
        isTrue,
      );
      bridge.emitEmpty(RecommendationView.explore);
      expect(controller.itemsForFolder<Track>('global_top_tracks'), isEmpty);
      bridge.result.complete(const RecommendationSnapshot());
      await loading;
    },
  );

  test(
    'home publishes early sections and retains identical content and failed refresh',
    () async {
      final bridge = _RecommendationBridge();
      final controller = HomeController(coreBridge: bridge);
      addTearDown(() async {
        controller.dispose();
        await bridge.eventsController.close();
      });
      await controller.load();
      await bridge.requested.future;
      bridge.emit(RecommendationView.home);
      final tracks = controller.topTracks;
      final artists = controller.topArtists;
      expect(tracks.single.name, 'Cached');
      expect(bridge.result.isCompleted, isFalse);
      var tracksChanges = 0;
      var artistsChanges = 0;
      controller.topTracksListenable.addListener(() => tracksChanges++);
      controller.topArtistsListenable.addListener(() => artistsChanges++);
      bridge.emit(RecommendationView.home);
      expect(tracksChanges, 0);
      expect(artistsChanges, 0);
      expect(identical(controller.topTracks, tracks), isTrue);
      bridge.emit(RecommendationView.explore, name: 'Wrong view');
      expect(identical(controller.topTracks, tracks), isTrue);
      bridge.emit(RecommendationView.home, name: 'Server');
      expect(tracksChanges, 1);
      expect(artistsChanges, 0);
      expect(identical(controller.topArtists, artists), isTrue);
      final updated = controller.topTracks;
      bridge.result.completeError(StateError('offline'));
      await Future<void>.delayed(Duration.zero);
      expect(controller.hasLoadError, isTrue);
      expect(identical(controller.topTracks, updated), isTrue);
    },
  );

  test(
    'explore accepts early snapshots, suppresses duplicates and rejects disposed events',
    () async {
      final bridge = _RecommendationBridge();
      final controller = ExploreController(coreBridge: bridge);
      addTearDown(bridge.eventsController.close);
      final loading = controller.load();
      await bridge.requested.future;
      bridge.emit(RecommendationView.explore);
      final tracks = controller.itemsForFolder<Track>('global_top_tracks');
      final artists = controller.itemsForFolder<Artist>('global_top_artists');
      expect(tracks.single.name, 'Cached');
      expect(bridge.result.isCompleted, isFalse);
      var changes = 0;
      controller.addListener(() => changes++);
      bridge.emit(RecommendationView.explore);
      expect(changes, 0);
      expect(
        identical(
          controller.itemsForFolder<Track>('global_top_tracks').single,
          tracks.single,
        ),
        isTrue,
      );
      bridge.emit(RecommendationView.home, name: 'Wrong view');
      expect(changes, 0);
      bridge.emit(RecommendationView.explore, name: 'Server');
      expect(changes, 1);
      expect(
        identical(
          controller.itemsForFolder<Artist>('global_top_artists').single,
          artists.single,
        ),
        isTrue,
      );
      final updated = controller.itemsForFolder<Track>('global_top_tracks');
      controller.dispose();
      bridge.emit(RecommendationView.explore, name: 'Late');
      bridge.result.complete(const RecommendationSnapshot());
      await loading;
      expect(
        identical(
          controller.itemsForFolder<Track>('global_top_tracks').single,
          updated.single,
        ),
        isTrue,
      );
    },
  );
}

Map<String, dynamic> _folder(
  String identity,
  String mediaType,
  List<String> items,
) => {
  'provider': 'lastfm_recommendations_enhanced--test',
  'item_id': identity,
  'title': identity,
  'items': [
    for (final itemId in items)
      {
        'media_type': mediaType,
        'provider': 'source',
        'item_id': itemId,
        'name': itemId,
      },
  ],
};

class _RecommendationBridge implements MashinCoreBridgeApi {
  _RecommendationBridge({this.recentResult, this.recentLoader});

  final Completer<List<MediaItem>>? recentResult;
  final Future<List<MediaItem>> Function(int)? recentLoader;
  final eventsController = StreamController<Map<String, dynamic>>.broadcast(
    sync: true,
  );
  final result = Completer<RecommendationSnapshot>();
  final requested = Completer<void>();
  int recentReadCount = 0;
  List<MediaItem> playedItems = const [];

  @override
  Future<void> queuePlayMedia(
    List<MediaItem> mediaItems, {
    QueueOption option = QueueOption.replace,
    int? startItem,
    bool shuffle = false,
    String? providerInstance,
  }) async {
    playedItems = mediaItems;
  }

  @override
  Stream<PlayerState> get playerStateChanges => eventsController.stream
      .where((event) => event['type'] == 'player_state_changed')
      .map(
        (event) =>
            PlayerState.fromJson(event['player_state'] as Map<String, dynamic>),
      );

  void emitPlayback(
    String itemId, {
    PlayState playbackState = PlayState.playing,
    int positionMillis = 0,
    int durationMillis = 180000,
  }) {
    eventsController.add({
      'type': 'player_state_changed',
      'player_state': {
        'connectionState': 'online',
        'playbackState': playbackState.name,
        'currentQueueItemId': itemId,
        'positionMillis': positionMillis,
        'durationMillis': durationMillis,
      },
    });
  }

  @override
  Stream<Map<String, dynamic>> get events => eventsController.stream;

  void emitFolders(
    RecommendationView view,
    List<Map<String, dynamic>> folders, {
    bool isLoading = false,
    bool hasLoadError = false,
  }) => eventsController.add({
    'type': 'recommendations_updated',
    'view': view.name,
    'snapshot': {
      'folders': folders,
      'is_loading': isLoading,
      'has_load_error': hasLoadError,
    },
  });

  void emitEmpty(
    RecommendationView view, {
    bool isLoading = false,
    bool hasLoadError = false,
  }) {
    eventsController.add({
      'type': 'recommendations_updated',
      'view': view.name,
      'snapshot': {
        'is_loading': isLoading,
        'has_load_error': hasLoadError,
        'folders': [],
      },
    });
  }

  // Exercise the production snapshot decoder through the native event contract.
  void emit(RecommendationView view, {String name = 'Cached'}) {
    eventsController.add({
      'type': 'recommendations_updated',
      'view': view.name,
      'snapshot': {
        'is_loading': true,
        'has_load_error': false,
        'folders': [
          {
            'item_id': view == RecommendationView.home
                ? 'user_top_tracks_monthly'
                : 'global_top_tracks',
            'provider': 'source',
            'title': 'Tracks',
            'is_loading': false,
            'has_load_error': false,
            'items': [
              {
                'media_type': 'track',
                'provider': 'source',
                'item_id': 'track',
                'name': name,
              },
            ],
          },
          {
            'item_id': view == RecommendationView.home
                ? 'user_top_artists_monthly'
                : 'global_top_artists',
            'provider': 'source',
            'title': 'Artists',
            'is_loading': false,
            'has_load_error': false,
            'items': [
              {
                'media_type': 'artist',
                'provider': 'source',
                'item_id': 'artist',
                'name': 'Unchanged',
              },
            ],
          },
        ],
      },
    });
  }

  @override
  Future<RecommendationSnapshot> getRecommendations({
    required RecommendationView view,
    bool refresh = true,
  }) {
    if (!requested.isCompleted) {
      requested.complete();
    }
    return result.future;
  }

  @override
  Future<List<MediaItem>> getRecentlyPlayedTracks({int limit = 40}) {
    recentReadCount++;
    return recentLoader?.call(recentReadCount) ??
        recentResult?.future ??
        Future.value([
          Track.fromJson({
            'provider': 'source',
            'item_id': 'history-$recentReadCount',
            'name': 'History $recentReadCount',
          }),
        ]);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
