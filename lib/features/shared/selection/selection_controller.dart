import 'package:flutter/foundation.dart';
import 'package:flutter/scheduler.dart';

class SelectionSessionRegistration {
  final bool allSelected;
  final int selectedCount;
  final String itemTypeLabel;
  final VoidCallback onToggleAll;
  final VoidCallback? onMenuTap;
  final VoidCallback onClose;

  const SelectionSessionRegistration({
    required this.allSelected,
    this.selectedCount = 0,
    this.itemTypeLabel = 'Elemente',
    required this.onToggleAll,
    required this.onMenuTap,
    required this.onClose,
  });
}

class SelectionController extends ChangeNotifier {
  SelectionController._();

  static final SelectionController instance = SelectionController._();

  Object? _activeOwner;
  SelectionSessionRegistration? _activeRegistration;

  bool get isVisible => _activeOwner != null && _activeRegistration != null;

  bool get allSelected => _activeRegistration?.allSelected ?? false;

  int get selectedCount => _activeRegistration?.selectedCount ?? 0;

  String get itemTypeLabel => _activeRegistration?.itemTypeLabel ?? 'Elemente';

  void _runAfterBuild(VoidCallback action) {
    final SchedulerPhase phase = SchedulerBinding.instance.schedulerPhase;
    if (phase == SchedulerPhase.idle ||
        phase == SchedulerPhase.postFrameCallbacks) {
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
    final SelectionSessionRegistration? previousRegistration =
        _activeRegistration;
    final bool registrationChanged =
        previousRegistration == null ||
        previousRegistration.allSelected != registration.allSelected ||
        previousRegistration.onMenuTap != registration.onMenuTap ||
        previousRegistration.selectedCount != registration.selectedCount ||
        previousRegistration.itemTypeLabel != registration.itemTypeLabel;

    if (!ownerChanged && !registrationChanged) {
      return;
    }

    _activeOwner = owner;
    _activeRegistration = registration;

    if (ownerChanged) {
      final VoidCallback? clearPreviousSelection =
          previousRegistration?.onClose;
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
    final Object? owner = _activeOwner;
    final VoidCallback? close = _activeRegistration?.onClose;
    if (owner == null || close == null) {
      return;
    }
    _runAfterBuild(() {
      if (identical(_activeOwner, owner)) {
        close();
      }
    });
  }
}

String selectionTypeLabel(Type? type, int count) {
  final String name = type?.toString() ?? '';
  if (count == 1) {
    return switch (name) {
      'Track' || 'QueueItem' => 'Track',
      'Album' => 'Album',
      'Artist' => 'Artist',
      'Playlist' => 'Playlist',
      _ => 'Element',
    };
  }
  return switch (name) {
    'Track' || 'QueueItem' => 'Tracks',
    'Album' => 'Alben',
    'Artist' => 'Artists',
    'Playlist' => 'Playlists',
    _ => 'Elemente',
  };
}
