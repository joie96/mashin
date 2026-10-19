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

  for (final width in <double>[320, 1280]) {
    testWidgets(
      'header switches sources at width $width without changing metadata',
      (tester) async {
        tester.view.physicalSize = Size(width, 800);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final selection = DetailProviderSelection(
          itemId: '78',
          provider: 'library',
        );
        await selection.initialize(artist);
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
        expect(find.byTooltip('deezer'), findsOneWidget);
        expect(find.byTooltip('ytmusic'), findsOneWidget);
        expect(find.byTooltip('library'), findsNothing);
        final Finder deezerButton = find.byKey(
          ValueKey<String>(deezer.providerInstance),
        );
        final Finder youtubeButton = find.byKey(
          ValueKey<String>(youtube.providerInstance),
        );
        final Size buttonSize = tester.getSize(deezerButton);
        expect(tester.getSize(youtubeButton), buttonSize);
        expect(
          tester
              .widget<MediaProviderIcon>(
                find.descendant(
                  of: deezerButton,
                  matching: find.byType(MediaProviderIcon),
                ),
              )
              .size,
          24,
        );
        expect(
          tester
              .widget<MediaProviderIcon>(
                find.descendant(
                  of: youtubeButton,
                  matching: find.byType(MediaProviderIcon),
                ),
              )
              .size,
          16,
        );
        for (final Finder buttonFinder in <Finder>[
          deezerButton,
          youtubeButton,
        ]) {
          final ButtonStyle style = tester
              .widget<IconButton>(buttonFinder)
              .style!;
          for (final states in <Set<WidgetState>>[
            <WidgetState>{},
            <WidgetState>{WidgetState.hovered},
            <WidgetState>{WidgetState.selected},
            <WidgetState>{WidgetState.disabled},
            <WidgetState>{WidgetState.focused},
            <WidgetState>{WidgetState.pressed},
          ]) {
            expect(style.shape!.resolve(states), isA<CircleBorder>());
            expect(style.side!.resolve(states), BorderSide.none);
            expect(style.backgroundColor!.resolve(states), Colors.transparent);
            expect(style.overlayColor!.resolve(states), Colors.transparent);
          }
        }
        final Finder youtubeScale = find.descendant(
          of: youtubeButton,
          matching: find.byType(AnimatedScale),
        );
        final TestGesture mouse = await tester.createGesture(
          kind: PointerDeviceKind.mouse,
        );
        await mouse.addPointer(location: const Offset(300, 700));
        await mouse.moveTo(tester.getCenter(deezerButton));
        await tester.pumpAndSettle();
        expect(
          tester
              .widget<AnimatedScale>(
                find.descendant(
                  of: deezerButton,
                  matching: find.byType(AnimatedScale),
                ),
              )
              .scale,
          1,
        );
        await mouse.moveTo(tester.getCenter(youtubeButton));
        await tester.pumpAndSettle();
        final AnimatedScale hoveredScale = tester.widget<AnimatedScale>(
          youtubeScale,
        );
        expect(hoveredScale.scale, 1.5);
        expect(hoveredScale.duration, const Duration(milliseconds: 140));
        expect(hoveredScale.curve, Curves.easeOutCubic);
        expect(tester.getSize(youtubeButton), buttonSize);
        await mouse.moveTo(const Offset(300, 700));
        await tester.pumpAndSettle();
        expect(tester.widget<AnimatedScale>(youtubeScale).scale, 1);
        await mouse.removePointer();
        await tester.tap(find.byTooltip('ytmusic'));
        await tester.pumpAndSettle();
        expect(selection.selectedProvider, youtube.providerInstance);
        expect(artist.provider, 'library');
        expect(tester.getSize(deezerButton), buttonSize);
        expect(tester.getSize(youtubeButton), buttonSize);
        expect(
          tester
              .widget<MediaProviderIcon>(
                find.descendant(
                  of: youtubeButton,
                  matching: find.byType(MediaProviderIcon),
                ),
              )
              .size,
          24,
        );
        expect(
          tester
              .widget<MediaProviderIcon>(
                find.descendant(
                  of: deezerButton,
                  matching: find.byType(MediaProviderIcon),
                ),
              )
              .size,
          16,
        );
        expect(
          tester.getTopLeft(find.byTooltip('ytmusic')).dx,
          lessThan(tester.getTopLeft(find.byTooltip('deezer')).dx),
        );
        expect(tester.takeException(), isNull);
      },
    );
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
