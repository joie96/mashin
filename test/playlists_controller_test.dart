import 'dart:async';
import 'dart:collection' show ListBase;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_events.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/core/core_userdata.dart';
import 'package:mashin/features/playlist/playlists_controller.dart';
import 'package:mashin/features/playlist/playlists_page_mobile.dart';

class _PlaylistBridge extends Fake implements MashinCoreBridgeApi {
  @override
  Stream<Map<String, dynamic>> get events => const Stream.empty();
}

class _PlaylistUserData extends UserData {
  _PlaylistUserData(MashinCoreBridgeApi bridge) : super(coreBridge: bridge);

  List<Playlist> currentPlaylists = const [];
  List<Track> currentTracks = const [];
  Future<void> Function()? readSnapshot;

  @override
  List<Playlist> get playlists => currentPlaylists;

  @override
  bool get isLoadingPlaylists => false;

  @override
  List<Track> tracksForPlaylist(String playlistId) => currentTracks;

  @override
  Future<void> refreshLibrarySnapshot() =>
      readSnapshot?.call() ?? Future<void>.value();

  void publish() => notifyListeners();
}

class _CountingTracks extends ListBase<Track> {
  _CountingTracks(this.items);

  final List<Track> items;
  int reads = 0;

  @override
  int get length => items.length;

  @override
  set length(int value) => throw UnsupportedError('Read-only tracks');

  @override
  Track operator [](int index) {
    reads++;
    return items[index];
  }

  @override
  void operator []=(int index, Track value) =>
      throw UnsupportedError('Read-only tracks');
}

void main() {
  final playlist = Playlist.fromJson({
    'item_id': 'playlist',
    'provider': 'library',
    'name': 'Playlist',
  });
  const track = Track(
    itemId: 'track',
    provider: 'library',
    name: 'Track',
    duration: 120,
    artists: [],
  );

  test(
    'cached playlists are visible before snapshot completion and survive failure',
    () async {
      final bridge = _PlaylistBridge();
      final completion = Completer<void>();
      final userData = _PlaylistUserData(bridge)
        ..currentPlaylists = [playlist]
        ..currentTracks = [track]
        ..readSnapshot = () => completion.future;
      final controller = PlaylistsController(
        coreBridge: bridge,
        userDataController: userData,
      );
      addTearDown(userData.dispose);
      addTearDown(controller.dispose);
      expect(controller.playlists, same(userData.currentPlaylists));
      final loading = controller.load();
      expect(controller.playlists, same(userData.currentPlaylists));
      expect(controller.isLoading, isTrue);
      expect(controller.summaryFor(playlist), (
        trackCount: 1,
        totalSeconds: 120,
      ));
      completion.completeError(StateError('offline'));
      await loading;
      expect(controller.playlists, same(userData.currentPlaylists));
      expect(controller.hasLoadError, isFalse);
      expect(controller.isLoading, isFalse);
    },
  );

  test(
    'cache publications appear while the initial snapshot is pending',
    () async {
      final bridge = _PlaylistBridge();
      final completion = Completer<void>();
      final userData = _PlaylistUserData(bridge)
        ..readSnapshot = () => completion.future;
      final controller = PlaylistsController(
        coreBridge: bridge,
        userDataController: userData,
      );
      addTearDown(userData.dispose);
      addTearDown(controller.dispose);
      final loading = controller.load();
      expect(controller.playlists, isEmpty);
      expect(controller.isLoading, isTrue);
      var notifications = 0;
      controller.addListener(() => notifications++);
      userData.currentPlaylists = [playlist];
      userData.currentTracks = [track];
      userData.publish();
      expect(notifications, 1);
      expect(controller.playlists, same(userData.currentPlaylists));
      expect(controller.summaryFor(playlist), (
        trackCount: 1,
        totalSeconds: 120,
      ));
      completion.completeError(StateError('offline'));
      await loading;
      expect(controller.playlists, isNotEmpty);
      expect(controller.hasLoadError, isFalse);
    },
  );

  test(
    'late cache recovers a failed snapshot and online replacement remains live',
    () async {
      final bridge = _PlaylistBridge();
      final userData = _PlaylistUserData(bridge)
        ..readSnapshot = () => Future<void>.error(StateError('offline'));
      final controller = PlaylistsController(
        coreBridge: bridge,
        userDataController: userData,
      );
      addTearDown(userData.dispose);
      addTearDown(controller.dispose);
      await controller.load();
      expect(controller.playlists, isEmpty);
      expect(controller.hasLoadError, isTrue);
      expect(controller.isLoading, isFalse);
      var notifications = 0;
      controller.addListener(() => notifications++);
      userData.currentPlaylists = [playlist];
      userData.currentTracks = [track];
      userData.publish();
      expect(notifications, 1);
      expect(controller.playlists, isNotEmpty);
      expect(controller.hasLoadError, isFalse);
      final online = Playlist.fromJson({
        'item_id': 'online',
        'provider': 'library',
        'name': 'Online',
      });
      userData.currentPlaylists = [online];
      userData.currentTracks = [track, track];
      userData.publish();
      expect(notifications, 2);
      expect(controller.playlists, [online]);
      expect(controller.summaryFor(online), (trackCount: 2, totalSeconds: 240));
      expect(controller.summaryFor(playlist), (trackCount: 0, totalSeconds: 0));
      userData.currentPlaylists = [];
      userData.currentTracks = [];
      userData.publish();
      expect(controller.playlists, isEmpty);
      expect(controller.hasLoadError, isFalse);
    },
  );

  test(
    'completed empty snapshot is distinct from pending and failed reads',
    () async {
      final bridge = _PlaylistBridge();
      final userData = _PlaylistUserData(bridge);
      final controller = PlaylistsController(
        coreBridge: bridge,
        userDataController: userData,
      );
      addTearDown(userData.dispose);
      addTearDown(controller.dispose);
      expect(controller.playlists, isEmpty);
      expect(controller.isLoading, isTrue);
      await controller.load();
      expect(controller.playlists, isEmpty);
      expect(controller.isLoading, isFalse);
      expect(controller.hasLoadError, isFalse);
    },
  );

  testWidgets('mobile playlists show late cache before snapshot completion', (
    tester,
  ) async {
    final bridge = _PlaylistBridge();
    final completion = Completer<void>();
    final userData = _PlaylistUserData(bridge)
      ..readSnapshot = () => completion.future;
    final events = MashinCoreBridgeEvents(coreBridge: bridge);
    addTearDown(userData.dispose);
    addTearDown(events.dispose);
    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: PlaylistsPageMobile(
            coreBridge: bridge,
            coreEvents: events,
            userDataController: userData,
            onOpenPlaylist: (_) {},
          ),
        ),
      ),
    );
    await tester.pump();
    expect(find.text('Keine Wiedergabelisten vorhanden.'), findsNothing);
    expect(find.text('Playlist'), findsNothing);
    userData.currentPlaylists = [playlist];
    userData.currentTracks = [track];
    userData.publish();
    await tester.pumpAndSettle();
    expect(completion.isCompleted, isFalse);
    expect(find.text('Playlist'), findsOneWidget);
    expect(find.text('1 Titel • 2m'), findsOneWidget);
    completion.completeError(StateError('offline'));
    await tester.pumpAndSettle();
    expect(find.text('Playlist'), findsOneWidget);
    expect(
      find.text('Wiedergabelisten konnten nicht geladen werden.'),
      findsNothing,
    );
    await tester.pumpWidget(const SizedBox.shrink());
    expect(tester.takeException(), isNull);
  });

  test(
    'playlist summaries do not rescan retained tracks while rendering',
    () async {
      final bridge = _PlaylistBridge();
      final tracks = _CountingTracks(List.filled(1000, track));
      final userData = _PlaylistUserData(bridge)
        ..currentPlaylists = [playlist]
        ..currentTracks = tracks;
      final controller = PlaylistsController(
        coreBridge: bridge,
        userDataController: userData,
      );
      addTearDown(userData.dispose);
      addTearDown(controller.dispose);
      await controller.load();
      expect(tracks.reads, 1000);
      var notifications = 0;
      controller.addListener(() => notifications++);
      for (var index = 0; index < 100; index++) {
        expect(controller.summaryFor(playlist), (
          trackCount: 1000,
          totalSeconds: 120000,
        ));
      }
      userData.publish();
      expect(tracks.reads, 1000);
      expect(notifications, 0);
    },
  );

  test(
    'playlist summaries update on track-only changes and remove stale entries',
    () async {
      final bridge = _PlaylistBridge();
      final userData = _PlaylistUserData(bridge)
        ..currentPlaylists = [playlist]
        ..currentTracks = [track];
      final controller = PlaylistsController(
        coreBridge: bridge,
        userDataController: userData,
      );
      addTearDown(userData.dispose);
      var isControllerDisposed = false;
      addTearDown(() {
        if (!isControllerDisposed) controller.dispose();
      });
      await controller.load();
      var notifications = 0;
      controller.addListener(() => notifications++);
      userData.currentTracks = [track, track];
      userData.publish();
      expect(controller.summaryFor(playlist), (
        trackCount: 2,
        totalSeconds: 240,
      ));
      expect(notifications, 1);
      userData.currentTracks = [];
      userData.publish();
      expect(controller.summaryFor(playlist), (trackCount: 0, totalSeconds: 0));
      expect(notifications, 2);
      userData.currentTracks = [track];
      userData.publish();
      userData.currentPlaylists = [];
      userData.publish();
      expect(controller.summaryFor(playlist), (trackCount: 0, totalSeconds: 0));
      controller.dispose();
      isControllerDisposed = true;
      userData.currentPlaylists = [playlist];
      userData.publish();
      expect(controller.summaryFor(playlist), (trackCount: 0, totalSeconds: 0));
    },
  );
}
