import 'dart:async';

import 'package:flutter/material.dart' hide RepeatMode;
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_events.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/playerbar/playback_seek_slider.dart';
import 'package:mashin/features/playerbar/playerbar_overlay_mobile.dart';
import 'package:mashin/features/shared/widgets/media_image.dart';
import 'package:mashin/features/shared/widgets/overlay_lifecycle.dart';
import 'package:mashin/features/shared/widgets/media_table_section_desktop.dart';

void main() {
  test(
    'position updates notify progress without rebuilding presentation or identity',
    () async {
      final bridge = _PlaybackIndicatorBridge();
      final events = MashinCoreBridgeEvents(coreBridge: bridge);
      addTearDown(() async {
        events.dispose();
        await bridge.queues.close();
        await bridge.players.close();
      });
      await events.start();
      var presentationChanges = 0;
      var progressChanges = 0;
      var queueChanges = 0;
      var mediaChanges = 0;
      events.addListener(() => presentationChanges++);
      events.playbackProgressChanges.addListener(() => progressChanges++);
      events.queueChanges.addListener(() => queueChanges++);
      events.currentMediaChanges.addListener(() => mediaChanges++);
      for (final position in [250, 500, 750]) {
        bridge.players.add(
          PlayerState(
            playbackState: PlayState.playing,
            positionMillis: position,
            bufferedPositionMillis: position + 1000,
          ),
        );
      }
      expect(
        [presentationChanges, progressChanges, queueChanges, mediaChanges],
        [0, 3, 0, 0],
      );
      expect(events.mediaSessionState.playerState.positionMillis, 750);
      bridge.players.add(
        const PlayerState(
          playbackState: PlayState.paused,
          positionMillis: 750,
          bufferedPositionMillis: 1750,
        ),
      );
      expect(
        [presentationChanges, progressChanges, queueChanges, mediaChanges],
        [1, 3, 0, 0],
      );
      await events.reset();
      expect(
        [presentationChanges, progressChanges, queueChanges, mediaChanges],
        [2, 4, 1, 1],
      );
      expect(events.mediaSessionState.displayedMedia, isNull);
    },
  );

  test(
    'redecoded requested media separates metadata, identity, queue and progress',
    () async {
      final bridge = _PlaybackIndicatorBridge();
      final events = MashinCoreBridgeEvents(coreBridge: bridge);
      addTearDown(() async {
        events.dispose();
        await bridge.queues.close();
        await bridge.players.close();
      });
      await events.start();
      var presentationChanges = 0;
      var mediaChanges = 0;
      var progressChanges = 0;
      var queueChanges = 0;
      events.addListener(() => presentationChanges++);
      events.currentMediaChanges.addListener(() => mediaChanges++);
      events.playbackProgressChanges.addListener(() => progressChanges++);
      events.queueChanges.addListener(() => queueChanges++);
      void publish({int? position, String name = 'Requested'}) =>
          bridge.players.add(
            PlayerState(
              playbackState: PlayState.playing,
              positionMillis: position,
              requestedMedia: Track.fromJson({
                'item_id': 'requested',
                'provider': 'library',
                'name': name,
              }),
            ),
          );
      publish();
      publish(position: 250);
      expect(
        [presentationChanges, mediaChanges, progressChanges, queueChanges],
        [1, 1, 1, 0],
      );
      publish(position: 250, name: 'Updated title');
      expect(
        [presentationChanges, mediaChanges, progressChanges, queueChanges],
        [2, 1, 1, 0],
      );
      await bridge.refreshQueue();
      expect(
        [presentationChanges, mediaChanges, progressChanges, queueChanges],
        [3, 1, 1, 1],
      );
      bridge.players.add(
        const PlayerState(
          playbackState: PlayState.playing,
          positionMillis: 250,
        ),
      );
      expect(
        [presentationChanges, mediaChanges, progressChanges, queueChanges],
        [4, 2, 1, 1],
      );
    },
  );

  testWidgets(
    'progress controls rebuild without rebuilding their presentation sibling',
    (tester) async {
      final bridge = _PlaybackIndicatorBridge();
      final events = MashinCoreBridgeEvents(coreBridge: bridge);
      addTearDown(() async {
        events.dispose();
        await bridge.queues.close();
        await bridge.players.close();
      });
      await events.start();
      var presentationBuilds = 0;
      var progressBuilds = 0;
      var rowBuilds = 0;
      await tester.pumpWidget(
        MaterialApp(
          home: Column(
            children: [
              ListenableBuilder(
                listenable: events,
                builder: (context, child) {
                  presentationBuilds++;
                  return const Text('Track');
                },
              ),
              PlaybackProgressBuilder(
                coreEvents: events,
                builder: (context, state) {
                  progressBuilds++;
                  return Text('position:${state.positionMillis ?? 0}');
                },
              ),
              PlaybackAwareBuilder(
                coreEvents: events,
                item: bridge.oldTrack,
                builder: (context, isCurrent) {
                  rowBuilds++;
                  return Text(isCurrent ? 'Current row' : 'Other row');
                },
              ),
            ],
          ),
        ),
      );
      for (final position in [250, 500, 750]) {
        bridge.players.add(
          PlayerState(
            playbackState: PlayState.playing,
            positionMillis: position,
          ),
        );
      }
      await tester.pump();
      expect(presentationBuilds, 1);
      expect(progressBuilds, 2);
      expect(rowBuilds, 1);
      expect(find.text('position:750'), findsOneWidget);
    },
  );

  testWidgets(
    'closed overlays release listeners after animation and survive reopening',
    (tester) async {
      final changes = _OverlayChanges();
      addTearDown(changes.dispose);
      var isActive = false;
      var contentBuilds = 0;
      Widget host() => MaterialApp(
        home: OverlayLifecycle(
          isActive: isActive,
          builder: (context, hasEntered, onEnd) => AnimatedSlide(
            duration: const Duration(milliseconds: 260),
            offset: Offset(0, isActive && hasEntered ? 0 : 1),
            onEnd: onEnd,
            child: ListenableBuilder(
              listenable: changes,
              builder: (context, child) {
                contentBuilds++;
                return const Text('Overlay content');
              },
            ),
          ),
        ),
      );
      await tester.pumpWidget(host());
      expect(contentBuilds, 0);
      expect(changes.hasActiveListeners, isFalse);
      isActive = true;
      await tester.pumpWidget(host());
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));
      expect(changes.hasActiveListeners, isTrue);
      isActive = false;
      await tester.pumpWidget(host());
      await tester.pump(const Duration(milliseconds: 100));
      expect(find.text('Overlay content'), findsOneWidget);
      expect(changes.hasActiveListeners, isTrue);
      isActive = true;
      await tester.pumpWidget(host());
      await tester.pump(const Duration(milliseconds: 300));
      expect(changes.hasActiveListeners, isTrue);
      isActive = false;
      await tester.pumpWidget(host());
      await tester.pump(const Duration(milliseconds: 300));
      await tester.pump();
      expect(find.text('Overlay content'), findsNothing);
      expect(changes.hasActiveListeners, isFalse);
      final closedBuilds = contentBuilds;
      changes.notifyListeners();
      await tester.pump();
      expect(contentBuilds, closedBuilds);
      isActive = true;
      await tester.pumpWidget(host());
      isActive = false;
      await tester.pumpWidget(host());
      await tester.pump();
      expect(changes.hasActiveListeners, isFalse);
      expect(find.text('Overlay content'), findsNothing);
    },
  );

  testWidgets(
    'player cover sources are lazy and survive transport-only rebuilds',
    (tester) async {
      tester.view.physicalSize = const Size(800, 1200);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final bridge = _PlaybackIndicatorBridge();
      final events = MashinCoreBridgeEvents(coreBridge: bridge);
      final sourceRequests = <String>[];
      void configureLoader() {
        MediaImage.configureImageLoader(
          (request, refresh) async => ImageReadSnapshot(bytes: null),
        );
        final createSource = MediaItemImage.cachedImageSource!;
        MediaItemImage.cachedImageSource = (request) {
          sourceRequests.add(request.image.path);
          return createSource(request);
        };
      }

      configureLoader();
      addTearDown(() async {
        events.dispose();
        MediaImage.configureImageLoader(null);
        await bridge.queues.close();
        await bridge.players.close();
      });
      Track track(int index, {String? path}) => Track.fromJson({
        'item_id': '$index',
        'provider': 'library',
        'name': 'Track $index',
        'metadata': {
          'images': [
            {
              'type': 'thumb',
              'path': path ?? 'cover-$index',
              'provider': 'builtin',
              'remotely_accessible': false,
            },
          ],
        },
      });
      var queue = Queue(
        currentIndex: 0,
        items: [
          for (var index = 0; index < 45; index++)
            QueueItem(queueItemId: '$index', mediaItem: track(index)),
        ],
      );
      var state = PlayState.playing;
      Future<void> command() async {}
      Widget host() => MaterialApp(
        theme: AppTheme.light(),
        home: Scaffold(
          body: PlayerBarOverlayMobile(
            coreEvents: events,
            isOpen: true,
            openProgress: 0,
            trackTitle: 'Track',
            trackSubtitle: 'Artist',
            trackCoverUrl: null,
            currentTrack: null,
            playbackStateType: state,
            playbackQueue: queue,
            maxSeconds: 180,
            dontStopTheMusicEnabled: false,
            userDataController: null,
            onClose: () {},
            onAnimationEnd: () {},
            onOpenQueue: () {},
            onQueueDragUpdate: (delta, height) {},
            onQueueDragEnd: (velocity) {},
            onTogglePlayPause: command,
            onPrevious: command,
            onNext: command,
            onToggleShuffle: command,
            onToggleRepeatMode: command,
            onToggleDontStopTheMusic: command,
            onToggleMute: command,
            onSeek: (seconds) async {},
            onOpenAudioOptions: (context) {},
          ),
        ),
      );
      await tester.pumpWidget(host());
      await tester.pumpAndSettle();
      expect(sourceRequests.length, lessThan(3));
      await tester.drag(find.byType(PageView), const Offset(-700, 0));
      await tester.pumpAndSettle();
      expect(sourceRequests, contains('cover-1'));
      expect(sourceRequests.length, lessThan(4));
      final beforeTransport = sourceRequests.length;
      state = PlayState.paused;
      await tester.pumpWidget(host());
      await tester.pumpAndSettle();
      expect(sourceRequests.length, beforeTransport);
      queue = Queue(
        currentIndex: 0,
        items: [
          queue.items.first,
          QueueItem(
            queueItemId: '1',
            mediaItem: track(1, path: 'changed-cover'),
          ),
          ...queue.items.skip(2),
        ],
      );
      await tester.pumpWidget(host());
      await tester.pumpAndSettle();
      expect(sourceRequests, contains('changed-cover'));
      final beforeLoaderChange = sourceRequests.length;
      configureLoader();
      await tester.pumpAndSettle();
      expect(sourceRequests.length, greaterThan(beforeLoaderChange));
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  Track mappedTrack(
    String provider,
    String itemId, {
    String? uri,
    List<Map<String, dynamic>> mappings = const [],
  }) => Track.fromJson(<String, dynamic>{
    'media_type': 'track',
    'provider': provider,
    'item_id': itemId,
    'uri': uri,
    'name': 'Track',
    'duration': 180,
    'provider_mappings': mappings,
  });

  test('selected YouTube payload matches its library playlist row', () async {
    final libraryTrack = mappedTrack(
      'library',
      '123',
      uri: 'library://track/123',
      mappings: [
        {
          'provider_domain': 'youtube',
          'provider_instance': 'youtube-a',
          'item_id': 'video-id',
          'available': true,
        },
      ],
    );
    final selected = Track.fromJson(
      await createSelectedProviderMediaItemPayload(
        libraryTrack,
        providerInstance: 'youtube-a',
      ),
    );
    expect(selected.uri, 'youtube-a://track/video-id');
    expect(isSamePlaybackTrack(libraryTrack, selected), isTrue);
    expect(isSamePlaybackTrack(selected, libraryTrack), isTrue);
  });

  test(
    'shared exact mappings match even when both primary identities differ',
    () {
      final mapping = {
        'provider_domain': 'youtube',
        'provider_instance': 'youtube-a',
        'item_id': 'video-id',
      };
      final left = mappedTrack(
        'library',
        '123',
        uri: 'library://track/123',
        mappings: [mapping],
      );
      final right = mappedTrack(
        'source',
        '456',
        uri: 'source://track/456',
        mappings: [mapping],
      );
      expect(isSamePlaybackTrack(left, right), isTrue);
      expect(isSamePlaybackTrack(right, left), isTrue);
    },
  );

  test('equal item IDs never cross provider instances or media types', () {
    final left = mappedTrack('youtube-a', 'video-id');
    expect(
      isSamePlaybackTrack(left, mappedTrack('youtube-b', 'video-id')),
      isFalse,
    );
    expect(
      isSamePlaybackTrack(left, mappedTrack('youtube-a', 'video-id')),
      isTrue,
    );
    expect(
      isSamePlaybackTrack(
        left,
        Album.fromJson({
          'provider': 'youtube-a',
          'item_id': 'video-id',
          'name': 'Album',
        }),
      ),
      isFalse,
    );
    expect(
      isSamePlaybackTrack(
        mappedTrack('', 'video-id'),
        mappedTrack('', 'video-id'),
      ),
      isFalse,
    );
  });

  test('URI identity remains valid without provider mappings', () {
    expect(
      isSamePlaybackTrack(
        mappedTrack('library', '123', uri: ' library://track/123 '),
        mappedTrack('library', '123', uri: 'LIBRARY://TRACK/123'),
      ),
      isTrue,
    );
  });

  testWidgets(
    'pending YouTube start marks the playlist row before the queue changes',
    (tester) async {
      final bridge = _PlaybackIndicatorBridge();
      final events = MashinCoreBridgeEvents(coreBridge: bridge);
      addTearDown(() async {
        events.dispose();
        await bridge.queues.close();
        await bridge.players.close();
      });
      await events.start();
      final libraryTrack = mappedTrack(
        'library',
        '123',
        uri: 'library://track/123',
        mappings: [
          {
            'provider_domain': 'youtube',
            'provider_instance': 'youtube-a',
            'item_id': 'video-id',
            'available': true,
          },
        ],
      );
      final selected = Track.fromJson(
        await createSelectedProviderMediaItemPayload(
          libraryTrack,
          providerInstance: 'youtube-a',
        ),
      );
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.light(),
          home: Scaffold(
            body: Column(
              children: [
                DesktopPlaybackIndicator(
                  key: const ValueKey('chosen-indicator'),
                  coreEvents: events,
                  item: libraryTrack,
                  fallback: '2',
                ),
                DesktopPlaybackIndicator(
                  key: const ValueKey('old-indicator'),
                  coreEvents: events,
                  item: bridge.oldTrack,
                  fallback: '1',
                ),
                PlaybackAwareBuilder(
                  coreEvents: events,
                  item: libraryTrack,
                  builder: (context, isCurrent) =>
                      Text(isCurrent ? 'marked' : 'unmarked'),
                ),
              ],
            ),
          ),
        ),
      );
      expect(find.text('unmarked'), findsOneWidget);
      bridge.players.add(
        PlayerState(
          playbackState: PlayState.buffering,
          requestedMedia: selected,
        ),
      );
      await tester.pump();
      expect(find.text('marked'), findsOneWidget);
      expect(
        find.descendant(
          of: find.byKey(const ValueKey('chosen-indicator')),
          matching: find.byType(CircularProgressIndicator),
        ),
        findsOneWidget,
      );
      expect(
        find.descendant(
          of: find.byKey(const ValueKey('old-indicator')),
          matching: find.byType(CircularProgressIndicator),
        ),
        findsNothing,
      );
      expect(
        events.mediaSessionState.queue.items.single.mediaItem.itemId,
        'old',
      );
      bridge.players.add(const PlayerState(playbackState: PlayState.error));
      await tester.pump();
      expect(find.text('unmarked'), findsOneWidget);
      expect(find.byType(CircularProgressIndicator), findsNothing);
    },
  );

  List<QueueItem> queueItems(List<int?> sortIndices) =>
      List<QueueItem>.generate(
        sortIndices.length,
        (index) => QueueItem(
          queueItemId: 'queue-$index',
          sortIndex: sortIndices[index],
          mediaItem: const Track(
            itemId: 'same-track',
            provider: 'test',
            name: 'Track',
            duration: 120,
            artists: <Artist>[],
          ),
        ),
      );

  test('queue numbers rank sparse sort indices without reordering items', () {
    final items = queueItems(<int?>[0, 12, 4, 9]);
    expect(items.normalizedItemNumbers, <int>[1, 4, 2, 3]);
    expect(items.map((item) => item.sortIndex), <int>[0, 12, 4, 9]);
    expect(items.map((item) => item.queueItemId), <String>[
      'queue-0',
      'queue-1',
      'queue-2',
      'queue-3',
    ]);
  });

  test('queue numbers follow item order only when sort indices are absent', () {
    expect(queueItems(<int?>[null, null, null]).normalizedItemNumbers, <int>[
      1,
      2,
      3,
    ]);
    expect(queueItems(<int?>[8, null, 2]).normalizedItemNumbers, <int>[
      3,
      1,
      2,
    ]);
  });

  test(
    'equal queue sort indices keep snapshot order including duplicate tracks',
    () {
      expect(queueItems(<int?>[8, 2, 8, 2]).normalizedItemNumbers, <int>[
        3,
        1,
        4,
        2,
      ]);
    },
  );

  test(
    'queue numbers are dense after removal and independent of index scale',
    () {
      final items = queueItems(<int?>[0, 12, 4, 9]);
      expect(
        <QueueItem>[items[0], items[1], items[3]].normalizedItemNumbers,
        <int>[1, 3, 2],
      );
      expect(
        queueItems(<int?>[100, 900, 200, 500]).normalizedItemNumbers,
        <int>[1, 4, 2, 3],
      );
    },
  );

  test('queue numbering handles empty and single-item snapshots', () {
    expect(queueItems(<int?>[]).normalizedItemNumbers, isEmpty);
    expect(queueItems(<int?>[42]).normalizedItemNumbers, <int>[1]);
  });

  test('projects the Rust player-state payload', () {
    final playerState = PlayerState.fromJson(<String, dynamic>{
      'activeMode': 'remote',
      'connectionState': 'online',
      'playbackState': 'playing',
      'currentQueueItemId': 'queue-item-7',
      'currentIndex': 0,
      'positionMillis': 12345,
      'durationMillis': 180000,
      'bufferedPositionMillis': 18000,
      'volume': 64,
      'muted': false,
    });

    expect(playerState.activeMode, PlaybackMode.remote);
    expect(playerState.connectionState, CoreConnectionState.online);
    expect(playerState.playbackState, PlayState.playing);
    expect(playerState.currentQueueItemId, 'queue-item-7');
    expect(playerState.positionMillis, 12345);
    expect(playerState.durationMillis, 180000);
    expect(playerState.bufferedPositionMillis, 18000);
    expect(playerState.volume, 64);
  });

  test('projects the Rust queue payload without player-state fields', () {
    final queue = Queue.fromJson(<String, dynamic>{
      'queueId': 'queue-42',
      'items': const <Map<String, dynamic>>[],
      'currentIndex': 0,
      'shuffleEnabled': true,
      'repeatMode': 'all',
      'autoplayEnabled': true,
    });

    expect(queue.queueId, 'queue-42');
    expect(queue.items, isEmpty);
    expect(queue.currentIndex, 0);
    expect(queue.shuffleEnabled, isTrue);
    expect(queue.repeatMode, RepeatMode.all);
    expect(queue.autoplayEnabled, isTrue);
  });

  test('requested media is displayed without changing the confirmed queue', () {
    final queue = Queue(items: queueItems(<int?>[0]), currentIndex: 0);
    final player = PlayerState.fromJson(<String, dynamic>{
      'playbackState': 'buffering',
      'requestedMedia': <String, dynamic>{
        'media_type': 'track',
        'item_id': 'chosen',
        'provider': 'test',
        'name': 'Chosen',
        'duration': 180,
      },
    });
    final state = MediaSessionState(queue: queue, playerState: player);
    expect(state.displayedMedia?.itemId, 'chosen');
    expect(state.queue.currentIndex, 0);
    expect(state.queue.items.single.mediaItem.itemId, 'same-track');
    expect(player.playbackState, PlayState.buffering);
    expect(
      MediaSessionState(queue: queue).displayedMedia?.itemId,
      'same-track',
    );
    expect(const MediaSessionState().displayedMedia, isNull);
  });

  test(
    'finite playlists expand in place and retain each track provider',
    () async {
      final bridge = _PlaybackResolutionBridge();
      final resolved = await resolveQueuePlayMedia(bridge, <MediaItem>[
        const Track(
          itemId: 'before',
          provider: 'source',
          name: 'Before',
          duration: 180,
          artists: [],
        ),
        const Playlist(
          itemId: 'playlist',
          provider: 'builtin',
          name: 'Playlist',
          isEditable: true,
        ),
        const Track(
          itemId: 'after',
          provider: 'source',
          name: 'After',
          duration: 180,
          artists: [],
        ),
      ], startItem: 1);
      expect(resolved.mediaItems.map((item) => item['item_id']), [
        'before',
        'first',
        'second',
        'after',
      ]);
      expect(
        resolved.mediaItems.map((item) => item['provider']),
        everyElement('source'),
      );
      expect(resolved.startItem, 1);
      expect(bridge.playlistReads, 1);
    },
  );

  test(
    'dynamic playlist commands retain their server-managed start path',
    () async {
      final bridge = _PlaybackResolutionBridge();
      final resolved = await resolveQueuePlayMedia(bridge, <MediaItem>[
        const Playlist(
          itemId: 'dynamic',
          provider: 'radio_playlist',
          name: 'Dynamic',
          isEditable: false,
          isDynamic: true,
        ),
      ]);
      expect(resolved.mediaItems.single['item_id'], 'dynamic');
      expect(resolved.mediaItems.single['is_dynamic'], isTrue);
      expect(bridge.playlistReads, 0);
    },
  );
}

class _PlaybackResolutionBridge implements MashinCoreBridgeApi {
  int playlistReads = 0;

  @override
  Future<List<Track>> getPlaylistTracks(
    String itemId,
    String provider, {
    bool forceRefresh = false,
    bool allowDynamicTracks = false,
  }) async {
    playlistReads++;
    return const <Track>[
      Track(
        itemId: 'first',
        provider: 'source',
        name: 'First',
        duration: 180,
        artists: [],
      ),
      Track(
        itemId: 'second',
        provider: 'source',
        name: 'Second',
        duration: 180,
        artists: [],
      ),
    ];
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class _PlaybackIndicatorBridge extends _PlaybackResolutionBridge {
  final queues = StreamController<Queue>.broadcast(sync: true);
  final players = StreamController<PlayerState>.broadcast(sync: true);
  final oldTrack = const Track(
    itemId: 'old',
    provider: 'library',
    name: 'Old',
    duration: 180,
    artists: [],
  );

  @override
  Stream<Queue> get queueChanges => queues.stream;

  @override
  Stream<PlayerState> get playerStateChanges => players.stream;

  @override
  Future<void> refreshQueue() async {
    queues.add(
      Queue(
        currentIndex: 0,
        items: [QueueItem(queueItemId: 'old', mediaItem: oldTrack)],
      ),
    );
  }

  @override
  Future<void> refreshPlayerState() async {
    players.add(const PlayerState(playbackState: PlayState.playing));
  }
}

class _OverlayChanges extends ChangeNotifier {
  bool get hasActiveListeners => hasListeners;
}
