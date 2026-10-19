import 'package:flutter/material.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/navigation/app_router.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_events.dart';
import 'package:mashin_flutter/features/shared/item_provider_selection.dart';
import 'package:mashin_flutter/features/shared/widgets/detail_provider_switcher.dart';
import 'package:mashin_flutter/features/shared/widgets/media_table_section_desktop.dart';
import 'package:shared_preferences/shared_preferences.dart';

class _UnusedBridge extends Fake implements MashinCoreBridgeApi {}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(
    () => SharedPreferences.setMockInitialValues(<String, Object>{
      preferredProviderPreferenceKey: <String>['youtube--one', 'youtube'],
    }),
  );

  Track track() => Track.fromJson(<String, dynamic>{
    'item_id': '1',
    'provider': 'library',
    'name': 'Original',
    'media_type': 'track',
    'provider_mappings': <Object?>[
      <String, Object?>{
        'item_id': 'dz',
        'provider_instance': 'deezer--one',
        'provider_domain': 'deezer',
      },
      <String, Object?>{
        'item_id': 'yt',
        'provider_instance': 'youtube--one',
        'provider_domain': 'youtube',
      },
    ],
  });

  for (final size in <double>[16, 18]) {
    for (final primaryIconOffset in <Offset?>[null, Offset.zero]) {
      testWidgets(
        'main provider has an identical anchor for one to three mappings at size $size and offset $primaryIconOffset',
        (tester) async {
          Rect? originalBounds;
          Size? originalArea;
          for (final count in <int>[1, 2, 3]) {
            final mappings = <ProviderMapping>[
              const ProviderMapping(
                itemId: 'yt',
                providerDomain: 'youtube',
                providerInstance: 'youtube--one',
                available: true,
              ),
              if (count > 1)
                const ProviderMapping(
                  itemId: 'dz',
                  providerDomain: 'deezer',
                  providerInstance: 'deezer--one',
                  available: true,
                ),
              if (count > 2)
                const ProviderMapping(
                  itemId: 'third',
                  providerDomain: 'third',
                  providerInstance: 'third--one',
                  available: true,
                ),
            ];
            final item = MediaItem(
              itemId: '1',
              provider: 'library',
              name: 'Original',
              mediaType: MediaType.track,
              providerMappings: mappings,
            );
            await tester.pumpWidget(
              MaterialApp(
                theme: AppTheme.dark(),
                home: Scaffold(
                  body: Center(
                    child: SizedBox(
                      width: 80,
                      height: 64,
                      child: Center(
                        child: MediaItemProviderIcon(
                          item: item,
                          size: size,
                          primaryIconOffset: primaryIconOffset,
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            );
            await tester.pumpAndSettle();
            final primary = find.byKey(
              ValueKey<String>(
                count == 1
                    ? 'item-provider-primary'
                    : 'mobile-provider-youtube--one',
              ),
            );
            final Rect bounds = tester.getRect(primary);
            final Size area = tester.getSize(
              find.byType(MediaItemProviderIcon),
            );
            originalBounds ??= bounds;
            originalArea ??= area;
            expect(bounds, originalBounds);
            expect(bounds.width, size);
            expect(bounds.height, size);
            expect(area, originalArea);
            if (primaryIconOffset == null) {
              expect(
                bounds.center.dy,
                tester.getCenter(find.byType(MediaItemProviderIcon)).dy,
              );
            } else {
              expect(
                bounds.topLeft,
                tester.getTopLeft(find.byType(MediaItemProviderIcon)) +
                    primaryIconOffset,
              );
            }
            if (count > 1) {
              final rear = find.byKey(
                const ValueKey<String>('mobile-provider-deezer--one'),
              );
              expect(tester.getCenter(rear).dx, greaterThan(bounds.center.dx));
              expect(tester.getCenter(rear).dy, lessThan(bounds.center.dy));
            }
            const int mouseDevice = 1;
            final mouse = TestGesture(
              dispatcher: tester.sendEventToBinding,
              kind: PointerDeviceKind.mouse,
              device: mouseDevice,
            );
            await mouse.addPointer();
            await mouse.moveTo(bounds.center);
            await tester.pump();
            expect(
              tester.binding.mouseTracker.debugDeviceActiveCursor(mouseDevice),
              count > 1 ? SystemMouseCursors.click : SystemMouseCursors.basic,
            );
            await mouse.removePointer();
            expect(tester.takeException(), isNull);
          }
        },
      );
    }
  }

  test(
    'opening uses the selected provider without changing the original item',
    () async {
      final mappings = track().providerMappings;
      final artist = Artist(
        itemId: '1',
        provider: 'library',
        name: 'Original',
        providerMappings: mappings,
      );
      final album = Album(
        itemId: '1',
        provider: 'library',
        name: 'Original',
        artists: <Artist>[artist],
        providerMappings: mappings,
      );
      final playlist = Playlist(
        itemId: '1',
        provider: 'library',
        name: 'Original',
        isEditable: false,
        providerMappings: mappings,
      );
      for (final item in <MediaItem>[artist, album, playlist]) {
        final selection = ItemProviderSelection.forItem(item);
        String route() => switch (item) {
          Artist() => AppRoutes.artist(item),
          Album() => AppRoutes.album(item),
          Playlist() => AppRoutes.playlist(item),
          _ => throw StateError('Unexpected test item'),
        };
        expect(route(), '/${mediaTypeToJson(item.mediaType)}/library/1');
        await selection.initialize();
        expect(route(), '/${mediaTypeToJson(item.mediaType)}/youtube--one/yt');
        await selection.cycle();
        expect(route(), '/${mediaTypeToJson(item.mediaType)}/deezer--one/dz');
        expect(item.provider, 'library');
        expect(item.itemId, '1');
        expect(item.name, 'Original');
      }
    },
  );

  test(
    'cycling is object-local and reload reapplies the global preference',
    () async {
      final item = track();
      final selection = ItemProviderSelection.forItem(item);
      await selection.initialize();
      expect(selection.mappings.first.providerInstance, 'youtube--one');
      await selection.cycle();
      expect(selection.mappings.first.providerInstance, 'deezer--one');
      expect(identical(selection, ItemProviderSelection.forItem(item)), isTrue);
      final reloaded = ItemProviderSelection.forItem(track());
      await reloaded.initialize();
      expect(reloaded.mappings.first.providerInstance, 'youtube--one');
      expect(
        (await SharedPreferences.getInstance()).getStringList(
          preferredProviderPreferenceKey,
        ),
        <String>['youtube--one', 'youtube'],
      );
      expect(item.provider, 'library');
      expect(item.name, 'Original');
    },
  );

  test(
    'provider actions use the first selected mapping without changing library identity',
    () async {
      final item = track();
      expect(
        (await createSelectedProviderMediaItemPayload(item))['provider'],
        'youtube--one',
      );
      await ItemProviderSelection.forItem(item).cycle();
      final payload = await createSelectedProviderMediaItemPayload(item);
      expect(payload['provider'], 'deezer--one');
      expect(payload['item_id'], 'dz');
      expect(payload['uri'], 'deezer--one://track/dz');
      expect(
        (payload['provider_mappings'] as List).first['provider_instance'],
        'deezer--one',
      );
      expect(item.provider, 'library');
    },
  );

  testWidgets('provider tap does not activate or select the containing item', (
    tester,
  ) async {
    final item = track();
    int taps = 0;
    int selections = 0;
    int doubleTaps = 0;
    int longPresses = 0;
    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: Center(
            child: Listener(
              onPointerDown: (event) {
                if (!MediaItemProviderIcon.handlesPointer(event)) selections++;
              },
              child: GestureDetector(
                onTap: () => taps++,
                onDoubleTap: () => doubleTaps++,
                onLongPress: () => longPresses++,
                child: SizedBox(
                  width: 200,
                  height: 60,
                  child: Row(
                    children: <Widget>[
                      MediaItemProviderIcon(item: item),
                      const Expanded(child: Text('Original')),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.byTooltip('Zu deezer wechseln'), findsOneWidget);
    await tester.tap(find.byType(IconButton));
    await tester.pump(kDoubleTapTimeout);
    await tester.pumpAndSettle();
    expect(
      ItemProviderSelection.forItem(item).selected?.providerInstance,
      'deezer--one',
    );
    await tester.longPress(find.byType(IconButton));
    await tester.pumpAndSettle();
    expect(taps, 0);
    expect(selections, 0);
    expect(doubleTaps, 0);
    expect(longPresses, 0);
    await tester.tap(find.text('Original'));
    await tester.pump(kDoubleTapTimeout);
    await tester.pumpAndSettle();
    expect(taps, 1);
    expect(selections, 1);
  });

  testWidgets(
    'actual table provider button does not select or activate its row',
    (tester) async {
      final item = track();
      final bridge = _UnusedBridge();
      final events = MashinCoreBridgeEvents(coreBridge: bridge);
      addTearDown(events.dispose);
      int selections = 0;
      int activations = 0;
      tester.view.physicalSize = const Size(1280, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.dark(),
          home: Scaffold(
            body: SingleChildScrollView(
              child: MediaTableSectionDesktop<Track>(
                title: 'Results',
                items: <Track>[item],
                isLoading: false,
                emptyText: 'Empty',
                coreBridge: bridge,
                coreEvents: events,
                onSelectionChanged: (_) => selections++,
                onRowDoubleTap: (_, _) => activations++,
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final provider = find.byTooltip('Zu deezer wechseln');
      expect(provider, findsOneWidget);
      await tester.tap(provider);
      await tester.pump(kDoubleTapTimeout);
      await tester.pumpAndSettle();
      expect(
        ItemProviderSelection.forItem(item).selected?.providerInstance,
        'deezer--one',
      );
      expect(selections, 0);
      expect(activations, 0);
      final button = find.descendant(
        of: find.byType(MediaItemProviderIcon),
        matching: find.byType(IconButton),
      );
      await tester.tap(button);
      await tester.pump(const Duration(milliseconds: 50));
      await tester.tap(button);
      await tester.pump(kDoubleTapTimeout);
      await tester.pumpAndSettle();
      expect(selections, 0);
      expect(activations, 0);
      expect(tester.takeException(), isNull);
    },
  );

  test(
    'unavailable providers are skipped and missing preference falls back',
    () async {
      SharedPreferences.setMockInitialValues(<String, Object>{
        preferredProviderPreferenceKey: <String>['missing', 'missing'],
      });
      final item = MediaItem(
        itemId: '1',
        provider: 'library',
        name: 'Artist',
        mediaType: MediaType.artist,
        providerMappings: <ProviderMapping>[
          const ProviderMapping(
            itemId: 'offline',
            providerDomain: 'offline',
            providerInstance: 'offline',
            available: false,
          ),
          ...track().providerMappings,
        ],
      );
      final selection = ItemProviderSelection.forItem(item);
      await selection.initialize();
      expect(selection.mappings.first.providerInstance, 'deezer--one');
      await selection.cycle();
      expect(selection.mappings.first.providerInstance, 'youtube--one');
      await selection.cycle();
      expect(selection.mappings.first.providerInstance, 'deezer--one');
    },
  );
}
