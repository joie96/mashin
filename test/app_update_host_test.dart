import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/updates/app_update_dialog.dart';
import 'package:mashin/app/updates/app_update_service.dart';
import 'package:pub_semver/pub_semver.dart';

class _UpdateService extends Fake implements AppUpdateService {
  int checks = 0;
  int failuresRemaining = 0;
  AppUpdate? update;
  Completer<AppUpdate?>? pendingCheck;

  @override
  Future<AppUpdate?> checkForUpdate() async {
    checks++;
    if (failuresRemaining > 0) {
      failuresRemaining--;
      throw const SocketException('Network unavailable');
    }
    return pendingCheck == null ? update : await pendingCheck!.future;
  }

  @override
  void cancelDownload() {}
}

Widget _app({
  required GlobalKey<NavigatorState> navigatorKey,
  required AppUpdateService service,
  GlobalKey<NavigatorState>? hostNavigatorKey,
  bool isReady = true,
}) => MaterialApp(
  navigatorKey: navigatorKey,
  builder: (_, child) => AppUpdateHost(
    navigatorKey: hostNavigatorKey ?? navigatorKey,
    service: service,
    isReady: isReady,
    child: child!,
  ),
  home: const Scaffold(body: Text('Home')),
);

AppUpdate _update() => AppUpdate(
  version: Version.parse('0.0.2-beta.5'),
  notes: 'Release notes',
  fileName: 'Mashin-0.0.2-beta.5-android-arm64.apk',
  downloadUrl: Uri.https('github.com', '/installer.apk'),
  checksumUrl: Uri.https('github.com', '/installer.apk.sha256'),
  size: 100,
);

void main() {
  testWidgets('waits for startup readiness and checks only once', (
    tester,
  ) async {
    final service = _UpdateService();
    final navigatorKey = GlobalKey<NavigatorState>();
    await tester.pumpWidget(
      _app(navigatorKey: navigatorKey, service: service, isReady: false),
    );
    await tester.pump();
    expect(service.checks, 0);

    await tester.pumpWidget(_app(navigatorKey: navigatorKey, service: service));
    await tester.pump();
    expect(service.checks, 1);

    await tester.pumpWidget(_app(navigatorKey: navigatorKey, service: service));
    await tester.pump();
    expect(service.checks, 1);
  });

  testWidgets('retries failed checks without bypassing the retry delay', (
    tester,
  ) async {
    final service = _UpdateService()..failuresRemaining = 2;
    final navigatorKey = GlobalKey<NavigatorState>();
    await tester.pumpWidget(_app(navigatorKey: navigatorKey, service: service));
    await tester.pump();
    expect(service.checks, 1);

    await tester.pumpWidget(_app(navigatorKey: navigatorKey, service: service));
    expect(service.checks, 1);
    await tester.pump(const Duration(seconds: 2));
    expect(service.checks, 2);
    await tester.pump(const Duration(seconds: 5));
    expect(service.checks, 3);
    await tester.pump(const Duration(seconds: 30));
    expect(service.checks, 3);
  });

  testWidgets('stops after three failed attempts', (tester) async {
    final service = _UpdateService()..failuresRemaining = 10;
    final navigatorKey = GlobalKey<NavigatorState>();
    await tester.pumpWidget(_app(navigatorKey: navigatorKey, service: service));
    await tester.pump();
    await tester.pump(const Duration(seconds: 2));
    await tester.pump(const Duration(seconds: 5));
    await tester.pump(const Duration(seconds: 30));
    await tester.pumpWidget(_app(navigatorKey: navigatorKey, service: service));
    expect(service.checks, 3);
  });

  testWidgets('cancels scheduled checks when disposed', (tester) async {
    final service = _UpdateService()..failuresRemaining = 10;
    await tester.pumpWidget(
      _app(navigatorKey: GlobalKey<NavigatorState>(), service: service),
    );
    await tester.pump();
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump(const Duration(seconds: 30));
    expect(service.checks, 1);
  });

  testWidgets('pauses retries until startup is ready again', (tester) async {
    final service = _UpdateService()..failuresRemaining = 1;
    final navigatorKey = GlobalKey<NavigatorState>();
    await tester.pumpWidget(_app(navigatorKey: navigatorKey, service: service));
    await tester.pump();
    await tester.pumpWidget(
      _app(navigatorKey: navigatorKey, service: service, isReady: false),
    );
    await tester.pump(const Duration(seconds: 2));
    expect(service.checks, 1);
    await tester.pumpWidget(_app(navigatorKey: navigatorKey, service: service));
    await tester.pump();
    expect(service.checks, 2);
  });

  testWidgets('ignores request completion after disposal', (tester) async {
    final service = _UpdateService()..pendingCheck = Completer<AppUpdate?>();
    await tester.pumpWidget(
      _app(navigatorKey: GlobalKey<NavigatorState>(), service: service),
    );
    await tester.pumpWidget(const SizedBox.shrink());
    service.pendingCheck!.complete(_update());
    await tester.pumpAndSettle();
    expect(find.byType(AppUpdateDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('waits again if readiness changes during the request', (
    tester,
  ) async {
    final service = _UpdateService()..pendingCheck = Completer<AppUpdate?>();
    final navigatorKey = GlobalKey<NavigatorState>();
    await tester.pumpWidget(_app(navigatorKey: navigatorKey, service: service));
    await tester.pumpWidget(
      _app(navigatorKey: navigatorKey, service: service, isReady: false),
    );
    expect(service.checks, 1);
    service.pendingCheck!.complete(_update());
    await tester.pumpAndSettle();
    expect(find.byType(AppUpdateDialog), findsNothing);

    await tester.pumpWidget(_app(navigatorKey: navigatorKey, service: service));
    await tester.pumpAndSettle();
    expect(find.byType(AppUpdateDialog), findsOneWidget);
    expect(service.checks, 1);
  });

  testWidgets('rechecks readiness in the scheduled dialog callback', (
    tester,
  ) async {
    final service = _UpdateService()..pendingCheck = Completer<AppUpdate?>();
    final navigatorKey = GlobalKey<NavigatorState>();
    await tester.pumpWidget(_app(navigatorKey: navigatorKey, service: service));
    service.pendingCheck!.complete(_update());
    await tester.idle();
    await tester.pumpWidget(
      _app(navigatorKey: navigatorKey, service: service, isReady: false),
    );
    await tester.pumpAndSettle();
    expect(find.byType(AppUpdateDialog), findsNothing);
    await tester.pumpWidget(_app(navigatorKey: navigatorKey, service: service));
    await tester.pumpAndSettle();
    expect(find.byType(AppUpdateDialog), findsOneWidget);
  });

  testWidgets('cancels navigator waiting when disposed', (tester) async {
    final service = _UpdateService()..update = _update();
    await tester.pumpWidget(
      _app(
        navigatorKey: GlobalKey<NavigatorState>(),
        hostNavigatorKey: GlobalKey<NavigatorState>(),
        service: service,
      ),
    );
    await tester.pump();
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump(const Duration(seconds: 1));
    expect(find.byType(AppUpdateDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('retries dialog display when the navigator is not mounted', (
    tester,
  ) async {
    final service = _UpdateService()..update = _update();
    final navigatorKey = GlobalKey<NavigatorState>();
    final delayedNavigatorKey = GlobalKey<NavigatorState>();
    await tester.pumpWidget(
      _app(
        navigatorKey: navigatorKey,
        hostNavigatorKey: delayedNavigatorKey,
        service: service,
      ),
    );
    await tester.pump();
    expect(find.byType(AppUpdateDialog), findsNothing);
    await tester.pumpWidget(
      _app(navigatorKey: delayedNavigatorKey, service: service),
    );
    await tester.pump(const Duration(milliseconds: 250));
    await tester.pumpAndSettle();
    expect(find.byType(AppUpdateDialog), findsOneWidget);
    expect(service.checks, 1);

    delayedNavigatorKey.currentState!.pop();
    await tester.pumpAndSettle();
    await tester.pumpWidget(
      _app(navigatorKey: delayedNavigatorKey, service: service),
    );
    await tester.pumpAndSettle();
    expect(find.byType(AppUpdateDialog), findsNothing);
    expect(service.checks, 1);
  });
}
