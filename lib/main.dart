import 'dart:async';

import 'package:flutter/material.dart';

import 'app/session/app_session_controller.dart';
import 'app/shell/app_shell.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final session = AppSessionController();
  runApp(MashinApp(session: session));
  unawaited(session.initialize());
}
