import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/features/core/core_query_scope.dart';

void main() {
  test(
    'cancel aborts only owned requests and consumes late failures',
    () async {
      final first = CoreQueryScope();
      final second = CoreQueryScope();
      final pending = Completer<int>();
      var aborts = 0;
      final result = first.execute(
        () => pending.future,
        onCancel: () => aborts++,
      );
      final canceled = expectLater(result, throwsA(isA<CoreQueryCanceled>()));
      first.cancel();
      first.cancel();
      await canceled;
      pending.completeError(StateError('late response'));
      expect(
        await second.execute(() async => 42, onCancel: () => aborts++),
        42,
      );
      expect(aborts, 1);
    },
  );

  test('deadline aborts the underlying request', () async {
    final scope = CoreQueryScope();
    var wasAborted = false;
    await expectLater(
      scope.execute(
        () => Completer<void>().future,
        onCancel: () => wasAborted = true,
        timeout: const Duration(milliseconds: 1),
      ),
      throwsA(isA<TimeoutException>()),
    );
    expect(wasAborted, isTrue);
  });

  test(
    'completed requests are not aborted and canceled scopes start no work',
    () async {
      final scope = CoreQueryScope();
      var aborts = 0;
      expect(await scope.execute(() async => 1, onCancel: () => aborts++), 1);
      scope.cancel();
      await expectLater(
        scope.execute(
          () async => fail('must not start'),
          onCancel: () => aborts++,
        ),
        throwsA(isA<CoreQueryCanceled>()),
      );
      expect(aborts, 0);
    },
  );

  test('async continuations retain the owning scope', () async {
    final scope = CoreQueryScope();
    await scope.run(() async {
      await Future<void>.value();
      expect(CoreQueryScope.current, same(scope));
    });
    expect(CoreQueryScope.current, isNull);
  });
}
