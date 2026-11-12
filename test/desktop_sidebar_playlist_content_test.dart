import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/shell/app_shell.dart';
import 'package:mashin/app/theme/app_theme.dart';

void main() {
  for (final scale in <double>[1, 2, 3]) {
    testWidgets('playlist prototype fits text at scale $scale', (tester) async {
      final controller = ScrollController();
      addTearDown(controller.dispose);

      Widget row(String title, String metadata, {Key? key}) => Padding(
        key: key,
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
        child: DesktopSidebarPlaylistContent(
          title: title,
          metadata: metadata,
          cover: const SizedBox.shrink(),
        ),
      );

      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.light(),
          home: MediaQuery(
            data: MediaQueryData(textScaler: TextScaler.linear(scale)),
            child: Scaffold(
              body: SizedBox(
                width: 250,
                height: 300,
                child: ListView.builder(
                  controller: controller,
                  padding: const EdgeInsets.symmetric(horizontal: 10),
                  prototypeItem: row('Playlist', '0 Titel'),
                  itemCount: 100,
                  itemBuilder: (_, index) => row(
                    'Playlist $index with a long title',
                    '1000 Titel • 100 Std. 59 Min.',
                    key: ValueKey(index),
                  ),
                ),
              ),
            ),
          ),
        ),
      );

      final firstRow = find.byKey(const ValueKey(0));
      final rowHeight = tester.getSize(firstRow).height;
      if (scale == 1) {
        expect(rowHeight, 52);
      } else {
        expect(rowHeight, greaterThan(52));
      }
      for (final text in tester.widgetList<Text>(
        find.descendant(of: firstRow, matching: find.byType(Text)),
      )) {
        final bounds = tester.getRect(find.byWidget(text));
        final rowBounds = tester.getRect(firstRow);
        expect(bounds.top, greaterThanOrEqualTo(rowBounds.top + 6));
        expect(bounds.bottom, lessThanOrEqualTo(rowBounds.bottom - 6));
      }
      expect(
        controller.position.maxScrollExtent,
        closeTo(100 * rowHeight - 300, 0.001),
      );
      controller.jumpTo(controller.position.maxScrollExtent);
      await tester.pump();
      expect(find.byKey(const ValueKey(99)), findsOneWidget);
      expect(tester.getSize(find.byKey(const ValueKey(99))).height, rowHeight);
      expect(tester.takeException(), isNull);
    });
  }
}
