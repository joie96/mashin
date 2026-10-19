import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/shared/detail_provider_selection.dart';
import 'package:mashin_flutter/features/shared/widgets/detail_provider_switcher.dart';
import 'package:mashin_flutter/features/shared/widgets/media_table_section_desktop.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() => SharedPreferences.setMockInitialValues(<String, Object>{}));

  const deezer = ProviderMapping(
    itemId: '406',
    providerDomain: 'deezer',
    providerInstance: 'deezer--account',
    available: true,
  );
  const youtube = ProviderMapping(
    itemId: 'youtube-artist',
    providerDomain: 'ytmusic',
    providerInstance: 'ytmusic--account',
    available: true,
  );
  const artist = Artist(
    itemId: '78',
    provider: 'library',
    name: 'Avenged Sevenfold',
    providerMappings: <ProviderMapping>[deezer, youtube],
  );

  test(
    'provider cycling reaches every available mapping and wraps around',
    () async {
      const third = ProviderMapping(
        itemId: 'third-artist',
        providerDomain: 'third',
        providerInstance: 'third--account',
        available: true,
      );
      final selection = DetailProviderSelection(
        itemId: '78',
        provider: 'library',
      );
      await selection.initialize(
        const Artist(
          itemId: '78',
          provider: 'library',
          name: 'Artist',
          providerMappings: <ProviderMapping>[
            deezer,
            ProviderMapping(
              itemId: 'offline',
              providerDomain: 'offline',
              providerInstance: 'offline--account',
              available: false,
            ),
            youtube,
            third,
          ],
        ),
      );
      expect(selection.nextProvider, youtube);
      await selection.select(selection.nextProvider!);
      expect(selection.nextProvider, third);
      await selection.select(selection.nextProvider!);
      expect(selection.nextProvider, deezer);
      await selection.select(selection.nextProvider!);
      expect(selection.selectedProvider, deezer.providerInstance);
    },
  );

  testWidgets(
    'mobile detail main provider keeps its anchor for one to three mappings',
    (tester) async {
      tester.view.physicalSize = const Size(390, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      Rect? originalBounds;
      Size? originalArea;
      for (final count in <int>[0, 1, 2, 3]) {
        final item = Artist(
          itemId: '78',
          provider: deezer.providerInstance,
          name: 'Artist',
          providerMappings: <ProviderMapping>[
            if (count > 0) deezer,
            if (count > 1) youtube,
            if (count > 2)
              const ProviderMapping(
                itemId: 'third',
                providerDomain: 'third',
                providerInstance: 'third--account',
                available: true,
              ),
          ],
        );
        final selection = DetailProviderSelection(
          itemId: item.itemId,
          provider: item.provider,
        );
        await selection.initialize(item);
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark(),
            home: Scaffold(
              body: Center(
                child: DetailProviderSwitcher(
                  selection: selection,
                  item: item,
                  isLoading: false,
                  onSelected: (_) async {},
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        final primary = count == 0
            ? find.byType(MediaProviderIcon)
            : find.byKey(
                const ValueKey<String>('mobile-provider-deezer--account'),
              );
        final bounds = tester.getRect(primary);
        final area = tester.getSize(find.byType(DetailProviderSwitcher));
        originalBounds ??= bounds;
        originalArea ??= area;
        expect(bounds, originalBounds);
        expect(area, originalArea);
        expect(tester.takeException(), isNull);
      }
    },
  );

  for (final rowWidth in <double>[240, 80]) {
    testWidgets(
      'desktop shows all providers and swaps directly at width $rowWidth',
      (tester) async {
        tester.view.physicalSize = const Size(1280, 800);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        const third = ProviderMapping(
          itemId: 'third',
          providerDomain: 'third',
          providerInstance: 'third--account',
          available: true,
        );
        const fourth = ProviderMapping(
          itemId: 'fourth',
          providerDomain: 'fourth',
          providerInstance: 'fourth--account',
          available: true,
        );
        const offline = ProviderMapping(
          itemId: 'offline',
          providerDomain: 'offline',
          providerInstance: 'offline--account',
          available: false,
        );
        const mappings = <ProviderMapping>[
          deezer,
          youtube,
          third,
          fourth,
          offline,
        ];
        final selection = DetailProviderSelection(
          itemId: '78',
          provider: 'library',
        );
        await selection.initialize(
          const Artist(
            itemId: '78',
            provider: 'library',
            name: 'Artist',
            providerMappings: mappings,
          ),
        );
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark().copyWith(
              visualDensity: VisualDensity.compact,
            ),
            home: Scaffold(
              body: Align(
                alignment: Alignment.topLeft,
                child: SizedBox(
                  width: rowWidth,
                  child: StatefulBuilder(
                    builder: (context, setState) => DetailProviderSwitcher(
                      selection: selection,
                      item: artist,
                      isLoading: false,
                      onSelected: (mapping) async {
                        await selection.select(mapping);
                        setState(() {});
                      },
                    ),
                  ),
                ),
              ),
            ),
          ),
        );
        Finder button(ProviderMapping mapping) =>
            find.byKey(ValueKey<String>(mapping.providerInstance));
        Finder icon(ProviderMapping mapping) => find.byKey(
          ValueKey<String>('desktop-provider-${mapping.providerInstance}'),
        );
        expect(find.byType(IconButton), findsNWidgets(mappings.length));
        expect(
          find.byKey(const ValueKey<String>('detail-provider-cycle')),
          findsNothing,
        );
        for (final mapping in mappings) {
          final String label =
              ProviderManifestRegistry.getByProvider(
                mapping.providerDomain,
              )?.name ??
              mapping.providerDomain;
          expect(
            tester.widget<IconButton>(button(mapping)).tooltip,
            mapping == deezer || !mapping.available
                ? label
                : 'Zu $label wechseln',
          );
          expect(
            tester.widget<MediaProviderIcon>(icon(mapping)).size,
            mapping == deezer ? 24 : 16,
          );
          expect(
            tester.getCenter(icon(mapping)).dy,
            tester.getCenter(button(mapping)).dy,
          );
          expect(
            tester
                .widget<IconButton>(button(mapping))
                .style!
                .overlayColor!
                .resolve(<WidgetState>{WidgetState.hovered}),
            Colors.transparent,
          );
        }
        expect(tester.widget<IconButton>(button(offline)).onPressed, isNull);
        final Rect original = tester.getRect(button(deezer));
        final Rect destination = tester.getRect(button(fourth));
        final Rect unchanged = tester.getRect(button(youtube));
        Finder hoverScale(ProviderMapping mapping) => find.descendant(
          of: button(mapping),
          matching: find.byType(AnimatedScale),
        );
        final TestGesture mouse = await tester.createGesture(
          kind: PointerDeviceKind.mouse,
        );
        await mouse.addPointer(location: const Offset(300, 700));
        for (final mapping in <ProviderMapping>[fourth, deezer, offline]) {
          await mouse.moveTo(tester.getCenter(button(mapping)));
          await tester.pumpAndSettle();
          final AnimatedScale animation = tester.widget<AnimatedScale>(
            hoverScale(mapping),
          );
          expect(animation.scale, mapping == fourth ? 1.5 : 1);
          expect(animation.duration, const Duration(milliseconds: 140));
          expect(animation.curve, Curves.easeOutCubic);
          expect(
            tester.getCenter(icon(mapping)).dy,
            tester.getCenter(button(mapping)).dy,
          );
          expect(tester.getRect(button(fourth)), destination);
        }
        await mouse.moveTo(const Offset(300, 700));
        await tester.pumpAndSettle();
        expect(tester.widget<AnimatedScale>(hoverScale(fourth)).scale, 1);
        await mouse.removePointer();
        for (final entry in mappings.indexed) {
          for (final other in mappings.skip(entry.$1 + 1)) {
            expect(
              tester
                  .getRect(button(entry.$2))
                  .overlaps(tester.getRect(button(other))),
              isFalse,
            );
          }
        }
        await tester.tap(button(fourth));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 100));
        expect(tester.getRect(button(fourth)), isNot(destination));
        expect(tester.getRect(button(fourth)), isNot(original));
        expect(
          tester.widget<MediaProviderIcon>(icon(fourth)).size,
          inExclusiveRange(16, 24),
        );
        expect(
          tester.widget<MediaProviderIcon>(icon(deezer)).size,
          inExclusiveRange(16, 24),
        );
        for (final mapping in mappings) {
          expect(
            tester.getCenter(icon(mapping)).dy,
            tester.getCenter(button(mapping)).dy,
          );
        }
        await tester.pumpAndSettle();
        expect(selection.selectedProvider, fourth.providerInstance);
        expect(tester.widget<IconButton>(button(fourth)).tooltip, 'fourth');
        expect(
          tester.widget<IconButton>(button(deezer)).tooltip,
          'Zu ${ProviderManifestRegistry.getByProvider(deezer.providerDomain)?.name ?? deezer.providerDomain} wechseln',
        );
        expect(tester.getRect(button(fourth)), original);
        expect(tester.getRect(button(deezer)), destination);
        expect(tester.getRect(button(youtube)), unchanged);
        expect(tester.widget<MediaProviderIcon>(icon(fourth)).size, 24);
        expect(tester.widget<MediaProviderIcon>(icon(deezer)).size, 16);
        expect(artist.provider, 'library');
        expect(tester.takeException(), isNull);
      },
    );
  }

  for (final width in <double>[320, 390]) {
    testWidgets('provider stack cycles all sources at width $width', (
      tester,
    ) async {
      tester.view.physicalSize = Size(width, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      const third = ProviderMapping(
        itemId: 'third-artist',
        providerDomain: 'third',
        providerInstance: 'third--account',
        available: true,
      );
      final selection = DetailProviderSelection(
        itemId: '78',
        provider: 'library',
      );
      await selection.initialize(
        const Artist(
          itemId: '78',
          provider: 'library',
          name: 'Artist',
          providerMappings: <ProviderMapping>[deezer, youtube, third],
        ),
      );
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.dark(),
          home: Scaffold(
            body: StatefulBuilder(
              builder: (context, setState) {
                return DetailProviderSwitcher(
                  selection: selection,
                  item: artist,
                  isLoading: false,
                  onSelected: (mapping) async {
                    await selection.select(mapping);
                    setState(() {});
                  },
                );
              },
            ),
          ),
        ),
      );
      final Finder cycleButton = find.byKey(
        const ValueKey<String>('detail-provider-cycle'),
      );
      expect(find.byType(IconButton), findsOneWidget);
      final ButtonStyle style = tester.widget<IconButton>(cycleButton).style!;
      for (final states in <Set<WidgetState>>[
        <WidgetState>{},
        <WidgetState>{WidgetState.hovered},
        <WidgetState>{WidgetState.disabled},
        <WidgetState>{WidgetState.focused},
        <WidgetState>{WidgetState.pressed},
      ]) {
        expect(style.backgroundColor!.resolve(states), Colors.transparent);
        expect(style.overlayColor!.resolve(states), Colors.transparent);
        expect(style.side!.resolve(states), BorderSide.none);
      }
      final Size buttonSize = tester.getSize(cycleButton);
      expect(buttonSize.height, greaterThanOrEqualTo(48));
      final Finder activeIcon = find.byKey(
        ValueKey<String>('mobile-provider-${deezer.providerInstance}'),
      );
      final Finder nextIcon = find.byKey(
        ValueKey<String>('mobile-provider-${youtube.providerInstance}'),
      );
      expect(tester.widget<MediaProviderIcon>(activeIcon).size, 24);
      expect(tester.widget<MediaProviderIcon>(nextIcon).size, 20);
      expect(tester.getCenter(activeIcon).dy, tester.getCenter(cycleButton).dy);
      expect(
        tester.getCenter(nextIcon).dy,
        tester.getCenter(activeIcon).dy - 5,
      );
      expect(
        tester.getTopLeft(nextIcon).dx,
        greaterThan(tester.getTopLeft(activeIcon).dx),
      );
      expect(
        tester.getTopLeft(nextIcon).dx,
        lessThan(tester.getTopRight(activeIcon).dx),
      );
      expect(
        tester.getTopLeft(nextIcon).dy,
        lessThan(tester.getTopLeft(activeIcon).dy),
      );
      expect(
        tester.getTopLeft(nextIcon).dx - tester.getTopLeft(activeIcon).dx,
        16,
      );
      final Offset nextStart = tester.getTopLeft(nextIcon);
      if (width == 1280) {
        final TestGesture mouse = await tester.createGesture(
          kind: PointerDeviceKind.mouse,
        );
        await mouse.addPointer(location: const Offset(300, 700));
        await mouse.moveTo(tester.getCenter(cycleButton));
        await tester.pumpAndSettle();
        expect(tester.getTopLeft(nextIcon), nextStart);
        expect(tester.getSize(cycleButton), buttonSize);
        await mouse.removePointer();
      }
      await tester.tap(cycleButton);
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 100));
      expect(tester.widget<MediaProviderIcon>(nextIcon).size, greaterThan(20));
      expect(tester.widget<MediaProviderIcon>(nextIcon).size, lessThan(24));
      expect(tester.getTopLeft(nextIcon).dx, lessThan(nextStart.dx));
      expect(tester.getTopLeft(nextIcon).dy, greaterThan(nextStart.dy));
      expect(tester.getSize(cycleButton), buttonSize);
      final Offset interruptedPosition = tester.getTopLeft(nextIcon);
      final double interruptedSize = tester
          .widget<MediaProviderIcon>(nextIcon)
          .size;
      await tester.tap(cycleButton);
      await tester.pump();
      expect(tester.getTopLeft(nextIcon), interruptedPosition);
      expect(tester.widget<MediaProviderIcon>(nextIcon).size, interruptedSize);
      await tester.pumpAndSettle();
      expect(selection.selectedProvider, third.providerInstance);
      expect(
        tester
            .getCenter(
              find.byKey(
                ValueKey<String>('mobile-provider-${third.providerInstance}'),
              ),
            )
            .dy,
        tester.getCenter(cycleButton).dy,
      );
      for (final mapping in <ProviderMapping>[deezer]) {
        await tester.tap(cycleButton);
        await tester.pumpAndSettle();
        expect(selection.selectedProvider, mapping.providerInstance);
        expect(tester.getSize(cycleButton), buttonSize);
        expect(
          tester.getCenter(activeIcon).dy,
          tester.getCenter(cycleButton).dy,
        );
        expect(
          tester
              .widget<MediaProviderIcon>(
                find.byKey(
                  ValueKey<String>(
                    'mobile-provider-${mapping.providerInstance}',
                  ),
                ),
              )
              .size,
          24,
        );
      }
      expect(artist.provider, 'library');
      expect(tester.takeException(), isNull);
    });
  }

  test(
    'first mapping supplies sections without changing library identity',
    () async {
      final selection = DetailProviderSelection(
        itemId: '78',
        provider: 'library',
      );
      await selection.initialize(artist);
      expect(selection.selectedItemId, '406');
      expect(selection.selectedProvider, deezer.providerInstance);
      expect(artist.itemId, '78');
      expect(artist.provider, 'library');
      expect(
        (await SharedPreferences.getInstance()).getStringList(
          DetailProviderSelection.preferenceKey,
        ),
        isNull,
      );
    },
  );

  test(
    'switch persists across library items and active provider is first',
    () async {
      final selection = DetailProviderSelection(
        itemId: '78',
        provider: 'library',
      );
      await selection.initialize(artist);
      await selection.select(youtube);
      expect(selection.mappings.first, youtube);
      final reopened = DetailProviderSelection(
        itemId: '79',
        provider: 'library',
      );
      await reopened.initialize(artist);
      expect(reopened.selectedProvider, youtube.providerInstance);
    },
  );

  test('explicit provider takes precedence over saved preference', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      DetailProviderSelection.preferenceKey: <String>[
        'ytmusic--account',
        'ytmusic',
      ],
    });
    final selection = DetailProviderSelection(
      itemId: deezer.itemId,
      provider: deezer.providerInstance,
    );
    await selection.initialize(artist);
    expect(selection.selectedProvider, deezer.providerInstance);
  });

  test('missing preference falls back and library mapping is hidden', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      DetailProviderSelection.preferenceKey: <String>[
        'missing--account',
        'missing',
      ],
    });
    final selection = DetailProviderSelection(
      itemId: '78',
      provider: 'library',
    );
    await selection.initialize(
      const Artist(
        itemId: '78',
        provider: 'library',
        name: 'Artist',
        providerMappings: <ProviderMapping>[
          ProviderMapping(
            itemId: '78',
            providerDomain: 'library',
            providerInstance: 'library',
            available: true,
          ),
          deezer,
          youtube,
        ],
      ),
    );
    expect(selection.mappings, <ProviderMapping>[deezer, youtube]);
    expect(selection.selectedProvider, deezer.providerInstance);
  });
}
