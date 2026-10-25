import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/settings/settings_screen.dart';

void main() {
  for (final isCompact in [true, false]) {
    testWidgets('settings draft cancel and save in ${isCompact ? 'mobile' : 'desktop'} layout', (tester) async {
      tester.view.physicalSize = isCompact ? const Size(390, 800) : const Size(1200, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final saved = <SessionSettingsUpdate>[];
      await tester.pumpWidget(MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(body: SettingsEditor(
          musicAssistantUrl: 'https://ma.example',
          settings: const SessionSettingsUpdate(
            localCacheSizeBytes: 250 * 1024 * 1024,
            isDarkTheme: true,
            logLevel: LogLevelPreference.info,
          ),
          onSave: (update) async => saved.add(update),
        )),
      ));
      await tester.pumpAndSettle();
      final cache = find.byKey(const Key('settings-cache-size'));
      await tester.ensureVisible(cache);
      await tester.enterText(cache, '500');
      await tester.ensureVisible(find.text('Hell'));
      await tester.tap(find.text('Hell'));
      await tester.pumpAndSettle();
      expect(saved, isEmpty);
      await tester.tap(find.text('Abbrechen'));
      await tester.pumpAndSettle();
      expect(tester.widget<TextField>(cache).controller!.text, '250');
      await tester.ensureVisible(cache);
      await tester.enterText(cache, '600');
      await tester.ensureVisible(find.text('Hell'));
      await tester.tap(find.text('Hell'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Log'));
      await tester.pumpAndSettle();
      expect(find.byTooltip('Logpfad anzeigen'), findsNothing);
      expect(find.byTooltip('Log speichern'), findsNothing);
      expect(find.byIcon(Icons.folder_outlined), findsNothing);
      await tester.tap(find.byType(DropdownButtonFormField<LogLevelPreference>));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Debug').last);
      await tester.pumpAndSettle();
      expect(saved, isEmpty);
      await tester.tap(find.text('Speichern'));
      await tester.pumpAndSettle();
      expect(saved, hasLength(1));
      expect(saved.single.localCacheSizeBytes, 600 * 1024 * 1024);
      expect(saved.single.isDarkTheme, isFalse);
      expect(saved.single.logLevel, LogLevelPreference.debug);
      expect(tester.takeException(), isNull);
    });
  }
}
