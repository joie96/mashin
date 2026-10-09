import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/settings/settings_listenbrainz_form.dart';
import 'package:mashin_flutter/features/settings/settings_screen_desktop.dart';
import 'package:mashin_flutter/features/settings/settings_screen_mobile.dart';

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
                onCacheSizeChanged: (_) {},
                onThemeChanged: (_) {},
                onLogLevelChanged: (_) {},
                onSaveLogSnapshot: () async => '',
                listenBrainzCredentials: const ListenBrainzCredentials(),
                onListenBrainzSave: (_) async {},
              )
            : SettingsScreenDesktop(
                musicAssistantUrl: 'https://ma.example',
                cacheSizeController: cacheController,
                isDarkTheme: true,
                logLevel: LogLevelPreference.info,
                onCacheSizeChanged: (_) {},
                onThemeChanged: (_) {},
                onLogLevelChanged: (_) {},
                onSaveLogSnapshot: () async => '',
                listenBrainzCredentials: const ListenBrainzCredentials(),
                onListenBrainzSave: (_) async {},
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

  testWidgets('conceals the stored token and saves trimmed credentials', (
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
            onSave: (credentials) {
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
    await tester.tap(find.byTooltip('ListenBrainz speichern'));
    await tester.pump();
    expect(saved?.username, 'new-user');
    expect(saved?.token, 'new-token');
    expect(
      tester
          .widget<IconButton>(
            find.byWidgetPredicate(
              (widget) =>
                  widget is IconButton &&
                  widget.tooltip == 'ListenBrainz speichern',
            ),
          )
          .onPressed,
      isNull,
    );
    result.complete();
    await tester.pumpAndSettle();
    expect(find.text('Scrobbling aktiviert'), findsOneWidget);
  });

  testWidgets('requires both credentials and allows clearing both to disable', (
    tester,
  ) async {
    ListenBrainzCredentials? saved;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SettingsListenBrainzForm(
            credentials: const ListenBrainzCredentials(),
            onSave: (credentials) async {
              saved = credentials;
            },
          ),
        ),
      ),
    );
    await tester.enterText(find.byType(TextFormField).first, 'user');
    await tester.tap(find.byTooltip('ListenBrainz speichern'));
    await tester.pumpAndSettle();
    expect(saved, isNull);
    expect(find.text('Token erforderlich'), findsOneWidget);
    await tester.enterText(find.byType(TextFormField).first, '');
    await tester.tap(find.byTooltip('ListenBrainz speichern'));
    await tester.pumpAndSettle();
    expect(saved?.isConfigured, isFalse);
    expect(find.text('Scrobbling deaktiviert'), findsOneWidget);
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
            onSave: (_) async => throw StateError('secret'),
          ),
        ),
      ),
    );
    await tester.tap(find.byTooltip('ListenBrainz speichern'));
    await tester.pumpAndSettle();
    expect(find.textContaining('Speichern fehlgeschlagen'), findsOneWidget);
    expect(
      find.byWidgetPredicate(
        (widget) => widget is Text && (widget.data ?? '').contains('secret'),
      ),
      findsNothing,
    );
  });
}
