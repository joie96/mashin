import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/logging/app_log_buffer.dart';

void main() {
  test('AppLogBuffer keeps only its newest 200 lines', () {
    final buffer = AppLogBuffer.shared;
    for (var index = 0; index < 205; index++) {
      buffer.add('line $index');
    }

    expect(buffer.lines, hasLength(200));
    expect(buffer.lines.first, 'line 5');
    expect(buffer.lines.last, 'line 204');
  });
}
