import 'dart:io';

import 'package:flutter/services.dart';

abstract final class AppUpdateInstaller {
  // The detached Windows installer must survive termination of this process.
  static Future<void> install(File file) async {
    if (Platform.isWindows) {
      final windowsDirectory = Platform.environment['SystemRoot'];
      if (windowsDirectory == null) {
        throw StateError('Das Windows-Verzeichnis wurde nicht gefunden.');
      }
      await Process.start(
        '$windowsDirectory\\System32\\msiexec.exe',
        ['/i', file.path],
        mode: ProcessStartMode.detached,
      );
      exit(0);
    }
    if (Platform.isAndroid) {
      await const MethodChannel(
        'mashin/updates',
      ).invokeMethod<void>('installApk', {'path': file.path});
      return;
    }
    throw UnsupportedError(
      'Updates werden auf dieser Plattform nicht unterstützt.',
    );
  }
}
