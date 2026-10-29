import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/updates/app_update_installer.dart';

void main() {
  group('Windows installer arguments', () {
    test('normalizes mixed download path separators', () {
      final arguments = AppUpdateInstaller.windowsInstallerArguments(
        File(
          r'C:\Users\test\AppData\Local\Temp/mashin-updates/installer-123/Mashin-0.0.1-dev.8-windows-x64.msi',
        ),
      );

      expect(arguments, [
        '/i',
        r'C:\Users\test\AppData\Local\Temp\mashin-updates\installer-123\Mashin-0.0.1-dev.8-windows-x64.msi',
      ]);
    });

    test('preserves spaces and special characters as one argument', () {
      final arguments = AppUpdateInstaller.windowsInstallerArguments(
        File(r'C:\Users\Test User\Temp/update #1/Mashin update.msi'),
      );

      expect(arguments, [
        '/i',
        r'C:\Users\Test User\Temp\update #1\Mashin update.msi',
      ]);
    });

    test('resolves relative package paths before launching', () {
      final arguments = AppUpdateInstaller.windowsInstallerArguments(
        File('mashin-updates/installer-123/update.msi'),
      );

      expect(arguments[1], startsWith('${Directory.current.path}\\'));
      expect(
        arguments[1],
        endsWith(r'\mashin-updates\installer-123\update.msi'),
      );
      expect(arguments[1], isNot(contains('/')));
    });
  }, skip: !Platform.isWindows);
}