import 'dart:async';

import 'package:flutter/gestures.dart';
import 'package:flutter/widgets.dart';

/// Keeps primary actions from also selecting their containing desktop item.
class DesktopItemAction extends StatelessWidget {
  const DesktopItemAction({super.key, required this.child});

  final Widget child;
  static final Set<int> _actionPointers = <int>{};

  // Pointer routing.
  static bool handlesPointer(PointerDownEvent event) =>
      _actionPointers.contains(event.pointer);

  @override
  Widget build(BuildContext context) => Listener(
    onPointerDown: (event) {
      if (event.kind != PointerDeviceKind.mouse ||
          (event.buttons & kPrimaryMouseButton) != 0) {
        _actionPointers.add(event.pointer);
        scheduleMicrotask(() => _actionPointers.remove(event.pointer));
      }
    },
    child: GestureDetector(
      excludeFromSemantics: true,
      onTap: () {},
      child: child,
    ),
  );
}
