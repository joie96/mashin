import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/logging/app_log_buffer.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/settings/settings_log_view.dart';

void main() {
  testWidgets('save button waits for Rust result and shows the file path', (
    WidgetTester tester,
  ) async {
    final result = Completer<String>();
    var saveCount = 0;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SettingsSaveLogButton(
            onSave: () {
              saveCount++;
              return result.future;
            },
          ),
        ),
      ),
    );

    await tester.tap(find.byTooltip('Log speichern'));
    await tester.pump();
    expect(saveCount, 1);
    expect(
      tester.widget<IconButton>(find.byType(IconButton)).onPressed,
      isNull,
    );
    expect(find.byIcon(Icons.save_outlined), findsOneWidget);

    result.complete('C:\\Logs\\mashin.log');
    await tester.pump();
    expect(find.text('Log gespeichert: C:\\Logs\\mashin.log'), findsOneWidget);
    expect(
      tester.widget<IconButton>(find.byType(IconButton)).onPressed,
      isNotNull,
    );
    expect(find.byIcon(Icons.check), findsOneWidget);
    expect(tester.widget<Icon>(find.byIcon(Icons.check)).color, Colors.green);
    await tester.pump(const Duration(seconds: 2));
    expect(find.byIcon(Icons.save_outlined), findsOneWidget);
  });

  testWidgets('save button reports a Rust export failure', (
    WidgetTester tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SettingsSaveLogButton(
            onSave: () async => throw StateError('disk full'),
          ),
        ),
      ),
    );

    await tester.tap(find.byTooltip('Log speichern'));
    await tester.pump();
    expect(find.textContaining('disk full'), findsOneWidget);
    expect(find.byIcon(Icons.close), findsOneWidget);
    expect(
      tester.widget<Icon>(find.byIcon(Icons.close)).color,
      Theme.of(tester.element(find.byIcon(Icons.close))).colorScheme.error,
    );
    await tester.pump(const Duration(seconds: 2));
    expect(find.byIcon(Icons.save_outlined), findsOneWidget);
  });

  testWidgets('log follows new lines only while scrolled to the end', (
    WidgetTester tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.light(),
        home: const Scaffold(
          body: SizedBox(height: 300, child: SettingsLogOutput()),
        ),
      ),
    );

    for (var index = 0; index < 80; index++) {
      AppLogBuffer.shared.add('log entry $index');
    }
    await tester.pump();

    final controller = tester
        .widget<ListView>(find.byType(ListView))
        .controller!;
    expect(controller.position.maxScrollExtent, greaterThan(0));
    expect(controller.offset, controller.position.maxScrollExtent);

    await tester.drag(find.byType(ListView), const Offset(0, 200));
    await tester.pumpAndSettle();
    final manualOffset = controller.offset;
    expect(manualOffset, lessThan(controller.position.maxScrollExtent));

    AppLogBuffer.shared.add('new line while reading');
    await tester.pump();
    expect(controller.offset, manualOffset);

    await tester.drag(find.byType(ListView), const Offset(0, -2000));
    await tester.pumpAndSettle();
    expect(controller.offset, controller.position.maxScrollExtent);

    AppLogBuffer.shared.add('new line at the end');
    await tester.pump();
    expect(controller.offset, controller.position.maxScrollExtent);
  });
}
