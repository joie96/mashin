import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/settings/settings_screen.dart';

class _CacheBridge extends Fake implements MashinCoreBridgeApi {
  CacheUsage usage = const CacheUsage(metadataBytes: 2048, imageBytes: 3145728);
  int reads = 0;
  int metadataClears = 0;
  int imageClears = 0;
  bool shouldFailRead = false;
  bool shouldFailClear = false;
  Completer<void>? pendingClear;

  @override
  Future<CacheUsage> getCacheUsage() async {
    reads++;
    if (shouldFailRead) throw StateError('synthetic read failure');
    return usage;
  }

  @override
  Future<void> clearDatabaseCache() async {
    metadataClears++;
    if (shouldFailClear) throw StateError('synthetic clear failure');
    await pendingClear?.future;
    usage = CacheUsage(metadataBytes: 0, imageBytes: usage.imageBytes);
  }

  @override
  Future<void> clearImageCache() async {
    imageClears++;
    if (shouldFailClear) throw StateError('synthetic clear failure');
    await pendingClear?.future;
    usage = CacheUsage(metadataBytes: usage.metadataBytes, imageBytes: 0);
  }
}

Widget _app(Widget child) => MaterialApp(
  theme: AppTheme.light(),
  home: Scaffold(body: child),
);

Widget _settings(_CacheBridge bridge) => SettingsEditor(
  coreBridge: bridge,
  musicAssistantUrl: 'https://music.example',
  settings: const SessionSettingsUpdate(
    isDarkTheme: false,
    logLevel: LogLevelPreference.info,
  ),
  onSave: (_) async {},
  onLogout: () async {},
);

void main() {
  test('cache usage decodes the native byte-count contract strictly', () {
    final usage = CacheUsage.fromJson({'metadataBytes': 60, 'imageBytes': 72});
    expect(usage.metadataBytes, 60);
    expect(usage.imageBytes, 72);
    for (final payload in <Map<String, dynamic>>[
      {},
      {'metadataBytes': -1, 'imageBytes': 0},
      {'metadataBytes': 0, 'imageBytes': '72'},
      {'metadataBytes': 0, 'imageBytes': 1.5},
    ]) {
      expect(() => CacheUsage.fromJson(payload), throwsFormatException);
    }
  });

  for (final width in [390.0, 1200.0]) {
    testWidgets('general settings show both caches at width $width', (
      tester,
    ) async {
      tester.view.physicalSize = Size(width, 1000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final bridge = _CacheBridge();
      await tester.pumpWidget(
        _app(
          SettingsEditor(
            coreBridge: bridge,
            musicAssistantUrl: 'https://music.example',
            settings: const SessionSettingsUpdate(
              isDarkTheme: false,
              logLevel: LogLevelPreference.info,
            ),
            onSave: (_) async {},
            onLogout: () async {},
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.text('Bild-Cache'));
      await tester.pump();
      expect(find.text('Metadaten-Cache'), findsOneWidget);
      expect(find.text('2,0 KiB'), findsOneWidget);
      expect(find.text('3,0 MiB'), findsOneWidget);
      expect(find.byTooltip('Metadaten-Cache leeren'), findsOneWidget);
      expect(find.byTooltip('Bild-Cache leeren'), findsOneWidget);
      final cacheLeft = tester.getTopLeft(find.text('Cache')).dx;
      final rowLeft = tester.getTopLeft(find.text('Metadaten-Cache')).dx;
      expect(rowLeft - cacheLeft, width == 1200 ? 24 : 0);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    });
  }

  for (final shouldClearImages in [false, true]) {
    testWidgets('clearing images=$shouldClearImages refreshes only that area', (
      tester,
    ) async {
      final bridge = _CacheBridge();
      await tester.pumpWidget(_app(_settings(bridge)));
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.text('Bild-Cache'));
      await tester.pump();
      await tester.tap(
        find.byTooltip(
          shouldClearImages ? 'Bild-Cache leeren' : 'Metadaten-Cache leeren',
        ),
      );
      await tester.pumpAndSettle();
      expect(bridge.metadataClears, shouldClearImages ? 0 : 1);
      expect(bridge.imageClears, shouldClearImages ? 1 : 0);
      expect(bridge.reads, 2);
      expect(find.text('0 B'), findsOneWidget);
      expect(
        find.text(shouldClearImages ? '2,0 KiB' : '3,0 MiB'),
        findsOneWidget,
      );
      await tester.pumpWidget(const SizedBox.shrink());
    });
  }

  testWidgets('maintenance disables other actions and waits before rereading', (
    tester,
  ) async {
    final bridge = _CacheBridge()..pendingClear = Completer<void>();
    await tester.pumpWidget(_app(_settings(bridge)));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Bild-Cache'));
    await tester.pump();
    await tester.tap(find.byTooltip('Metadaten-Cache leeren'));
    await tester.pump();
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
    final imageButton = tester.widget<IconButton>(
      find.byWidgetPredicate(
        (widget) =>
            widget is IconButton && widget.tooltip == 'Bild-Cache leeren',
      ),
    );
    expect(imageButton.onPressed, isNull);
    await tester.pump(const Duration(seconds: 5));
    expect(bridge.reads, 1);
    bridge.pendingClear!.complete();
    await tester.pumpAndSettle();
    expect(bridge.reads, 2);
    expect(find.text('0 B'), findsOneWidget);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets(
    'read failures show unavailable rather than a false zero and retry',
    (tester) async {
      final bridge = _CacheBridge()..shouldFailRead = true;
      await tester.pumpWidget(_app(_settings(bridge)));
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.text('Cache-Größe nicht verfügbar.'));
      await tester.pump();
      expect(find.text('Nicht verfügbar'), findsNWidgets(2));
      expect(find.text('0 B'), findsNothing);
      bridge.shouldFailRead = false;
      await tester.tap(find.byTooltip('Cache-Größe erneut laden'));
      await tester.pumpAndSettle();
      expect(find.text('2,0 KiB'), findsOneWidget);
      expect(find.text('Cache-Größe nicht verfügbar.'), findsNothing);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  testWidgets('failed clear preserves size and reports failure', (
    tester,
  ) async {
    final bridge = _CacheBridge()..shouldFailClear = true;
    await tester.pumpWidget(_app(_settings(bridge)));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Bild-Cache'));
    await tester.pump();
    await tester.tap(find.byTooltip('Bild-Cache leeren'));
    await tester.pumpAndSettle();
    expect(find.text('Cache konnte nicht geleert werden.'), findsOneWidget);
    expect(find.text('3,0 MiB'), findsOneWidget);
    expect(bridge.reads, 2);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('polling refreshes physical usage only in an enabled view', (
    tester,
  ) async {
    final bridge = _CacheBridge();
    Widget view(bool isEnabled) =>
        _app(TickerMode(enabled: isEnabled, child: _settings(bridge)));
    await tester.pumpWidget(view(false));
    await tester.pumpAndSettle();
    bridge.usage = const CacheUsage(metadataBytes: 512, imageBytes: 1024);
    await tester.pump(const Duration(seconds: 5));
    expect(bridge.reads, 1);
    await tester.pumpWidget(view(true));
    await tester.pump(const Duration(seconds: 5));
    await tester.pumpAndSettle();
    expect(bridge.reads, 2);
    expect(find.text('512 B'), findsOneWidget);
    expect(find.text('1,0 KiB'), findsOneWidget);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump(const Duration(seconds: 5));
    expect(bridge.reads, 2);
  });
}
