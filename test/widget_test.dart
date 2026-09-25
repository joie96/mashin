// This is a basic Flutter widget test.
//
// To perform an interaction with a widget in your test, use the WidgetTester
// utility in the flutter_test package. For example, you can send tap and scroll
// gestures. You can also use WidgetTester to find child widgets in the widget
// tree, read text, and verify that the values of widget properties are correct.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:mashin_flutter/app/navigation/app_router.dart';
import 'package:mashin_flutter/app/session/app_session_controller.dart';
import 'package:mashin_flutter/app/shell/app_shell.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/playerbar/playerbar_overlay.dart';
import 'package:mashin_flutter/features/queue/queue_overlay.dart';
import 'package:mashin_flutter/features/search/search_screen_mobile.dart';

void main() {
  testWidgets('shows login screen when no session exists', (
    WidgetTester tester,
  ) async {
    SharedPreferences.setMockInitialValues(<String, Object>{});

    await tester.pumpWidget(const MashinApp());
    await tester.pumpAndSettle();

    expect(find.text('Bei Music Assistant anmelden'), findsOneWidget);
    expect(find.byType(TextFormField), findsNWidgets(3));
    expect(find.widgetWithText(FilledButton, 'Anmelden'), findsOneWidget);
  });

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
