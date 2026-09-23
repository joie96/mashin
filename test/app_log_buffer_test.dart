import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/app/logging/app_log_buffer.dart';

void main() {
  test('AppLogBuffer keeps only its newest 10000 lines', () {
    final buffer = AppLogBuffer.shared;
    for (var index = 0; index < 10005; index++) {
      buffer.add('line $index');
    }

    expect(buffer.lines, hasLength(10000));
    expect(buffer.lines.first, 'line 5');
    expect(buffer.lines.last, 'line 10004');
  });
}
