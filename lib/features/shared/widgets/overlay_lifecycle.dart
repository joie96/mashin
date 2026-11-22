import 'package:flutter/widgets.dart';

/// Keeps overlay subscriptions alive only through opening and closing animations.
class OverlayLifecycle extends StatefulWidget {
  const OverlayLifecycle({
    super.key,
    required this.isActive,
    required this.builder,
  });

  final bool isActive;
  final Widget Function(
    BuildContext context,
    bool hasEntered,
    VoidCallback onAnimationEnd,
  )
  builder;

  @override
  State<OverlayLifecycle> createState() => _OverlayLifecycleState();
}

class _OverlayLifecycleState extends State<OverlayLifecycle> {
  bool _hasContent = false;
  bool _hasEntered = false;
  bool _hasRenderedEntry = false;

  // Mount off-screen first so a newly created panel retains its opening animation.
  @override
  void initState() {
    super.initState();
    if (widget.isActive) _mountContent();
  }

  @override
  void didUpdateWidget(covariant OverlayLifecycle oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.isActive && !_hasContent) _mountContent();
    if (!widget.isActive && !_hasRenderedEntry) {
      _hasContent = false;
      _hasEntered = false;
    }
  }

  // Visibility can reverse while closing; only the latest state permits disposal.
  void _finishAnimation() {
    if (!mounted || widget.isActive || !_hasContent) return;
    setState(() {
      _hasContent = false;
      _hasEntered = false;
      _hasRenderedEntry = false;
    });
  }

  void _mountContent() {
    _hasContent = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || !_hasContent) return;
      setState(() => _hasEntered = true);
    });
  }

  @override
  Widget build(BuildContext context) {
    if (!_hasContent) return const SizedBox.shrink();
    if (_hasEntered && widget.isActive) _hasRenderedEntry = true;
    return widget.builder(context, _hasEntered, _finishAnimation);
  }
}
