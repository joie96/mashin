import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';

void main() {
  for (final isDark in <bool>[true, false]) {
    for (final isIconButton in <bool>[true, false]) {
      testWidgets(
        '${isDark ? 'dark' : 'light'} ${isIconButton ? 'icon button' : 'tooltip'} waits before showing hover text',
        (tester) async {
          final ThemeData theme = isDark ? AppTheme.dark() : AppTheme.light();
          expect(theme.tooltipTheme.waitDuration, AppTheme.tooltipWaitDuration);
          await tester.pumpWidget(
            MaterialApp(
              theme: theme,
              home: Scaffold(
                body: Center(
                  child: isIconButton
                      ? IconButton(
                          tooltip: 'Tooltip text',
                          onPressed: () {},
                          icon: const Icon(Icons.info_outline),
                        )
                      : const Tooltip(
                          message: 'Tooltip text',
                          child: SizedBox(width: 40, height: 40),
                        ),
                ),
              ),
            ),
          );
          final TestGesture mouse = await tester.createGesture(
            kind: PointerDeviceKind.mouse,
          );
          await mouse.addPointer(location: Offset.zero);
          final Offset target = tester.getCenter(find.byType(Tooltip));
          await mouse.moveTo(target);
          await tester.pump();
          await tester.pump(const Duration(milliseconds: 300));
          expect(find.text('Tooltip text'), findsNothing);

          await mouse.moveTo(Offset.zero);
          await tester.pump(AppTheme.tooltipWaitDuration);
          expect(find.text('Tooltip text'), findsNothing);

          await mouse.moveTo(target);
          await tester.pump();
          await tester.pump(
            AppTheme.tooltipWaitDuration - const Duration(milliseconds: 1),
          );
          expect(find.text('Tooltip text'), findsNothing);
          await tester.pump(const Duration(milliseconds: 1));
          await tester.pumpAndSettle();
          expect(find.text('Tooltip text'), findsOneWidget);
          await mouse.removePointer();
          await tester.pumpAndSettle();
        },
      );
    }
  }
}
