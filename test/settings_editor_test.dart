import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/app/updates/app_update_dialog.dart';
import 'package:mashin/app/updates/app_update_service.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/settings/settings_info_view.dart';
import 'package:mashin/features/settings/settings_screen.dart';
import 'package:pub_semver/pub_semver.dart';

void main() {
  for (final width in [320.0, 390.0, 720.0, 1200.0]) {
    testWidgets('Info shows app metadata and reuses update overlay at $width', (
      tester,
    ) async {
      tester.view.physicalSize = Size(width, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final service = _InfoUpdateService();
      Uri? projectUri;
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.dark(),
          home: Scaffold(
            body: SettingsInfoView(
              service: service,
              openProjectLink: (uri) async {
                projectUri = uri;
                return true;
              },
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('Mashin'), findsNothing);
      expect(find.text('Music Assistant Client'), findsNothing);
      expect(find.text('0.0.1-dev.7'), findsOneWidget);
      expect(find.textContaining('Zuletzt geprüft:'), findsNothing);
      for (final entry in <String, Finder>{
        'Version': find.text('0.0.1-dev.7'),
        'Projekt': find.widgetWithText(TextButton, 'github.com/joie96/mashin'),
        'Updates': find.widgetWithText(OutlinedButton, 'Update suchen'),
      }.entries) {
        final labelRect = tester.getRect(find.text(entry.key));
        final valueRect = tester.getRect(entry.value);
        if (width >= 720) {
          expect(valueRect.left, greaterThan(labelRect.right));
          expect(valueRect.center.dy, closeTo(labelRect.center.dy, 2));
          if (entry.key == 'Updates') {
            expect(valueRect.right, greaterThan(width * .8));
          } else {
            expect(valueRect.left - labelRect.left, closeTo(144, 2));
            expect(valueRect.right, lessThan(width - 24));
          }
        } else {
          expect(valueRect.top, greaterThan(labelRect.bottom));
        }
      }
      await tester.tap(find.text('github.com/joie96/mashin'));
      expect(projectUri.toString(), 'https://github.com/joie96/mashin');
      expect(find.widgetWithText(FilledButton, 'Update'), findsNothing);
      await tester.tap(find.text('Update suchen'));
      await tester.pumpAndSettle();
      expect(service.checkCount, 1);
      expect(find.byType(AppUpdateDialog), findsOneWidget);
      await tester.tap(find.text('Abbrechen'));
      await tester.pumpAndSettle();
      expect(find.textContaining('Zuletzt geprüft:'), findsOneWidget);
      await tester.tap(find.text('Update suchen'));
      await tester.pumpAndSettle();
      expect(find.byType(AppUpdateDialog), findsOneWidget);
      expect(service.checkCount, 2);
      await tester.tap(find.text('Abbrechen'));
      await tester.pumpAndSettle();
      service.shouldOfferUpdate = false;
      await tester.tap(find.text('Update suchen'));
      await tester.pumpAndSettle();
      expect(service.checkCount, 3);
      expect(find.byType(AppUpdateDialog), findsNothing);
      expect(find.text('Mashin ist auf dem neuesten Stand.'), findsOneWidget);
      final lastCheckedStatus = tester
          .widget<Text>(find.textContaining('Zuletzt geprüft:'))
          .data!;
      tester
          .state<ScaffoldMessengerState>(find.byType(ScaffoldMessenger))
          .removeCurrentSnackBar();
      await tester.pumpAndSettle();
      service.shouldFailCheck = true;
      await tester.tap(find.text('Update suchen'));
      await tester.pumpAndSettle();
      expect(
        find.textContaining('Update-Prüfung fehlgeschlagen'),
        findsOneWidget,
      );
      expect(find.text(lastCheckedStatus), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }

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
      'logout is available only in the mobile server section $isCompact',
      (tester) async {
        final completion = Completer<void>();
        int logoutCount = 0;
        await _pumpEditor(
          tester,
          isCompact,
          (_) async {},
          onLogout: () {
            logoutCount++;
            return completion.future;
          },
        );
        final button = find.widgetWithText(OutlinedButton, 'Abmelden');
        if (!isCompact) {
          expect(button, findsNothing);
          return;
        }
        expect(button, findsOneWidget);
        expect(
          tester.getRect(button).top,
          greaterThan(tester.getRect(find.byType(TextFormField)).bottom),
        );
        expect(
          tester.getRect(button).bottom,
          lessThan(tester.getRect(find.text('Lokaler Cache')).top),
        );
        await tester.tap(button);
        await tester.pump();
        expect(logoutCount, 1);
        expect(tester.widget<OutlinedButton>(button).onPressed, isNull);
        completion.completeError(StateError('storage unavailable'));
        await tester.pumpAndSettle();
        expect(
          find.text('Abmelden fehlgeschlagen. Bitte erneut versuchen.'),
          findsOneWidget,
        );
        expect(tester.widget<OutlinedButton>(button).onPressed, isNotNull);
      },
    );

    testWidgets(
      'Info tab is available in ${isCompact ? 'mobile' : 'desktop'} settings',
      (tester) async {
        await _pumpEditor(tester, isCompact, (_) async {});
        await tester.tap(find.text('Info'));
        await tester.pumpAndSettle();
        expect(find.byType(SettingsInfoView), findsOneWidget);
        expect(find.text('Mashin'), findsNothing);
        expect(find.text('Version'), findsOneWidget);
        expect(find.text('github.com/joie96/mashin'), findsOneWidget);
        expect(find.text('Update suchen'), findsOneWidget);
        expect(tester.takeException(), isNull);
      },
    );

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
        if (isCompact) {
          final transform = tester.widget<Transform>(
            find.descendant(of: toggle, matching: find.byType(Transform)).first,
          );
          expect(transform.transform.entry(0, 0), .7);
          expect(transform.transform.entry(1, 1), .7);
        }
        if (!isCompact) {
          final transform = tester.widget<Transform>(
            find.ancestor(of: toggle, matching: find.byType(Transform)).first,
          );
          expect(transform.transform.entry(0, 0), .7);
          expect(transform.transform.entry(1, 1), .7);
          final list = tester.widget<ListView>(
            find.ancestor(of: toggle, matching: find.byType(ListView)).first,
          );
          expect(list.padding, const EdgeInsets.only(top: 12, bottom: 32));
        }
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

class _InfoUpdateService extends AppUpdateService {
  int checkCount = 0;
  bool shouldOfferUpdate = true;
  bool shouldFailCheck = false;

  @override
  Future<Version> getInstalledVersion() async => Version.parse('0.0.1-dev.7');

  @override
  Future<AppUpdate?> checkForUpdate() async {
    checkCount++;
    if (shouldFailCheck) throw StateError('offline');
    if (!shouldOfferUpdate) return null;
    return AppUpdate(
      version: Version.parse('0.0.1-dev.10'),
      notes: "## What's Changed\n* Update test",
      fileName: 'update.msi',
      downloadUrl: Uri.https('github.com', '/installer'),
      checksumUrl: Uri.https('github.com', '/checksum'),
      size: 3,
    );
  }
}

// Shared themed host for both responsive settings layouts.
Future<void> _pumpEditor(
  WidgetTester tester,
  bool isCompact,
  Future<void> Function(SessionSettingsUpdate) onSave, {
  Future<void> Function()? onLogout,
}) async {
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
          onLogout: onLogout ?? () async {},
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}
