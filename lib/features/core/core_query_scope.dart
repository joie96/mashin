import 'dart:async';

class CoreQueryCanceled implements Exception {
  const CoreQueryCanceled();

  @override
  String toString() => 'Core query canceled';
}

/// Owns read requests for one visible page, including native cancellation.
class CoreQueryScope {
  static final Object _zoneKey = Object();
  final Set<void Function()> _cancelRequests = {};
  bool _isCanceled = false;

  static CoreQueryScope? get current =>
      Zone.current[_zoneKey] as CoreQueryScope?;
  bool get isCanceled => _isCanceled;

  // Owners with a separate delivery lifetime unregister explicitly after consumption.
  void Function() registerCancellation(void Function() onCancel) {
    if (_isCanceled) {
      onCancel();
      return () {};
    }
    _cancelRequests.add(onCancel);
    return () => _cancelRequests.remove(onCancel);
  }

  // Async continuations retain their original scope when the page starts a new load.
  Future<T> run<T>(Future<T> Function() operation) =>
      runZoned(operation, zoneValues: {_zoneKey: this});

  // Replaceable branches inherit cancellation and detach when they are retired.
  CoreQueryScope createChild() {
    final child = CoreQueryScope();
    if (_isCanceled) {
      child.cancel();
      return child;
    }
    void cancelChild() => child.cancel();
    _cancelRequests.add(cancelChild);
    child._cancelRequests.add(() => _cancelRequests.remove(cancelChild));
    return child;
  }

  void cancel() {
    if (_isCanceled) return;
    _isCanceled = true;
    for (final cancel in _cancelRequests.toList()) {
      cancel();
    }
  }

  // Every terminal path releases the timer and registration; late results are consumed.
  Future<T> execute<T>(
    Future<T> Function() operation, {
    required void Function() onCancel,
    Duration timeout = const Duration(seconds: 50),
  }) {
    if (_isCanceled) return Future<T>.error(const CoreQueryCanceled());
    final result = Completer<T>();
    late final Timer timer;
    late final void Function() cancelRequest;

    void finishError(
      Object error,
      StackTrace stackTrace, {
      bool abort = false,
    }) {
      if (result.isCompleted) return;
      timer.cancel();
      _cancelRequests.remove(cancelRequest);
      result.completeError(error, stackTrace);
      if (abort) onCancel();
    }

    cancelRequest = () =>
        finishError(const CoreQueryCanceled(), StackTrace.current, abort: true);
    timer = Timer(
      timeout,
      () => finishError(
        TimeoutException('Core query exceeded its total deadline', timeout),
        StackTrace.current,
        abort: true,
      ),
    );
    _cancelRequests.add(cancelRequest);
    Future<T>.sync(operation).then(
      (value) {
        if (result.isCompleted) return;
        timer.cancel();
        _cancelRequests.remove(cancelRequest);
        result.complete(value);
      },
      onError: (Object error, StackTrace stackTrace) =>
          finishError(error, stackTrace),
    );
    return result.future;
  }
}
