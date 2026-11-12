import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/features/core/core_query_scope.dart';

void main() {
  test(
    'cancellation registrations outlive delivery and can be released',
    () async {
      final scope = CoreQueryScope();
      var canceled = 0;
      final unregister = scope.registerCancellation(() => canceled++);
      unregister();
      scope.registerCancellation(() => canceled++);
      scope.cancel();
      scope.cancel();
      expect(canceled, 1);
      scope.registerCancellation(() => canceled++);
      expect(canceled, 2);
    },
  );
  test(
    'child cancellation is isolated and parent cancellation reaches descendants',
    () async {
      final parent = CoreQueryScope();
      final child = parent.createChild();
      final sibling = parent.createChild();
      final descendant = sibling.createChild();
      var childAborts = 0;
      var descendantAborts = 0;
      final pendingChild = Completer<void>();
      final pendingDescendant = Completer<void>();
      final childResult = child.execute(
        () => pendingChild.future,
        onCancel: () => childAborts++,
      );
      final descendantResult = descendant.execute(
        () => pendingDescendant.future,
        onCancel: () => descendantAborts++,
      );
      final childCanceled = expectLater(
        childResult,
        throwsA(isA<CoreQueryCanceled>()),
      );
      final descendantCanceled = expectLater(
        descendantResult,
        throwsA(isA<CoreQueryCanceled>()),
      );
      child.cancel();
      await childCanceled;
      expect(parent.isCanceled, isFalse);
      expect(sibling.isCanceled, isFalse);
      expect(descendantAborts, 0);
      parent.cancel();
      parent.cancel();
      await descendantCanceled;
      expect(sibling.isCanceled, isTrue);
      expect(descendant.isCanceled, isTrue);
      expect(childAborts, 1);
      expect(descendantAborts, 1);
      pendingChild.completeError(StateError('late child'));
      pendingDescendant.completeError(StateError('late descendant'));
    },
  );

  test(
    'children of retired scopes start no work and retain their own async zone',
    () async {
      final parent = CoreQueryScope();
      final child = parent.createChild();
      await child.run(() async {
        await Future<void>.value();
        expect(CoreQueryScope.current, same(child));
      });
      parent.cancel();
      final retired = parent.createChild();
      expect(retired.isCanceled, isTrue);
      await expectLater(
        retired.execute(() async => fail('must not start'), onCancel: () {}),
        throwsA(isA<CoreQueryCanceled>()),
      );
    },
  );

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
