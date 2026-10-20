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

class _PlaybackQueryBridge extends Fake implements MashinCoreBridgeApi {
  final queries = <(String, String, String)>[];
  List<Track> tracks = <Track>[];

  @override
  Future<List<Track>> getAlbumTracks(String itemId, String provider) async {
    queries.add(('album', itemId, provider));
    return tracks;
  }

  @override
  Future<List<Track>> getArtistTopTracks(String itemId, String provider) async {
    queries.add(('artist', itemId, provider));
    return tracks;
  }
}

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

  test(
    'album and artist playback expands the selected provider into tracks',
    () async {
      final source = track();
      final bridge = _PlaybackQueryBridge()..tracks = <Track>[source];
      for (final item in <MediaItem>[
        Album(
          itemId: 'album',
          provider: 'library',
          name: 'Album',
          artists: const <Artist>[],
          providerMappings: source.providerMappings,
        ),
        Artist(
          itemId: 'artist',
          provider: 'library',
          name: 'Artist',
          providerMappings: source.providerMappings,
        ),
      ]) {
        final selection = ItemProviderSelection.forItem(item);
        await selection.initialize();
        await selection.cycle();
        final resolved = await resolveQueuePlayMedia(bridge, <MediaItem>[item]);
        final payloads = resolved.mediaItems;

        expect(bridge.queries.last, (
          item is Album ? 'album' : 'artist',
          'dz',
          'deezer--one',
        ));
        expect(payloads.single['media_type'], 'track');
        expect(payloads.single['provider'], 'deezer--one');
        expect(payloads.single['item_id'], 'dz');
        expect(payloads.single['uri'], 'deezer--one://track/dz');
        expect(source.provider, 'library');
        expect(
          (await createSelectedProviderMediaItemPayload(source))['provider'],
          'youtube--one',
        );
      }
    },
  );

  test(
    'detail track playback uses the explicit provider without changing selection',
    () async {
      final item = track();
      final resolved = await resolveQueuePlayMedia(
        _PlaybackQueryBridge(),
        <MediaItem>[item],
        providerInstance: 'deezer--one',
      );

      expect(resolved.mediaItems.single['provider'], 'deezer--one');
      expect(resolved.mediaItems.single['item_id'], 'dz');
      expect(
        (await createSelectedProviderMediaItemPayload(item))['provider'],
        'youtube--one',
      );
    },
  );

  test(
    'playback rejects a missing selected provider instead of falling back',
    () async {
      await expectLater(
        resolveQueuePlayMedia(_PlaybackQueryBridge(), <MediaItem>[
          track(),
        ], providerInstance: 'missing--one'),
        throwsStateError,
      );
    },
  );

  test('empty album expansion does not send the album object', () async {
    final resolved =
        await resolveQueuePlayMedia(_PlaybackQueryBridge(), <MediaItem>[
          Album(
            itemId: 'album',
            provider: 'deezer--one',
            name: 'Empty',
            artists: const <Artist>[],
          ),
        ]);

    expect(resolved.mediaItems, isEmpty);
  });

  test(
    'mixed playback preserves track order while expanding album and artist',
    () async {
      final bridge = _PlaybackQueryBridge()
        ..tracks = <Track>[
          Track(
            itemId: 'first',
            provider: 'youtube--one',
            name: 'First',
            duration: 120,
            artists: const <Artist>[],
          ),
          Track(
            itemId: 'second',
            provider: 'youtube--one',
            name: 'Second',
            duration: 120,
            artists: const <Artist>[],
          ),
        ];
      final resolved = await resolveQueuePlayMedia(bridge, <MediaItem>[
        track(),
        Album(
          itemId: 'album',
          provider: 'youtube--one',
          name: 'Album',
          artists: const <Artist>[],
        ),
        Artist(itemId: 'artist', provider: 'youtube--one', name: 'Artist'),
      ], startItem: 2);
      final payloads = resolved.mediaItems;

      expect(payloads.map((payload) => payload['item_id']), <String>[
        'yt',
        'first',
        'second',
        'first',
        'second',
      ]);
      expect(resolved.startItem, 3);
      expect(
        payloads.map((payload) => payload['media_type']),
        everyElement('track'),
      );
      expect(bridge.queries, <(String, String, String)>[
        ('album', 'album', 'youtube--one'),
        ('artist', 'artist', 'youtube--one'),
      ]);
    },
  );

  test('album expansion skips unplayable tracks', () async {
    final bridge = _PlaybackQueryBridge()
      ..tracks = <Track>[
        Track(
          itemId: 'unavailable',
          provider: 'youtube--one',
          name: 'Unavailable',
          duration: 120,
          isPlayable: false,
          artists: const <Artist>[],
        ),
      ];

    final resolved = await resolveQueuePlayMedia(bridge, <MediaItem>[
      Album(
        itemId: 'album',
        provider: 'youtube--one',
        name: 'Album',
        artists: const <Artist>[],
      ),
    ]);
    expect(resolved.mediaItems, isEmpty);
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
                bounds.center,
                tester.getCenter(find.byType(MediaItemProviderIcon)),
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
      final primary = find.byKey(
        const ValueKey<String>('mobile-provider-youtube--one'),
      );
      final primaryBounds = tester.getRect(primary);
      final providerCell = find
          .ancestor(
            of: find.byType(MediaItemProviderIcon),
            matching: find.byType(Row),
          )
          .first;
      expect(primaryBounds.center.dx, tester.getCenter(providerCell).dx);
      await tester.tap(provider);
      await tester.pump(kDoubleTapTimeout);
      await tester.pumpAndSettle();
      expect(
        ItemProviderSelection.forItem(item).selected?.providerInstance,
        'deezer--one',
      );
      expect(
        tester.getRect(
          find.byKey(const ValueKey<String>('mobile-provider-deezer--one')),
        ),
        primaryBounds,
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
