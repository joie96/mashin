import 'package:flutter/foundation.dart';
import 'package:flutter/scheduler.dart';

class SelectionSessionRegistration {
  final bool allSelected;
  final VoidCallback onToggleAll;
  final VoidCallback? onMenuTap;
  final VoidCallback onClose;

  const SelectionSessionRegistration({
    required this.allSelected,
    required this.onToggleAll,
    required this.onMenuTap,
    required this.onClose,
  });
}

class SelectionSessionController extends ChangeNotifier {
  SelectionSessionController._();

  static final SelectionSessionController instance = SelectionSessionController._();

  Object? _activeOwner;
  SelectionSessionRegistration? _activeRegistration;

  bool get isVisible => _activeOwner != null && _activeRegistration != null;

  bool get allSelected => _activeRegistration?.allSelected ?? false;

  void _runAfterBuild(VoidCallback action) {
    final SchedulerPhase phase = SchedulerBinding.instance.schedulerPhase;
    if (phase == SchedulerPhase.idle || phase == SchedulerPhase.postFrameCallbacks) {
      action();
      return;
    }

    SchedulerBinding.instance.addPostFrameCallback((_) {
      action();
    });
  }

  void _notifyListenersSafely() {
    _runAfterBuild(notifyListeners);
  }

  void activate({
    required Object owner,
    required SelectionSessionRegistration registration,
  }) {
    final bool ownerChanged = !identical(_activeOwner, owner);
    final SelectionSessionRegistration? previousRegistration = _activeRegistration;
    final bool registrationChanged =
        previousRegistration == null ||
        previousRegistration.allSelected != registration.allSelected ||
        previousRegistration.onMenuTap != registration.onMenuTap;

    if (!ownerChanged && !registrationChanged) {
      return;
    }

    _activeOwner = owner;
    _activeRegistration = registration;

    if (ownerChanged) {
      final VoidCallback? clearPreviousSelection = previousRegistration?.onClose;
      if (clearPreviousSelection != null) {
        _runAfterBuild(clearPreviousSelection);
      }
    }

    _notifyListenersSafely();
  }

  void deactivate(Object owner) {
    if (!identical(_activeOwner, owner)) {
      return;
    }

    _activeOwner = null;
    _activeRegistration = null;
    _notifyListenersSafely();
  }

  void onToggleAll() {
    _activeRegistration?.onToggleAll();
  }

  void onMenuTap() {
    _activeRegistration?.onMenuTap?.call();
  }

  void onClose() {
    _activeRegistration?.onClose();
  }
}