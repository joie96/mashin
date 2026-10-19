import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_events.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/core/core_userdata.dart';
import 'package:mashin_flutter/features/playlist/playlist_detail_controller.dart';
import 'package:mashin_flutter/features/playlist/playlist_detail_page_desktop.dart';
import 'package:mashin_flutter/features/playlist/playlist_detail_page_mobile.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  for (final isDesktop in <bool>[true, false]) {
    testWidgets(
      '${isDesktop ? 'desktop' : 'mobile'} external playlist header switches tracks',
      (tester) async {
        SharedPreferences.setMockInitialValues(<String, Object>{});
        tester.view.physicalSize = Size(isDesktop ? 1280 : 320, 900);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final bridge = _ProviderPlaylistCoreBridge();
        final events = MashinCoreBridgeEvents(coreBridge: bridge);
        addTearDown(events.dispose);
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark(),
            home: Scaffold(
              body: isDesktop
                  ? PlaylistDetailPageDesktop(
                      coreBridge: bridge,
                      coreEvents: events,
                      itemId: 'playlist',
                      providerInstanceIdOrDomain: 'library',
                    )
                  : PlaylistDetailPageMobile(
                      coreBridge: bridge,
                      coreEvents: events,
                      itemId: 'playlist',
                      providerInstanceIdOrDomain: 'library',
                    ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await tester.tap(
            find.byKey(ValueKey<String>(isDesktop ? 'ytmusic--one' : 'detail-provider-cycle')),
        );
        await tester.pumpAndSettle();
        expect(bridge.queries, contains('tracks:yt-playlist:ytmusic--one'));
        expect(
          bridge.queries.where((query) => query.startsWith('metadata:')),
          hasLength(1),
        );
        expect(tester.takeException(), isNull);
      },
    );
  }

  test(
    'external playlist switches tracks without replacing library metadata',
    () async {
      SharedPreferences.setMockInitialValues(<String, Object>{});
      final bridge = _ProviderPlaylistCoreBridge();
      final controller = PlaylistDetailController(
        coreBridge: bridge,
        itemId: 'playlist',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(controller.dispose);
      await controller.loadPlaylist();
      final Playlist? original = controller.playlist;
      expect(bridge.queries, contains('tracks:dz-playlist:deezer--one'));
      bridge.queries.clear();
      await controller.selectProvider(controller.sectionProvider.mappings.last);
      expect(identical(controller.playlist, original), isTrue);
      expect(bridge.queries, <String>['tracks:yt-playlist:ytmusic--one']);
    },
  );

  for (final isDesktop in <bool>[true, false]) {
    testWidgets(
      '${isDesktop ? 'desktop' : 'mobile'} playlist menu refreshes its cover',
      (tester) async {
        tester.view.physicalSize = isDesktop
            ? const Size(1280, 800)
            : const Size(390, 844);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final bridge = _PrivatePlaylistCoreBridge();
        final userData = UserData(coreBridge: bridge);
        final coreEvents = MashinCoreBridgeEvents(coreBridge: bridge);
        addTearDown(bridge.changes.close);
        addTearDown(userData.dispose);
        addTearDown(coreEvents.dispose);
        await userData.loadLibraryData();
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark(),
            home: Scaffold(
              body: isDesktop
                  ? PlaylistDetailPageDesktop(
                      coreBridge: bridge,
                      coreEvents: coreEvents,
                      itemId: 'playlist-1',
                      providerInstanceIdOrDomain: 'library',
                      userDataController: userData,
                    )
                  : PlaylistDetailPageMobile(
                      coreBridge: bridge,
                      coreEvents: coreEvents,
                      itemId: 'playlist-1',
                      providerInstanceIdOrDomain: 'library',
                      userDataController: userData,
                    ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await tester.tap(
          isDesktop
              ? find.byTooltip('Weitere Optionen')
              : find.byIcon(Icons.more_vert_outlined).first,
        );
        await tester.pumpAndSettle();
        await tester.tap(find.text('Cover aktualisieren'));
        await tester.pumpAndSettle();
        expect(bridge.operations, <String>[
          'refreshPrivatePlaylistCover:playlist-1',
        ]);
        expect(tester.takeException(), isNull);
      },
    );
  }

  test(
    'cover refresh applies the confirmed snapshot and preserves the old cover on failure',
    () async {
      final bridge = _PrivatePlaylistCoreBridge()
        ..coverPath = 'old-cover.jpg'
        ..refreshedCoverPath = 'new-cover.jpg';
      final userData = UserData(coreBridge: bridge);
      addTearDown(bridge.changes.close);
      addTearDown(userData.dispose);
      await userData.loadLibraryData();
      final controller = PlaylistDetailController(
        coreBridge: bridge,
        itemId: 'playlist-1',
        providerInstanceIdOrDomain: 'library',
        userDataController: userData,
      );
      addTearDown(controller.dispose);
      await controller.loadPlaylist();
      expect(controller.playlist!.primaryImage!.path, 'old-cover.jpg');
      await userData.refreshPrivatePlaylistCover(userData.playlists.single);
      expect(controller.playlist!.primaryImage!.path, 'new-cover.jpg');
      bridge.shouldFailCoverRefresh = true;
      bridge.refreshedCoverPath = 'unconfirmed-cover.jpg';
      await expectLater(
        userData.refreshPrivatePlaylistCover(userData.playlists.single),
        throwsStateError,
      );
      expect(controller.playlist!.primaryImage!.path, 'new-cover.jpg');
      expect(controller.tracks, hasLength(1));
    },
  );

  for (final isDesktop in <bool>[true, false]) {
    for (final isExternalDeletion in <bool>[true, false]) {
      testWidgets(
        '${isDesktop ? 'desktop' : 'mobile'} navigates back once after ${isExternalDeletion ? 'external' : 'local'} playlist deletion',
        (tester) async {
          tester.view.physicalSize = isDesktop
              ? const Size(1280, 800)
              : const Size(390, 844);
          tester.view.devicePixelRatio = 1;
          addTearDown(tester.view.resetPhysicalSize);
          addTearDown(tester.view.resetDevicePixelRatio);
          final bridge = _PrivatePlaylistCoreBridge();
          final userData = UserData(coreBridge: bridge);
          final coreEvents = MashinCoreBridgeEvents(coreBridge: bridge);
          addTearDown(bridge.changes.close);
          addTearDown(userData.dispose);
          addTearDown(coreEvents.dispose);
          await userData.loadLibraryData();
          var navigationCount = 0;
          await tester.pumpWidget(
            MaterialApp(
              theme: AppTheme.dark(),
              home: Builder(
                builder: (context) => Scaffold(
                  body: TextButton(
                    onPressed: () => Navigator.of(context).push<void>(
                      MaterialPageRoute(
                        builder: (detailContext) {
                          void onDeleted() {
                            navigationCount++;
                            Navigator.of(detailContext).pop();
                          }

                          return Scaffold(
                            body: isDesktop
                                ? PlaylistDetailPageDesktop(
                                    coreBridge: bridge,
                                    coreEvents: coreEvents,
                                    itemId: 'playlist-1',
                                    providerInstanceIdOrDomain: 'library',
                                    userDataController: userData,
                                    onPlaylistDeleted: onDeleted,
                                  )
                                : PlaylistDetailPageMobile(
                                    coreBridge: bridge,
                                    coreEvents: coreEvents,
                                    itemId: 'playlist-1',
                                    providerInstanceIdOrDomain: 'library',
                                    userDataController: userData,
                                    onPlaylistDeleted: onDeleted,
                                  ),
                          );
                        },
                      ),
                    ),
                    child: const Text('Playlist overview'),
                  ),
                ),
              ),
            ),
          );
          await tester.tap(find.text('Playlist overview'));
          await tester.pumpAndSettle();
          expect(find.text('Private'), findsOneWidget);
          bridge.trackCount = 0;
          bridge.changes.add(<String, dynamic>{'type': 'user_data_changed'});
          await tester.pumpAndSettle();
          expect(navigationCount, 0);
          expect(find.text('Private'), findsOneWidget);
          if (isExternalDeletion) {
            bridge.hasPlaylist = false;
            bridge.changes.add(<String, dynamic>{'type': 'user_data_changed'});
          } else {
            await userData.removePlaylist(userData.playlists.single);
          }
          await tester.pumpAndSettle();
          expect(find.text('Playlist overview'), findsOneWidget);
          expect(find.text('Private'), findsNothing);
          bridge.changes.add(<String, dynamic>{'type': 'user_data_changed'});
          await tester.pumpAndSettle();
          expect(navigationCount, 1);
          expect(tester.takeException(), isNull);
        },
      );
    }
  }

  test(
    'updates an open private playlist when a delayed 50-track add completes',
    () async {
      final bridge = _PrivatePlaylistCoreBridge();
      final userData = UserData(coreBridge: bridge);
      addTearDown(userData.dispose);
      addTearDown(bridge.changes.close);
      await userData.loadLibraryData();
      final addition = userData.addPlaylistTracks(
        'playlist-1',
        List<Track>.generate(
          50,
          (index) => _track('new-$index', uri: 'test://track/$index'),
        ),
      );
      final controller = PlaylistDetailController(
        coreBridge: bridge,
        itemId: 'playlist-1',
        providerInstanceIdOrDomain: 'library',
        userDataController: userData,
      );
      addTearDown(controller.dispose);
      await controller.loadPlaylist();
      expect(controller.tracks, hasLength(1));
      bridge.trackCount = 51;
      bridge.changes.add(<String, dynamic>{'type': 'user_data_changed'});
      await Future<void>.delayed(Duration.zero);
      expect(controller.tracks, hasLength(51));
      bridge.addition.complete();
      await addition;
      expect(controller.tracks, hasLength(51));
      expect(bridge.operations, isEmpty);
    },
  );

  test(
    'coalesces events and rejects an invalidated snapshot response',
    () async {
      final bridge = _PrivatePlaylistCoreBridge();
      final userData = UserData(coreBridge: bridge);
      addTearDown(bridge.changes.close);
      addTearDown(userData.dispose);
      await userData.loadLibraryData();
      final appliedTrackCounts = <int>[];
      userData.addListener(() {
        appliedTrackCounts.add(userData.tracksForPlaylist('playlist-1').length);
      });
      final oldSnapshot = bridge.snapshot;
      bridge.deferSnapshots = true;
      bridge.changes.add(<String, dynamic>{'type': 'user_data_changed'});
      await Future<void>.delayed(Duration.zero);
      bridge.trackCount = 51;
      for (var index = 0; index < 20; index++) {
        bridge.changes.add(<String, dynamic>{'type': 'user_data_changed'});
      }
      await Future<void>.delayed(Duration.zero);
      expect(bridge.snapshotReads, hasLength(1));
      bridge.snapshotReads.first.complete(oldSnapshot);
      await Future<void>.delayed(Duration.zero);
      expect(appliedTrackCounts, isEmpty);
      expect(bridge.snapshotReads, hasLength(2));
      bridge.snapshotReads.last.complete(bridge.snapshot);
      await Future<void>.delayed(Duration.zero);
      expect(appliedTrackCounts, <int>[51]);
    },
  );

  test('disposal cancels events and ignores an in-flight snapshot', () async {
    final bridge = _PrivatePlaylistCoreBridge();
    final userData = UserData(coreBridge: bridge);
    addTearDown(bridge.changes.close);
    await userData.loadLibraryData();
    bridge.deferSnapshots = true;
    bridge.changes.add(<String, dynamic>{'type': 'user_data_changed'});
    await Future<void>.delayed(Duration.zero);
    expect(bridge.changes.hasListener, isTrue);
    var notifications = 0;
    userData.addListener(() => notifications++);
    userData.dispose();
    expect(bridge.changes.hasListener, isFalse);
    bridge.snapshotReads.single.complete(bridge.snapshot);
    await Future<void>.delayed(Duration.zero);
    expect(notifications, 0);
  });

  test('shows playlist tracks before their album details arrive', () async {
    final coreBridge = _DeferredAlbumPlaylistCoreBridge();
    final controller = PlaylistDetailController(
      coreBridge: coreBridge,
      itemId: 'playlist-1',
      providerInstanceIdOrDomain: 'library',
    );
    addTearDown(controller.dispose);

    await controller.loadPlaylist();
    expect(controller.isLoadingTracks, isFalse);
    expect(controller.tracks.single.album?.year, isNull);
    expect(coreBridge.requestedArtists, <String>['artist-1:library']);

    coreBridge.albums.complete(<Album>[
      Album(
        itemId: 'album-1',
        provider: 'library',
        name: 'Complete album',
        artists: const <Artist>[],
        year: 2014,
      ),
    ]);
    await Future<void>.delayed(Duration.zero);
    expect(controller.tracks.single.album?.year, 2014);
  });

  test(
    'loads and edits an online playlist through core bridge methods',
    () async {
      final coreBridge = _FakePlaylistCoreBridge();
      final controller = PlaylistDetailController(
        coreBridge: coreBridge,
        itemId: 'playlist-1',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(controller.dispose);

      await controller.loadPlaylist();
      await controller.addTracksToPlaylist('playlist-1', <MediaItem>[
        _track('track-2', uri: 'ma://track/2'),
      ]);
      await controller.removeTrack(controller.tracks.single);

      expect(controller.playlist?.name, 'Online Playlist');
      expect(controller.tracks, isEmpty);
      expect(coreBridge.operations, <String>[
        'getPlaylist:playlist-1:library',
        'getPlaylistTracks:playlist-1:library:true:false',
        'addPlaylistTracks:playlist-1:ma://track/2',
        'removePlaylistTracks:playlist-1:1',
      ]);
    },
  );

  testWidgets('desktop playlist scrolls its only header with the tracks', (
    WidgetTester tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final coreBridge = _FakePlaylistCoreBridge(trackCount: 30);
    final coreEvents = MashinCoreBridgeEvents(coreBridge: coreBridge);
    addTearDown(coreEvents.dispose);

    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: PlaylistDetailPageDesktop(
            coreBridge: coreBridge,
            coreEvents: coreEvents,
            itemId: 'playlist-1',
            providerInstanceIdOrDomain: 'library',
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Online Playlist'), findsOneWidget);
    expect(find.byTooltip('Header einklappen'), findsNothing);
    expect(find.byTooltip('Header ausklappen'), findsNothing);
    expect(find.text('track-29'), findsNothing);

    await tester.drag(find.byType(CustomScrollView), const Offset(0, -2200));
    await tester.pumpAndSettle();

    expect(find.text('Online Playlist'), findsNothing);
    expect(find.text('track-29'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}

class _FakePlaylistCoreBridge extends Fake implements MashinCoreBridgeApi {
  _FakePlaylistCoreBridge({this.trackCount = 1});

  final int trackCount;
  final List<String> operations = <String>[];

  @override
  Future<Playlist> getPlaylist(String itemId, String provider) async {
    operations.add('getPlaylist:$itemId:$provider');
    return Playlist(
      itemId: itemId,
      provider: provider,
      name: 'Online Playlist',
      isEditable: true,
    );
  }

  @override
  Future<List<Track>> getPlaylistTracks(
    String itemId,
    String provider, {
    bool forceRefresh = false,
    bool allowDynamicTracks = false,
  }) async {
    operations.add(
      'getPlaylistTracks:$itemId:$provider:$forceRefresh:$allowDynamicTracks',
    );
    return List<Track>.generate(
      trackCount,
      (int index) =>
          _track('track-${index + 1}', uri: 'ma://track/${index + 1}'),
    );
  }

  @override
  Future<void> addPlaylistTracks(String playlistId, List<String> uris) async {
    operations.add('addPlaylistTracks:$playlistId:${uris.join(',')}');
  }

  @override
  Future<void> removePlaylistTracks(
    String playlistId,
    List<int> positionsToRemove,
  ) async {
    operations.add(
      'removePlaylistTracks:$playlistId:${positionsToRemove.join(',')}',
    );
  }
}

class _ProviderPlaylistCoreBridge extends Fake implements MashinCoreBridgeApi {
  final List<String> queries = <String>[];

  @override
  Future<Playlist> getPlaylist(String itemId, String provider) async {
    queries.add('metadata:$itemId:$provider');
    return const Playlist(
      itemId: 'playlist',
      provider: 'library',
      name: 'Original playlist',
      isEditable: false,
      providerMappings: <ProviderMapping>[
        ProviderMapping(
          itemId: 'dz-playlist',
          providerDomain: 'deezer',
          providerInstance: 'deezer--one',
          available: true,
        ),
        ProviderMapping(
          itemId: 'yt-playlist',
          providerDomain: 'ytmusic',
          providerInstance: 'ytmusic--one',
          available: true,
        ),
      ],
    );
  }

  @override
  Future<List<Track>> getPlaylistTracks(
    String itemId,
    String provider, {
    bool forceRefresh = false,
    bool allowDynamicTracks = false,
  }) async {
    queries.add('tracks:$itemId:$provider');
    return const <Track>[];
  }
}

class _PrivatePlaylistCoreBridge extends _FakePlaylistCoreBridge {
  final changes = StreamController<Map<String, dynamic>>.broadcast();
  final addition = Completer<void>();
  final snapshotReads = <Completer<Map<String, dynamic>>>[];
  bool deferSnapshots = false;
  bool hasPlaylist = true;
  String? coverPath;
  String? refreshedCoverPath;
  bool shouldFailCoverRefresh = false;
  @override
  int trackCount = 1;

  @override
  Stream<Map<String, dynamic>> get events => changes.stream;

  @override
  Future<Map<String, dynamic>> loadUserData() => getUserData();

  @override
  Future<Map<String, dynamic>> getUserData() {
    if (!deferSnapshots) return Future.value(snapshot);
    final read = Completer<Map<String, dynamic>>();
    snapshotReads.add(read);
    return read.future;
  }

  Map<String, dynamic> get snapshot => <String, dynamic>{
    'favorites': <String, dynamic>{},
    'playlists': <Object?>[
      if (hasPlaylist)
        <String, dynamic>{
          'item_id': 'playlist-1',
          'provider': 'library',
          'name': 'Private',
          'is_editable': true,
          if (coverPath != null)
            'metadata': <String, dynamic>{
              'images': <Object?>[
                <String, dynamic>{
                  'type': 'thumb',
                  'path': coverPath,
                  'provider': 'builtin',
                },
              ],
            },
          'items': List.generate(
            trackCount,
            (index) => <String, dynamic>{
              'item_id': 'track-$index',
              'provider': 'library',
              'name': 'Track $index',
              'duration': 180,
              'artists': <Object?>[],
            },
          ),
        },
    ],
  };

  @override
  Future<void> refreshPrivatePlaylistCover(String playlistId) async {
    operations.add('refreshPrivatePlaylistCover:$playlistId');
    if (shouldFailCoverRefresh) throw StateError('Cover update failed');
    coverPath = refreshedCoverPath;
  }

  @override
  Future<void> removePrivatePlaylist(String playlistId) async {
    hasPlaylist = false;
    changes.add(<String, dynamic>{'type': 'user_data_changed'});
  }

  @override
  Future<void> addPrivatePlaylistTracks(
    String playlistId,
    List<Track> tracks,
  ) => addition.future;
}

class _DeferredAlbumPlaylistCoreBridge extends _FakePlaylistCoreBridge {
  final Completer<List<Album>> albums = Completer<List<Album>>();
  final List<String> requestedArtists = <String>[];

  @override
  Future<List<Track>> getPlaylistTracks(
    String itemId,
    String provider, {
    bool forceRefresh = false,
    bool allowDynamicTracks = false,
  }) async => <Track>[
    Track(
      itemId: 'track-1',
      provider: 'library',
      name: 'Track',
      duration: 120,
      artists: <Artist>[
        Artist.fromJson(<String, dynamic>{
          'item_id': 'artist-1',
          'provider': 'library',
          'name': 'Artist',
          'image': null,
        }),
      ],
      album: Album.fromJson(<String, dynamic>{
        'item_id': 'album-1',
        'provider': 'library',
        'name': 'Incomplete album',
        'image': null,
      }),
    ),
  ];

  @override
  Future<List<Album>> getArtistAlbums(String itemId, String provider) {
    requestedArtists.add('$itemId:$provider');
    return albums.future;
  }
}

Track _track(String itemId, {required String uri}) => Track(
  itemId: itemId,
  provider: 'library',
  name: itemId,
  duration: 180,
  uri: uri,
  artists: const <Artist>[],
);
