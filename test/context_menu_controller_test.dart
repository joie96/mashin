import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/theme/app_theme.dart';
import 'package:mashin_flutter/features/shared/context_menu/context_menu_controller.dart';

void main() {
  for (final viewportHeight in <double>[240, 800]) {
    testWidgets(
      'right-click submenu fits and remains selectable at height $viewportHeight',
      (WidgetTester tester) async {
        tester.view.physicalSize = Size(1200, viewportHeight);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);

        int? selectedAction;
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark(),
            home: Scaffold(
              body: Center(
                child: Builder(
                  builder: (BuildContext context) => GestureDetector(
                    onSecondaryTapDown: (TapDownDetails details) {
                      const AppContextMenuController().showContextMenu(
                        context: context,
                        globalPosition: details.globalPosition,
                        actions: <ContextMenuAction>[
                          ContextMenuAction.submenu(
                            label: 'Submenu',
                            icon: Icons.more_horiz,
                            children: List<ContextMenuAction>.generate(
                              12,
                              (int index) => ContextMenuAction(
                                label: 'Child $index',
                                icon: Icons.play_arrow,
                                onSelected: () => selectedAction = index,
                              ),
                            ),
                          ),
                        ],
                      );
                    },
                    child: const Text('Open menu'),
                  ),
                ),
              ),
            ),
          ),
        );

        final mouse = await tester.createGesture(
          buttons: kSecondaryMouseButton,
          kind: PointerDeviceKind.mouse,
        );
        await mouse.addPointer(location: Offset.zero);
        await mouse.down(tester.getCenter(find.text('Open menu')));
        await mouse.up();
        await tester.pumpAndSettle();

        await mouse.moveTo(tester.getCenter(find.text('Submenu')));
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);

        final submenu = find
            .ancestor(of: find.text('Child 0'), matching: find.byType(Material))
            .last;
        final submenuRect = tester.getRect(submenu);
        expect(submenuRect.top, greaterThanOrEqualTo(8));
        expect(submenuRect.bottom, lessThanOrEqualTo(viewportHeight - 8));

        await tester.drag(find.text('Child 0'), const Offset(0, -700));
        await tester.pumpAndSettle();
        await tester.tap(find.text('Child 11'));
        await tester.pumpAndSettle();
        expect(selectedAction, 11);
        expect(tester.takeException(), isNull);

        await mouse.removePointer();
        await tester.pumpWidget(const SizedBox.shrink());
        await tester.pumpAndSettle();
      },
    );
  }

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
