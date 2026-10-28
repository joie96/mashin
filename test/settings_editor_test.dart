import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/settings/settings_screen.dart';

void main() {
  test('multi-device visibility is transferred in session settings', () {
    const update = SessionSettingsUpdate(
      localCacheSizeBytes: 250 * 1024 * 1024,
      isDarkTheme: true,
      logLevel: LogLevelPreference.info,
      shouldShowMultiDeviceButton: false,
    );
    expect(update.toJson()['shouldShowMultiDeviceButton'], isFalse);
    expect(
      SessionSettings.fromJson(update.toJson()).shouldShowMultiDeviceButton,
      isFalse,
    );
    expect(SessionSettings.fromJson({}).shouldShowMultiDeviceButton, isTrue);
  });

  for (final isCompact in [true, false]) {
    testWidgets(
      'multi-device visibility saves and survives other edits in ${isCompact ? 'mobile' : 'desktop'} layout',
      (tester) async {
        final saved = <SessionSettingsUpdate>[];
        await _pumpEditor(
          tester,
          isCompact,
          (update) async => saved.add(update),
        );
        final toggle = find.byKey(const Key('settings-multi-device-button'));
        await tester.ensureVisible(toggle);
        await tester.tap(toggle);
        await tester.pumpAndSettle();
        expect(saved.single.shouldShowMultiDeviceButton, isFalse);
        await tester.ensureVisible(find.text('Hell'));
        await tester.tap(find.text('Hell'));
        await tester.pumpAndSettle();
        expect(saved.last.shouldShowMultiDeviceButton, isFalse);
        expect(saved.last.isDarkTheme, isFalse);
        expect(tester.takeException(), isNull);
      },
    );

    testWidgets(
      'settings save on blur and selection in ${isCompact ? 'mobile' : 'desktop'} layout',
      (tester) async {
        final saved = <SessionSettingsUpdate>[];
        await _pumpEditor(
          tester,
          isCompact,
          (update) async => saved.add(update),
        );
        expect(find.text('Abbrechen'), findsNothing);
        expect(find.text('Speichern'), findsNothing);
        final cache = find.byKey(const Key('settings-cache-size'));
        await tester.ensureVisible(cache);
        await tester.enterText(cache, '500');
        expect(saved, isEmpty);
        tester.widget<TextField>(cache).focusNode!.unfocus();
        await tester.pumpAndSettle();
        expect(saved, hasLength(1));
        expect(saved.last.localCacheSizeBytes, 500 * 1024 * 1024);
        await tester.ensureVisible(find.text('Hell'));
        await tester.tap(find.text('Hell'));
        await tester.pumpAndSettle();
        expect(saved.last.isDarkTheme, isFalse);
        await tester.ensureVisible(cache);
        await tester.enterText(cache, '24');
        tester.widget<TextField>(cache).focusNode!.unfocus();
        await tester.pumpAndSettle();
        expect(saved, hasLength(2));
        expect(tester.widget<TextField>(cache).controller!.text, '500');
        await tester.enterText(cache, '600');
        await tester.testTextInput.receiveAction(TextInputAction.done);
        await tester.pumpAndSettle();
        expect(saved, hasLength(3));
        await tester.tap(find.text('Log'));
        await tester.pumpAndSettle();
        expect(find.byTooltip('Logpfad anzeigen'), findsNothing);
        expect(find.byTooltip('Log speichern'), findsNothing);
        expect(find.byIcon(Icons.folder_outlined), findsNothing);
        await tester.tap(
          find.byType(DropdownButtonFormField<LogLevelPreference>),
        );
        await tester.pumpAndSettle();
        await tester.tap(find.text('Debug').last);
        await tester.pumpAndSettle();
        expect(saved, hasLength(4));
        expect(saved.last.localCacheSizeBytes, 600 * 1024 * 1024);
        expect(saved.last.isDarkTheme, isFalse);
        expect(saved.last.logLevel, LogLevelPreference.debug);
        expect(tester.takeException(), isNull);
      },
    );

    testWidgets(
      'settings saves are ordered in ${isCompact ? 'mobile' : 'desktop'} layout',
      (tester) async {
        final saved = <SessionSettingsUpdate>[];
        final firstSave = Completer<void>();
        await _pumpEditor(tester, isCompact, (update) async {
          saved.add(update);
          if (saved.length == 1) {
            await firstSave.future;
          }
        });
        final cache = find.byKey(const Key('settings-cache-size'));
        await tester.ensureVisible(cache);
        await tester.enterText(cache, '500');
        await tester.testTextInput.receiveAction(TextInputAction.done);
        await tester.pumpAndSettle();
        await tester.ensureVisible(find.text('Hell'));
        await tester.tap(find.text('Hell'));
        await tester.pumpAndSettle();
        expect(saved, hasLength(1));
        firstSave.complete();
        await tester.pumpAndSettle();
        expect(saved, hasLength(2));
        expect(saved.last.localCacheSizeBytes, 500 * 1024 * 1024);
        expect(saved.last.isDarkTheme, isFalse);
        expect(tester.takeException(), isNull);
      },
    );

    testWidgets(
      'failed settings save can be retried in ${isCompact ? 'mobile' : 'desktop'} layout',
      (tester) async {
        var attempts = 0;
        final saved = <SessionSettingsUpdate>[];
        await _pumpEditor(tester, isCompact, (update) async {
          attempts += 1;
          if (attempts == 1) {
            throw StateError('Save failed');
          }
          saved.add(update);
        });
        final cache = find.byKey(const Key('settings-cache-size'));
        await tester.ensureVisible(cache);
        await tester.enterText(cache, '500');
        await tester.testTextInput.receiveAction(TextInputAction.done);
        await tester.pumpAndSettle();
        expect(saved, isEmpty);
        expect(tester.widget<TextField>(cache).controller!.text, '250');
        expect(
          find.textContaining('konnten nicht gespeichert'),
          findsOneWidget,
        );
        await tester.enterText(cache, '500');
        await tester.testTextInput.receiveAction(TextInputAction.done);
        await tester.pumpAndSettle();
        expect(saved.single.localCacheSizeBytes, 500 * 1024 * 1024);
        expect(tester.takeException(), isNull);
      },
    );

    testWidgets(
      'leaving settings commits the active field in ${isCompact ? 'mobile' : 'desktop'} layout',
      (tester) async {
        final saved = <SessionSettingsUpdate>[];
        await _pumpEditor(
          tester,
          isCompact,
          (update) async => saved.add(update),
        );
        final cache = find.byKey(const Key('settings-cache-size'));
        await tester.ensureVisible(cache);
        await tester.tap(cache);
        tester.widget<TextField>(cache).focusNode!.unfocus();
        await tester.pumpAndSettle();
        expect(saved, isEmpty);
        await tester.enterText(cache, '600');
        await tester.pumpWidget(const SizedBox.shrink());
        await tester.pumpAndSettle();
        expect(saved.single.localCacheSizeBytes, 600 * 1024 * 1024);
        expect(tester.takeException(), isNull);
      },
    );
  }
}

// Shared themed host for both responsive settings layouts.
Future<void> _pumpEditor(
  WidgetTester tester,
  bool isCompact,
  Future<void> Function(SessionSettingsUpdate) onSave,
) async {
  tester.view.physicalSize = isCompact
      ? const Size(390, 800)
      : const Size(1200, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.dark(),
      home: Scaffold(
        body: SettingsEditor(
          musicAssistantUrl: 'https://ma.example',
          settings: const SessionSettingsUpdate(
            localCacheSizeBytes: 250 * 1024 * 1024,
            isDarkTheme: true,
            logLevel: LogLevelPreference.info,
          ),
          onSave: onSave,
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}
