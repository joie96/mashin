import 'package:flutter/material.dart';
import 'package:flutter/gestures.dart';

import '../../../app/theme/app_theme.dart';
import 'selection_controller.dart';

class MobileSectionSelectionState extends ChangeNotifier {
  final Type? itemType;
  final Set<int> _selectedIndexes = <int>{};
  final Object _owner = Object();
  final SelectionController _overlayController = SelectionController.instance;

  MobileSectionSelectionState({required this.itemType});

  bool get isSelectionModeActive => _selectedIndexes.isNotEmpty;

  bool isSelected(int index) => _selectedIndexes.contains(index);

  void select(int index, int itemCount) {
    if (index < 0 || index >= itemCount) {
      return;
    }
    _selectedIndexes.add(index);
    _syncOverlay(itemCount);
  }

  void toggle(int index, int itemCount) {
    if (index < 0 || index >= itemCount) {
      return;
    }
    if (_selectedIndexes.contains(index)) {
      _selectedIndexes.remove(index);
    } else {
      _selectedIndexes.add(index);
    }
    _syncOverlay(itemCount);
  }

  void updateItemCount(int itemCount) {
    _selectedIndexes.removeWhere((int index) => index >= itemCount);
    _syncOverlay(itemCount, notify: false);
  }

  void disposeSelection() {
    _overlayController.deactivate(_owner);
  }

  void _toggleAll(int itemCount) {
    if (_selectedIndexes.length == itemCount) {
      _selectedIndexes.clear();
    } else {
      _selectedIndexes
        ..clear()
        ..addAll(List<int>.generate(itemCount, (int index) => index));
    }
    _syncOverlay(itemCount);
  }

  void _clear() {
    _selectedIndexes.clear();
    _overlayController.deactivate(_owner);
    notifyListeners();
  }

  void _syncOverlay(int itemCount, {bool notify = true}) {
    if (_selectedIndexes.isEmpty || itemCount <= 0) {
      _overlayController.deactivate(_owner);
    } else {
      _overlayController.activate(
        owner: _owner,
        registration: SelectionSessionRegistration(
          allSelected: _selectedIndexes.length == itemCount,
          selectedCount: _selectedIndexes.length,
          itemTypeLabel: selectionTypeLabel(itemType, _selectedIndexes.length),
          onToggleAll: () => _toggleAll(itemCount),
          onMenuTap: null,
          onClose: _clear,
        ),
      );
    }
    if (notify) {
      notifyListeners();
    }
  }
}

class MobileSelectableSectionItem extends StatefulWidget {
  final bool selectionModeActive;
  final bool isSelected;
  final VoidCallback onLongPress;
  final VoidCallback onToggleSelection;
  final VoidCallback? onTap;
  final Widget child;

  const MobileSelectableSectionItem({
    super.key,
    required this.selectionModeActive,
    required this.isSelected,
    required this.onLongPress,
    required this.onToggleSelection,
    this.onTap,
    required this.child,
  });

  @override
  State<MobileSelectableSectionItem> createState() =>
      _MobileSelectableSectionItemState();
}

class _MobileSelectableSectionItemState
    extends State<MobileSelectableSectionItem>
    with SingleTickerProviderStateMixin {
  static const Duration _pressToSelectionDuration = Duration(milliseconds: 380);
  static const Duration _releaseDuration = Duration(milliseconds: 160);

  bool _isPressed = false;
  bool _isDragging = false;
  Offset? _pointerDownPosition;
  late final AnimationController _pressController;

  @override
  void initState() {
    super.initState();
    _pressController = AnimationController(
      vsync: this,
      duration: _pressToSelectionDuration,
      reverseDuration: _releaseDuration,
    )..addListener(_onPressAnimationChanged);
  }

  @override
  void dispose() {
    _pressController
      ..removeListener(_onPressAnimationChanged)
      ..dispose();
    super.dispose();
  }

  void _onPressAnimationChanged() {
    if (mounted) {
      setState(() {});
    }
  }

  @override
  void didUpdateWidget(covariant MobileSelectableSectionItem oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.isSelected != widget.isSelected) {
      if (widget.isSelected) {
        _pressController.forward();
      } else if (!_isPressed) {
        _pressController.reverse();
      }
    }
  }

  void _setPressed(bool isPressed) {
    if (!mounted ||
        _isPressed == isPressed ||
        (isPressed && widget.isSelected)) {
      return;
    }
    _isPressed = isPressed;
    if (isPressed) {
      _pressController.forward();
    } else if (!widget.isSelected) {
      _pressController.reverse();
    }
  }

  void _handlePointerDown(PointerDownEvent event) {
    if (!mounted) {
      return;
    }
    _pointerDownPosition = event.position;
    _isDragging = false;
    _setPressed(true);
  }

  void _handlePointerMove(PointerMoveEvent event) {
    final Offset? downPosition = _pointerDownPosition;
    if (!mounted || _isDragging || downPosition == null) {
      return;
    }
    if ((event.position - downPosition).distance <= kTouchSlop) {
      return;
    }
    _isDragging = true;
    _setPressed(false);
  }

  void _handlePointerEnd() {
    _pointerDownPosition = null;
    _setPressed(false);
  }

  double get _coverScale {
    final double progress = Curves.easeInOutCubic.transform(
      _pressController.value,
    );
    return 1 - (0.15 * progress);
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return GestureDetector(
      behavior: HitTestBehavior.deferToChild,
      onLongPress: widget.onLongPress,
      onTap: widget.selectionModeActive
          ? widget.onToggleSelection
          : widget.onTap,
      child: Listener(
        onPointerDown: _handlePointerDown,
        onPointerMove: _handlePointerMove,
        onPointerUp: (_) => _handlePointerEnd(),
        onPointerCancel: (_) => _handlePointerEnd(),
        child: Stack(
          fit: StackFit.passthrough,
          children: <Widget>[
            Positioned.fill(
              child: AnimatedOpacity(
                duration: const Duration(milliseconds: 180),
                opacity: widget.isSelected ? 1 : 0,
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    color: colors.selection,
                    borderRadius: BorderRadius.circular(12),
                  ),
                ),
              ),
            ),
            MobileSectionSelectionScope(
              isSelected: widget.isSelected,
              coverScale: _coverScale,
              child: AbsorbPointer(
                absorbing: widget.selectionModeActive,
                child: widget.child,
              ),
            ),
            Positioned(
              top: 4,
              right: 4,
              child: IgnorePointer(
                ignoring: !widget.selectionModeActive,
                child: AnimatedOpacity(
                  duration: const Duration(milliseconds: 180),
                  opacity: widget.selectionModeActive ? 1 : 0,
                  child: GestureDetector(
                    onTap: widget.onToggleSelection,
                    child: Container(
                      width: 20,
                      height: 20,
                      alignment: Alignment.center,
                      decoration: BoxDecoration(
                        color: widget.isSelected
                            ? colors.accent
                            : Colors.transparent,
                        border: Border.all(
                          color: widget.isSelected
                              ? colors.accent
                              : colors.textSecondary,
                        ),
                        shape: BoxShape.circle,
                      ),
                      child: widget.isSelected
                          ? Icon(
                              Icons.check,
                              size: 14,
                              color: Theme.of(context).colorScheme.onPrimary,
                            )
                          : null,
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class MobileSectionSelectionScope extends InheritedWidget {
  final bool isSelected;
  final double coverScale;

  const MobileSectionSelectionScope({
    super.key,
    required this.isSelected,
    required this.coverScale,
    required super.child,
  });

  static bool isSelectedOf(BuildContext context) {
    return context
            .dependOnInheritedWidgetOfExactType<MobileSectionSelectionScope>()
            ?.isSelected ??
        false;
  }

  static double coverScaleOf(BuildContext context) {
    final MobileSectionSelectionScope? scope = context
        .dependOnInheritedWidgetOfExactType<MobileSectionSelectionScope>();
    if (scope == null) {
      return 1;
    }
    return scope.coverScale;
  }

  @override
  bool updateShouldNotify(MobileSectionSelectionScope oldWidget) {
    return isSelected != oldWidget.isSelected ||
        coverScale != oldWidget.coverScale;
  }
}
