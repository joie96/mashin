// This is a basic Flutter widget test.
//
// To perform an interaction with a widget in your test, use the WidgetTester
// utility in the flutter_test package. For example, you can send tap and scroll
// gestures. You can also use WidgetTester to find child widgets in the widget
// tree, read text, and verify that the values of widget properties are correct.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:mashin_flutter/app/shell/app_shell.dart';

void main() {
  testWidgets('shows login screen when no session exists', (WidgetTester tester) async {
    SharedPreferences.setMockInitialValues(<String, Object>{});

    await tester.pumpWidget(const MashinApp());
    await tester.pumpAndSettle();

    expect(find.text('Bei Music Assistant anmelden'), findsOneWidget);
    expect(find.byType(TextFormField), findsNWidgets(3));
    expect(find.widgetWithText(FilledButton, 'Anmelden'), findsOneWidget);
  });
}
