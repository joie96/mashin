import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/shared/context_menu/context_menu_controller.dart';

void main() {
  for (final viewportWidth in <double>[390, 1200]) {
    for (final contentHeight in <double>[260, 340]) {
      testWidgets(
        'custom menu fits $contentHeight pixels of content at width $viewportWidth',
        (WidgetTester tester) async {
          tester.view.physicalSize = Size(viewportWidth, 800);
          tester.view.devicePixelRatio = 1;
          addTearDown(tester.view.resetPhysicalSize);
          addTearDown(tester.view.resetDevicePixelRatio);

          const menuKey = ValueKey<String>('custom-menu');
          await tester.pumpWidget(
            MaterialApp(
              theme: AppTheme.dark(),
              home: Scaffold(
                body: Align(
                  alignment: Alignment.bottomRight,
                  child: Builder(
                    builder: (BuildContext anchorContext) => IconButton(
                      icon: const Icon(Icons.tune),
                      onPressed: () {
                        const AppContextMenuController().showCustomContextMenu(
                          context: anchorContext,
                          anchorContext: anchorContext,
                          width: 280,
                          child: Material(
                            key: menuKey,
                            child: Column(
                              mainAxisSize: MainAxisSize.min,
                              children: <Widget>[
                                SizedBox(height: contentHeight),
                              ],
                            ),
                          ),
                        );
                      },
                    ),
                  ),
                ),
              ),
            ),
          );

          await tester.tap(find.byIcon(Icons.tune));
          await tester.pumpAndSettle();

          final menuRect = tester.getRect(find.byKey(menuKey));
          expect(menuRect.size, Size(280, contentHeight));
          expect(menuRect.top, greaterThanOrEqualTo(0));
          expect(menuRect.bottom, lessThanOrEqualTo(800));
          expect(tester.takeException(), isNull);

          await tester.tapAt(const Offset(10, 10));
          await tester.pumpAndSettle();
          expect(find.byKey(menuKey), findsNothing);
        },
      );
    }
  }
}
