import 'package:flutter/material.dart';

import 'app/logging/app_logger.dart';
import 'app/shell/app_shell.dart';

void main() {
  AppLogger.configureLevel(
    const String.fromEnvironment('MASHIN_LOG_LEVEL', defaultValue: 'info'),
  );
  runApp(const MashinApp());
}
