import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/logging/app_log_buffer.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/features/settings/settings_log_view.dart';

void main() {
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
