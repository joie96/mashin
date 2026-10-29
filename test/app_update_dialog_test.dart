import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/app/updates/app_update_dialog.dart';
import 'package:mashin/app/updates/app_update_service.dart';
import 'package:pub_semver/pub_semver.dart';

class FakeUpdateService extends AppUpdateService {
  bool hasCancelled = false;
  int downloadCount = 0;
  int checkCount = 0;
  AppUpdate? startupUpdate;
  bool shouldFailCheck = false;
  Completer<File>? pendingDownload;
  void Function(double)? reportProgress;

  @override
  Future<AppUpdate?> checkForUpdate() async {
    checkCount++;
    if (shouldFailCheck) throw const SocketException('offline');
    return startupUpdate;
  }

  @override
  Future<File> download(
    AppUpdate update, {
    required void Function(double) onProgress,
  }) async {
    downloadCount++;
    reportProgress = onProgress;
    if (pendingDownload != null) return pendingDownload!.future;
    onProgress(1);
    return File('update.msi');
  }

  @override
  void cancelDownload() => hasCancelled = true;
}

void main() {
  final update = AppUpdate(
    version: Version.parse('0.0.1-dev.10'),
    notes:
        "## What's Changed\n${List.filled(20, '* A release improvement with **Markdown**').join('\n')}",
    fileName: 'update.msi',
    downloadUrl: Uri.https('github.com', '/installer'),
    checksumUrl: Uri.https('github.com', '/checksum'),
    size: 3,
  );

  Future<void> open(
    WidgetTester tester,
    FakeUpdateService service, {
    required Future<void> Function(File) install,
    ThemeData? theme,
  }) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: theme ?? AppTheme.dark(),
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () => showDialog<void>(
                context: context,
                builder: (_) => AppUpdateDialog(
                  update: update,
                  service: service,
                  install: install,
                ),
              ),
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Open'));
    await tester.pumpAndSettle();
  }

  for (final size in [
    const Size(1280, 800),
    const Size(360, 640),
    const Size(640, 360),
  ]) {
    testWidgets(
      'release notes and buttons fit at $size and cancel does not install',
      (tester) async {
        tester.view.physicalSize = size;
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final service = FakeUpdateService();
        var hasInstalled = false;
        await open(tester, service, install: (_) async => hasInstalled = true);
        expect(find.text('Update 0.0.1-dev.10 verfügbar'), findsOneWidget);
        expect(find.text("What's Changed"), findsOneWidget);
        expect(find.text('See full changelog').hitTestable(), findsOneWidget);
        if (size.width >= 720) {
          final dialogSize = tester.getSize(find.byType(Dialog));
          expect(dialogSize.width, greaterThanOrEqualTo(800));
          expect(dialogSize.height, greaterThanOrEqualTo(600));
        }
        expect(tester.takeException(), isNull);
        await tester.tap(find.text('Abbrechen'));
        await tester.pumpAndSettle();
        expect(service.hasCancelled, isTrue);
        expect(hasInstalled, isFalse);
        expect(service.downloadCount, 0);
      },
    );
  }

  for (final isDark in [true, false]) {
    for (final size in [
      const Size(1280, 800),
      const Size(360, 640),
      const Size(640, 360),
    ]) {
      testWidgets(
        'download progress is visible and neutral at $size, dark=$isDark',
        (tester) async {
          tester.view.physicalSize = size;
          tester.view.devicePixelRatio = 1;
          addTearDown(tester.view.resetPhysicalSize);
          addTearDown(tester.view.resetDevicePixelRatio);
          final theme = isDark ? AppTheme.dark() : AppTheme.light();
          final palette = theme.extension<MashinColors>()!;
          final service = FakeUpdateService()
            ..pendingDownload = Completer<File>();
          await open(tester, service, install: (_) async {}, theme: theme);
          await tester.tap(find.text('Update'));
          await tester.pump();
          service.reportProgress!(0.42);
          await tester.pump(const Duration(milliseconds: 300));

          final progress = tester.widget<LinearProgressIndicator>(
            find.byType(LinearProgressIndicator),
          );
          expect(progress.value, 0.42);
          expect(progress.color, palette.textPrimary);
          expect(progress.backgroundColor, palette.selection);
          expect(progress.color, isNot(theme.colorScheme.primary));
          final foregroundLuminance = progress.color!.computeLuminance();
          final backgroundLuminance = progress.backgroundColor!
              .computeLuminance();
          final contrast = foregroundLuminance > backgroundLuminance
              ? (foregroundLuminance + 0.05) / (backgroundLuminance + 0.05)
              : (backgroundLuminance + 0.05) / (foregroundLuminance + 0.05);
          expect(contrast, greaterThan(3));
          expect(find.text('Download: 42 %').hitTestable(), findsOneWidget);
          expect(find.text('See full changelog').hitTestable(), findsOneWidget);
          expect(find.text('Abbrechen').hitTestable(), findsOneWidget);
          expect(tester.takeException(), isNull);

          await tester.tap(find.text('Abbrechen'));
          service.pendingDownload!.completeError(UpdateCancelled());
          await tester.pumpAndSettle();
        },
      );
    }
  }

  testWidgets(
    'Update hands the downloaded file to installer and dismisses dialog',
    (tester) async {
      final service = FakeUpdateService();
      File? installed;
      await open(tester, service, install: (file) async => installed = file);
      await tester.tap(find.text('Update'));
      await tester.pumpAndSettle();
      expect(installed?.path, 'update.msi');
      expect(find.byType(AppUpdateDialog), findsNothing);
    },
  );

  testWidgets('installer errors allow retry without another download', (
    tester,
  ) async {
    final service = FakeUpdateService();
    var attempts = 0;
    await open(
      tester,
      service,
      install: (_) async {
        attempts++;
        if (attempts == 1) throw StateError('Installer unavailable');
      },
    );
    await tester.tap(find.text('Update'));
    await tester.pumpAndSettle();
    expect(find.textContaining('Update fehlgeschlagen'), findsOneWidget);
    await tester.tap(find.text('Update'));
    await tester.pumpAndSettle();
    expect(attempts, 2);
    expect(service.downloadCount, 1);
    expect(find.byType(AppUpdateDialog), findsNothing);
  });

  testWidgets(
    'startup checks once and waits for session readiness before showing overlay',
    (tester) async {
      final service = FakeUpdateService()..startupUpdate = update;
      final key = GlobalKey<NavigatorState>();
      final isReady = ValueNotifier(false);
      addTearDown(isReady.dispose);
      await tester.pumpWidget(
        ValueListenableBuilder<bool>(
          valueListenable: isReady,
          builder: (_, ready, _) => MaterialApp(
            navigatorKey: key,
            builder: (_, child) => AppUpdateHost(
              navigatorKey: key,
              isReady: ready,
              service: service,
              child: child!,
            ),
            home: const Scaffold(body: Text('App content')),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(service.checkCount, 1);
      expect(find.byType(AppUpdateDialog), findsNothing);
      isReady.value = true;
      await tester.pumpAndSettle();
      expect(find.byType(AppUpdateDialog), findsOneWidget);
      await tester.tap(find.text('Abbrechen'));
      await tester.pumpAndSettle();
      isReady.value = false;
      await tester.pump();
      isReady.value = true;
      await tester.pumpAndSettle();
      expect(service.checkCount, 1);
      expect(find.byType(AppUpdateDialog), findsNothing);
    },
  );

  testWidgets('offline startup never blocks app content', (tester) async {
    final key = GlobalKey<NavigatorState>();
    await tester.pumpWidget(
      MaterialApp(
        navigatorKey: key,
        builder: (_, child) => AppUpdateHost(
          navigatorKey: key,
          service: FakeUpdateService()..shouldFailCheck = true,
          child: child!,
        ),
        home: const Scaffold(body: Text('App content')),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('App content'), findsOneWidget);
    expect(find.byType(AppUpdateDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
