import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/settings/settings_listenbrainz_form.dart';
import 'package:mashin_flutter/features/settings/settings_screen_desktop.dart';
import 'package:mashin_flutter/features/settings/settings_screen_mobile.dart';
import 'package:mashin_flutter/features/settings/settings_screen.dart';

void main() {
  for (final isCompact in <bool>[true, false]) {
    testWidgets(
      'ListenBrainz settings render in ${isCompact ? 'mobile' : 'desktop'} layout',
      (tester) async {
        tester.view.physicalSize = isCompact
            ? const Size(360, 640)
            : const Size(1200, 800);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final cacheController = TextEditingController(text: '250');
        addTearDown(cacheController.dispose);
        final Widget screen = isCompact
            ? SettingsScreenMobile(
                musicAssistantUrl: 'https://ma.example',
                cacheSizeController: cacheController,
                isDarkTheme: true,
                logLevel: LogLevelPreference.info,
                onThemeChanged: (_) {},
                onLogLevelChanged: (_) {},
                onSaveLogSnapshot: () async => '',
                listenBrainzCredentials: const ListenBrainzCredentials(),
                onListenBrainzChanged: (_) {},
                onListenBrainzTest: (_) async {},
                onSave: () {},
                onCancel: () {},
                isSaving: false,
              )
            : SettingsScreenDesktop(
                musicAssistantUrl: 'https://ma.example',
                cacheSizeController: cacheController,
                isDarkTheme: true,
                logLevel: LogLevelPreference.info,
                onThemeChanged: (_) {},
                onLogLevelChanged: (_) {},
                onSaveLogSnapshot: () async => '',
                listenBrainzCredentials: const ListenBrainzCredentials(),
                onListenBrainzChanged: (_) {},
                onListenBrainzTest: (_) async {},
                onSave: () {},
                onCancel: () {},
                isSaving: false,
              );
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark(),
            home: Scaffold(body: screen),
          ),
        );
        await tester.pumpAndSettle();
        expect(find.text('ListenBrainz'), findsOneWidget);
        expect(find.byType(SettingsListenBrainzForm), findsOneWidget);
        expect(tester.takeException(), isNull);
      },
    );
  }

  testWidgets('conceals the stored token and tests trimmed draft credentials', (
    tester,
  ) async {
    ListenBrainzCredentials? saved;
    final result = Completer<void>();
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SettingsListenBrainzForm(
            credentials: const ListenBrainzCredentials(
              username: 'user',
              token: 'secret',
            ),
            onChanged: (_) {},
            onTest: (credentials) {
              saved = credentials;
              return result.future;
            },
          ),
        ),
      ),
    );
    expect(
      tester.widget<TextField>(find.byType(TextField).last).obscureText,
      isTrue,
    );
    await tester.enterText(find.byType(TextFormField).first, ' new-user ');
    await tester.enterText(find.byType(TextFormField).last, ' new-token ');
    await tester.tap(find.widgetWithText(OutlinedButton, 'Test'));
    await tester.pump();
    expect(saved?.username, 'new-user');
    expect(saved?.token, 'new-token');
    expect(
      tester
          .widget<OutlinedButton>(find.widgetWithText(OutlinedButton, 'Test'))
          .onPressed,
      isNull,
    );
    result.complete();
    await tester.pumpAndSettle();
    expect(find.byIcon(Icons.check), findsOneWidget);
  });

  testWidgets('connection test requires both credentials', (tester) async {
    ListenBrainzCredentials? saved;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SettingsListenBrainzForm(
            credentials: const ListenBrainzCredentials(),
            onChanged: (_) {},
            onTest: (credentials) async {
              saved = credentials;
            },
          ),
        ),
      ),
    );
    await tester.enterText(find.byType(TextFormField).first, 'user');
    await tester.tap(find.widgetWithText(OutlinedButton, 'Test'));
    await tester.pumpAndSettle();
    expect(saved, isNull);
    expect(find.text('Token erforderlich'), findsOneWidget);
    await tester.enterText(find.byType(TextFormField).first, '');
    await tester.tap(find.widgetWithText(OutlinedButton, 'Test'));
    await tester.pumpAndSettle();
    expect(saved, isNull);
    expect(find.text('Benutzername erforderlich'), findsOneWidget);
  });

  testWidgets('reports a failure without exposing the token', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SettingsListenBrainzForm(
            credentials: const ListenBrainzCredentials(
              username: 'user',
              token: 'secret',
            ),
            onChanged: (_) {},
            onTest: (_) async => throw StateError('secret'),
          ),
        ),
      ),
    );
    await tester.tap(find.widgetWithText(OutlinedButton, 'Test'));
    await tester.pumpAndSettle();
    expect(
      find.textContaining('Verbindungstest fehlgeschlagen'),
      findsOneWidget,
    );
    expect(
      find.byWidgetPredicate(
        (widget) => widget is Text && (widget.data ?? '').contains('secret'),
      ),
      findsNothing,
    );
  });

  for (final isCompact in <bool>[true, false]) {
    testWidgets(
      'draft changes test cancel and save in ${isCompact ? 'mobile' : 'desktop'} settings',
      (tester) async {
        tester.view.physicalSize = isCompact
            ? const Size(390, 800)
            : const Size(1200, 900);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final saved = <SessionSettingsUpdate>[];
        final tested = <ListenBrainzCredentials>[];
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
                  listenBrainzCredentials: ListenBrainzCredentials(
                    username: 'old-user',
                    token: 'old-token',
                  ),
                ),
                onSave: (update) async {
                  saved.add(update);
                },
                onTestListenBrainz: (credentials) async {
                  tested.add(credentials);
                },
                onSaveLogSnapshot: () async => '',
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        final username = find.byKey(const Key('listenbrainz-username'));
        final token = find.byKey(const Key('listenbrainz-token'));
        final cache = find.byKey(const Key('settings-cache-size'));
        if (!isCompact) {
          final fields = tester.widgetList<TextField>(
            find.descendant(
              of: find.byType(SettingsListenBrainzForm),
              matching: find.byType(TextField),
            ),
          );
          expect(
            fields.every((field) => field.decoration?.labelText == null),
            isTrue,
          );
          expect(
            tester.getTopLeft(find.text('Benutzername')).dx,
            lessThan(tester.getTopLeft(username).dx),
          );
        }
        await tester.ensureVisible(username);
        await tester.enterText(username, 'new-user');
        await tester.enterText(token, 'new-token');
        await tester.ensureVisible(find.widgetWithText(OutlinedButton, 'Test'));
        await tester.tap(find.widgetWithText(OutlinedButton, 'Test'));
        await tester.pumpAndSettle();
        expect(tested.single.username, 'new-user');
        expect(
          find.descendant(
            of: find.byType(SettingsListenBrainzForm),
            matching: find.byIcon(Icons.check),
          ),
          findsOneWidget,
        );
        expect(saved, isEmpty);
        await tester.ensureVisible(cache);
        await tester.enterText(cache, '500');
        await tester.ensureVisible(find.text('Hell'));
        await tester.tap(find.text('Hell'));
        await tester.pumpAndSettle();
        expect(saved, isEmpty);
        await tester.tap(find.text('Abbrechen'));
        await tester.pumpAndSettle();
        expect(tester.widget<TextField>(cache).controller!.text, '250');
        await tester.ensureVisible(username);
        expect(
          tester
              .widget<TextField>(
                find.descendant(of: username, matching: find.byType(TextField)),
              )
              .controller!
              .text,
          'old-user',
        );
        expect(
          find.descendant(
            of: find.byType(SettingsListenBrainzForm),
            matching: find.byIcon(Icons.check),
          ),
          findsNothing,
        );
        await tester.enterText(username, 'saved-user');
        await tester.enterText(token, 'saved-token');
        await tester.ensureVisible(cache);
        await tester.enterText(cache, '600');
        await tester.ensureVisible(find.text('Hell'));
        await tester.tap(find.text('Hell'));
        await tester.pumpAndSettle();
        await tester.tap(find.text('Log'));
        await tester.pumpAndSettle();
        await tester.tap(
          find.byType(DropdownButtonFormField<LogLevelPreference>),
        );
        await tester.pumpAndSettle();
        await tester.tap(find.text('Debug').last);
        await tester.pumpAndSettle();
        expect(saved, isEmpty);
        await tester.tap(find.text('Speichern'));
        await tester.pumpAndSettle();
        expect(saved, hasLength(1));
        expect(saved.single.localCacheSizeBytes, 600 * 1024 * 1024);
        expect(saved.single.listenBrainzCredentials.username, 'saved-user');
        expect(saved.single.isDarkTheme, isFalse);
        expect(saved.single.logLevel, LogLevelPreference.debug);
      },
    );
  }

  testWidgets('cancel invalidates a connection test still in flight', (
    tester,
  ) async {
    final completion = Completer<void>();
    var credentials = const ListenBrainzCredentials(
      username: 'draft',
      token: 'draft-token',
    );
    late StateSetter rebuild;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: StatefulBuilder(
            builder: (context, setState) {
              rebuild = setState;
              return SettingsListenBrainzForm(
                credentials: credentials,
                onChanged: (value) => setState(() => credentials = value),
                onTest: (_) => completion.future,
              );
            },
          ),
        ),
      ),
    );
    await tester.tap(find.widgetWithText(OutlinedButton, 'Test'));
    await tester.pump();
    rebuild(() => credentials = const ListenBrainzCredentials());
    await tester.pump();
    completion.complete();
    await tester.pumpAndSettle();
    expect(find.byIcon(Icons.check), findsNothing);
    expect(
      tester.widget<TextField>(find.byType(TextField).first).controller!.text,
      isEmpty,
    );
  });
}
