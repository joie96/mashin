// This is a basic Flutter widget test.
//
// To perform an interaction with a widget in your test, use the WidgetTester
// utility in the flutter_test package. For example, you can send tap and scroll
// gestures. You can also use WidgetTester to find child widgets in the widget
// tree, read text, and verify that the values of widget properties are correct.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:image/image.dart' as img;

import 'package:mashin/app/navigation/app_router.dart';
import 'package:mashin/app/session/app_session_controller.dart';
import 'package:mashin/app/shell/app_shell.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/core/core_userdata.dart';
import 'package:mashin/features/playerbar/playerbar_overlay.dart';
import 'package:mashin/features/queue/queue_overlay.dart';
import 'package:mashin/features/search/search_screen_mobile.dart';
import 'package:mashin/features/shared/widgets/media_image.dart';
import 'dart:convert';

class _LightThemeSession extends AppSessionController {
  @override
  bool get isDarkTheme => false;
}

class _StartupBridge extends Fake implements MashinCoreBridge {
  final libraryStarted = Completer<void>();
  final library = Completer<Map<String, dynamic>>();
  final manifests = Completer<List<ProviderManifest>>();
  bool hasStartedLibrary = false;
  int libraryCalls = 0;
  bool hasRefreshedQueue = false;

  @override
  Stream<Map<String, dynamic>> get events => const Stream.empty();
  @override
  Stream<Queue> get queueChanges => const Stream.empty();
  @override
  Stream<PlayerState> get playerStateChanges => const Stream.empty();
  @override
  bool get isServiceOwnedPlayback => true;

  @override
  Future<bool> restorePersistedSession() async => true;

  @override
  Future<SessionSettings> getSessionSettings() async => const SessionSettings(
    musicAssistantUrl: 'http://localhost:8095',
    username: 'test',
    audioProfile: 'lossless',
    localCacheSizeBytes: 262144000,
    isDarkTheme: false,
    logLevel: LogLevelPreference.info,
  );

  @override
  Future<RecommendationSnapshot> getRecommendations({
    required RecommendationView view,
  }) {
    throw StateError(
      'Flutter startup must not start a second recommendation load',
    );
  }

  @override
  Future<Map<String, dynamic>> loadUserData() {
    hasStartedLibrary = true;
    libraryCalls += 1;
    if (!libraryStarted.isCompleted) libraryStarted.complete();
    return library.future;
  }

  @override
  Future<List<ProviderManifest>> getProviderManifests() => manifests.future;

  @override
  Future<void> refreshQueue() async => hasRefreshedQueue = true;

  @override
  Future<void> refreshPlayerState() async {}

  @override
  void log(LogLevelPreference level, String category, String message) {}

  @override
  Future<void> dispose() async {}
}

class _AuthenticatedSessionWithoutUserData extends AppSessionController {
  @override
  bool get isLoading => false;

  @override
  bool get isAuthenticated => true;
}

class _PlaylistShellSession extends AppSessionController {
  _PlaylistShellSession(MashinCoreBridgeApi bridge)
    : userData = UserData(coreBridge: bridge);

  final UserData userData;

  @override
  UserData get userDataController => userData;

  @override
  void dispose() {
    userData.dispose();
    super.dispose();
  }
}

class _PlaylistShellBridge implements MashinCoreBridgeApi {
  final List<Map<String, dynamic>> _playlists = <Map<String, dynamic>>[];
  String? cover;

  @override
  Stream<Map<String, dynamic>> get events => const Stream.empty();

  @override
  Future<Map<String, dynamic>> getUserData() async => <String, dynamic>{
    'favorites': <String, dynamic>{},
    'playlists': _playlists,
  };

  @override
  Future<void> createPrivatePlaylist(String name) async {
    _playlists.add(<String, dynamic>{
      'item_id': '1',
      'provider': 'library',
      'name': name,
      'items': <Map<String, dynamic>>[],
    });
  }

  @override
  Future<void> renamePrivatePlaylist(String playlistId, String name) async {
    _playlists.singleWhere(
      (playlist) => playlist['item_id'] == playlistId,
    )['name'] = name;
  }

  @override
  Future<void> addPrivatePlaylistTracks(
    String playlistId,
    List<Track> tracks,
  ) async {
    final Map<String, dynamic> playlist = _playlists.singleWhere(
      (playlist) => playlist['item_id'] == playlistId,
    );
    (playlist['items'] as List<Map<String, dynamic>>).addAll(
      tracks.map(
        (Track track) => <String, dynamic>{
          'item_id': track.itemId,
          'provider': track.provider,
          'name': track.name,
          'uri': track.uri,
          'duration': track.duration,
        },
      ),
    );
    playlist['metadata'] = <String, dynamic>{
      'images': <Map<String, dynamic>>[
        <String, dynamic>{
          'type': 'thumb',
          'path': cover,
          'provider': 'library',
        },
      ],
    };
  }

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw UnimplementedError(invocation.memberName.toString());
}

void main() {
  test(
    'startup releases home and delegates the one-shot data pipeline to Rust',
    () async {
      final bridge = _StartupBridge();
      final session = AppSessionController(coreBridge: bridge);
      addTearDown(session.dispose);
      final startup = session.initialize();
      expect(session.initialize(), same(startup));
      await bridge.libraryStarted.future;

      expect(session.isLoading, isFalse);
      expect(session.isAuthenticated, isTrue);
      expect(session.userDataController, isNotNull);
      expect(session.isDarkTheme, isFalse);
      expect(bridge.hasStartedLibrary, isTrue);
      expect(bridge.hasRefreshedQueue, isTrue);

      bridge.library.complete(<String, dynamic>{});
      bridge.manifests.complete(<ProviderManifest>[]);
      await startup;
      expect(bridge.hasStartedLibrary, isTrue);
      expect(bridge.libraryCalls, 1);
      expect(session.userDataController!.isLoadingLibraryData, isFalse);
    },
  );

  test(
    'startup library failure does not stop playback initialization',
    () async {
      final bridge = _StartupBridge();
      final session = AppSessionController(coreBridge: bridge);
      addTearDown(session.dispose);
      final startup = session.initialize();
      await bridge.libraryStarted.future;
      bridge.manifests.complete(<ProviderManifest>[]);
      await Future<void>.delayed(Duration.zero);
      bridge.library.completeError(StateError('Unavailable'));
      await startup;
      expect(session.isLoading, isFalse);
      expect(session.isAuthenticated, isTrue);
      expect(bridge.hasRefreshedQueue, isTrue);
    },
  );

  testWidgets('protected routes require initialized user data', (
    WidgetTester tester,
  ) async {
    final session = _AuthenticatedSessionWithoutUserData();
    final appRouter = AppRouter(session);
    addTearDown(appRouter.router.dispose);
    addTearDown(session.dispose);

    await tester.pumpWidget(
      MaterialApp.router(
        routerConfig: appRouter.router,
        theme: AppTheme.dark(),
      ),
    );
    await tester.pumpAndSettle();

    for (final location in <String>[
      AppRoutes.home,
      AppRoutes.favorites,
      AppRoutes.playlists,
    ]) {
      appRouter.router.go(location);
      await tester.pumpAndSettle();

      expect(
        appRouter.router.routeInformationProvider.value.uri.path,
        '/login',
      );
      expect(find.text('Bei Music Assistant anmelden'), findsOneWidget);
      expect(tester.takeException(), isNull);
    }
  });

  testWidgets('uses restored light theme on the first frame', (
    WidgetTester tester,
  ) async {
    final session = _LightThemeSession();

    await tester.pumpWidget(MashinApp(session: session));

    expect(
      tester.widget<MaterialApp>(find.byType(MaterialApp)).themeMode,
      ThemeMode.light,
    );
  });

  testWidgets('shows login screen when no session exists', (
    WidgetTester tester,
  ) async {
    final session = AppSessionController();
    await tester.pumpWidget(MashinApp(session: session));
    await tester.runAsync(session.initialize);
    await tester.pumpAndSettle();

    expect(find.text('Bei Music Assistant anmelden'), findsOneWidget);
    expect(find.byType(TextFormField), findsNWidgets(3));
    expect(find.widgetWithText(FilledButton, 'Anmelden'), findsOneWidget);
  });

  testWidgets(
    'desktop sidebar updates when a playlist is created and its cover changes',
    (WidgetTester tester) async {
      tester.view.physicalSize = const Size(1400, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final _PlaylistShellBridge bridge = _PlaylistShellBridge();
      final _PlaylistShellSession session = _PlaylistShellSession(bridge);
      final GlobalKey<NavigatorState> navigatorKey =
          GlobalKey<NavigatorState>();
      final ShellNavigationObserver observer = ShellNavigationObserver();
      final GoRouter router = GoRouter(
        initialLocation: AppRoutes.home,
        routes: <RouteBase>[
          ShellRoute(
            navigatorKey: navigatorKey,
            observers: <NavigatorObserver>[observer],
            builder:
                (BuildContext context, GoRouterState state, Widget child) =>
                    AppShell(
                      session: session,
                      navigatorKey: navigatorKey,
                      navigationObserver: observer,
                      child: child,
                    ),
            routes: <RouteBase>[
              GoRoute(
                path: AppRoutes.home,
                builder: (BuildContext context, GoRouterState state) =>
                    const ShellBackScope(child: Text('Home')),
              ),
            ],
          ),
        ],
      );
      addTearDown(router.dispose);
      addTearDown(session.dispose);

      await tester.pumpWidget(
        MaterialApp.router(routerConfig: router, theme: AppTheme.dark()),
      );
      expect(find.text('Neue Playlist'), findsNothing);

      final Playlist playlist = (await session.userData.createPrivatePlaylist(
        'Neue Playlist',
      ))!;
      await tester.pump();
      expect(find.text('Neue Playlist'), findsOneWidget);

      final String artwork =
          'data:image/jpeg;base64,${base64Encode(img.encodeJpg(img.Image(width: 2, height: 2)))}';
      bridge.cover = artwork;
      await session.userData.addPlaylistTracks('1', <Track>[
        const Track(
          itemId: 'track-1',
          provider: 'library',
          name: 'Titel',
          artists: <Artist>[],
          uri: 'library://track/1',
          duration: 180,
        ),
      ]);
      await tester.pump();
      expect(find.text('1 Titel • 3 Min.'), findsOneWidget);
      expect(
        find.byWidgetPredicate(
          (Widget widget) => widget is MediaImage && widget.source == artwork,
        ),
        findsOneWidget,
      );

      final String updatedArtwork =
          'data:image/jpeg;base64,${base64Encode(img.encodeJpg(img.Image(width: 3, height: 2)))}';
      bridge.cover = updatedArtwork;
      await session.userData.addPlaylistTracks('1', <Track>[
        const Track(
          itemId: 'track-2',
          provider: 'library',
          name: 'Weiterer Titel',
          artists: <Artist>[],
          uri: 'library://track/2',
          duration: 180,
        ),
      ]);
      await tester.pump();
      expect(find.text('2 Titel • 6 Min.'), findsOneWidget);
      expect(
        find.byWidgetPredicate(
          (Widget widget) =>
              widget is MediaImage && widget.source == updatedArtwork,
        ),
        findsOneWidget,
      );

      await session.userData.renamePlaylist(playlist, 'Umbenannt');
      await tester.pump();
      expect(find.text('Neue Playlist'), findsNothing);
      expect(find.text('Umbenannt'), findsOneWidget);
    },
  );

  testWidgets('Android back closes mobile overlays before the detail route', (
    WidgetTester tester,
  ) async {
    tester.view.physicalSize = const Size(540, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final AppSessionController session = AppSessionController();
    final GlobalKey<NavigatorState> navigatorKey = GlobalKey<NavigatorState>();
    final ShellNavigationObserver observer = ShellNavigationObserver();
    final GoRouter router = GoRouter(
      initialLocation: AppRoutes.home,
      routes: <RouteBase>[
        ShellRoute(
          navigatorKey: navigatorKey,
          observers: <NavigatorObserver>[observer],
          builder: (BuildContext context, GoRouterState state, Widget child) =>
              AppShell(
                session: session,
                navigatorKey: navigatorKey,
                navigationObserver: observer,
                child: child,
              ),
          routes: <RouteBase>[
            GoRoute(
              path: AppRoutes.home,
              builder: (BuildContext context, GoRouterState state) =>
                  const ShellBackScope(child: Center(child: Text('Home'))),
            ),
            GoRoute(
              path: '/artist/:provider/:itemId',
              builder: (BuildContext context, GoRouterState state) =>
                  const ShellBackScope(child: Center(child: Text('Artist'))),
            ),
          ],
        ),
      ],
    );
    addTearDown(router.dispose);
    addTearDown(session.dispose);

    await tester.pumpWidget(
      MaterialApp.router(routerConfig: router, theme: AppTheme.dark()),
    );
    router.push('/artist/test/1');
    await tester.pumpAndSettle();
    await tester.tap(
      find.byKey(const ValueKey<String>('mobile-player-track-swipe')),
    );
    await tester.pumpAndSettle();
    await tester.tap(
      find.descendant(
        of: find.byType(PlayerBarOverlay),
        matching: find.byIcon(Icons.queue_music_outlined),
      ),
    );
    await tester.pumpAndSettle();
    expect(
      tester.widget<QueueOverlay>(find.byType(QueueOverlay)).isOpen,
      isTrue,
    );

    await tester.binding.handlePopRoute();
    await tester.pumpAndSettle();

    expect(find.text('Artist'), findsOneWidget);
    expect(
      tester.widget<QueueOverlay>(find.byType(QueueOverlay)).isOpen,
      isFalse,
    );

    await tester.binding.handlePopRoute();
    await tester.pumpAndSettle();
    expect(find.text('Artist'), findsOneWidget);
    expect(
      tester.widget<PlayerBarOverlay>(find.byType(PlayerBarOverlay)).isOpen,
      isFalse,
    );

    await tester.binding.handlePopRoute();
    await tester.pumpAndSettle();
    expect(find.text('Home'), findsOneWidget);
  });

  testWidgets('mobile search focuses on entry and unfocuses on outside tap', (
    WidgetTester tester,
  ) async {
    final AppSessionController session = AppSessionController();
    addTearDown(session.dispose);

    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Builder(
          builder: (BuildContext context) => Scaffold(
            body: TextButton(
              onPressed: () => Navigator.of(context).push(
                MaterialPageRoute<void>(
                  builder: (BuildContext context) => Scaffold(
                    body: SearchScreenMobile(
                      coreBridge: session.coreBridge,
                      coreEvents: session.coreEvents,
                    ),
                  ),
                ),
              ),
              child: const Text('Suche öffnen'),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Suche öffnen'));
    await tester.pumpAndSettle();

    final Finder input = find.byType(EditableText);
    expect(tester.widget<EditableText>(input).focusNode.hasFocus, isTrue);
    expect(tester.testTextInput.isVisible, isTrue);

    await tester.tap(find.text('Suche'));
    await tester.pumpAndSettle();

    expect(tester.widget<EditableText>(input).focusNode.hasFocus, isFalse);
    expect(tester.testTextInput.isVisible, isFalse);
  });
}
