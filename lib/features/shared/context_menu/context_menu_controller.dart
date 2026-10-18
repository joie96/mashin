import 'dart:async';

import 'package:flutter/material.dart';

import '../../../app/layout/layout_utils.dart';
import '../../../app/theme/app_theme.dart';

typedef ContextMenuActionHandler = FutureOr<void> Function();

class MobileOverlayDragHandle extends StatelessWidget {
  const MobileOverlayDragHandle({required this.isDragging, super.key});

  final bool isDragging;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Container(
      width: 52,
      height: 4,
      decoration: BoxDecoration(
        color: isDragging ? colors.textPrimary : colors.bgTertiary,
        borderRadius: BorderRadius.circular(2),
      ),
    );
  }
}

class ContextMenuAction {
  final String label;
  final IconData icon;
  final int iconQuarterTurns;
  final ContextMenuActionHandler onSelected;
  final bool isDestructive;
  final bool isEnabled;

  /// Renders a horizontal divider instead of an actionable row.
  final bool isSeparator;

  /// Child entries shown in a flyout submenu when this entry is hovered.
  final List<ContextMenuAction> submenu;

  const ContextMenuAction({
    required this.label,
    required this.icon,
    required this.onSelected,
    this.iconQuarterTurns = 0,
    this.isDestructive = false,
    this.isEnabled = true,
  }) : isSeparator = false,
       submenu = const <ContextMenuAction>[];

  const ContextMenuAction.submenu({
    required this.label,
    required this.icon,
    required List<ContextMenuAction> children,
    this.iconQuarterTurns = 0,
    this.isEnabled = true,
  }) : isSeparator = false,
       isDestructive = false,
       submenu = children,
       onSelected = _noopAction;

  const ContextMenuAction.separator()
    : label = '',
      icon = Icons.remove,
      iconQuarterTurns = 0,
      onSelected = _noopAction,
      isDestructive = false,
      isEnabled = false,
      isSeparator = true,
      submenu = const <ContextMenuAction>[];

  static void _noopAction() {}

  bool get hasSubmenu => submenu.isNotEmpty;
}

class _DesktopCustomMenuLayout extends SingleChildLayoutDelegate {
  _DesktopCustomMenuLayout({required this.anchorCenter, required this.width});

  final Offset anchorCenter;
  final double width;

  // Content-sized layout and viewport positioning.
  @override
  BoxConstraints getConstraintsForChild(BoxConstraints constraints) {
    return BoxConstraints(
      maxWidth: width,
      maxHeight:
          constraints.maxHeight -
          2 * AppContextMenuController._desktopMenuViewportInset,
    );
  }

  @override
  Offset getPositionForChild(Size size, Size childSize) {
    const viewportInset = AppContextMenuController._desktopMenuViewportInset;
    const anchorGap = AppContextMenuController._desktopMenuAnchorGap;
    double left = anchorCenter.dx - childSize.width - anchorGap;
    double top = anchorCenter.dy - childSize.height - anchorGap;
    if (left < viewportInset) {
      left = anchorCenter.dx + anchorGap;
    }
    if (top < viewportInset) {
      top = anchorCenter.dy + anchorGap;
    }
    return Offset(
      left.clamp(viewportInset, size.width - childSize.width - viewportInset),
      top.clamp(viewportInset, size.height - childSize.height - viewportInset),
    );
  }

  @override
  bool shouldRelayout(_DesktopCustomMenuLayout oldDelegate) {
    return anchorCenter != oldDelegate.anchorCenter ||
        width != oldDelegate.width;
  }
}

/// Presents page-specific actions as an anchored popup on desktop and a bottom
/// sheet on mobile; mobile submenus use dialogs layered above their parent.
class AppContextMenuController {
  static const double _mobileDialogRadius = 8;
  static const double _mobileDismissThreshold = 120;
  static const Duration _mobileDragResetDuration = Duration(milliseconds: 260);
  static const double _desktopMenuWidth = 240;
  static const double _desktopMenuItemHeight = 48;
  static const double _desktopMenuSeparatorHeight = 9;
  static const double _desktopMenuVerticalPadding = 8;
  static const double _desktopMenuViewportInset = 8;
  static const double _desktopMenuAnchorGap = 4;
  static const Duration _desktopMenuTransitionDuration = Duration(
    milliseconds: 120,
  );

  static Widget _buildDesktopMenuFade(
    Animation<double> animation,
    Widget child,
  ) {
    return FadeTransition(
      opacity: CurvedAnimation(
        parent: animation,
        curve: Curves.easeOut,
        reverseCurve: Curves.easeIn,
      ),
      child: child,
    );
  }

  static double _measureMenuHeight(List<ContextMenuAction> actions) {
    double height = _desktopMenuVerticalPadding * 2;
    for (final ContextMenuAction action in actions) {
      height += action.isSeparator
          ? _desktopMenuSeparatorHeight
          : _desktopMenuItemHeight;
    }
    return height;
  }

  const AppContextMenuController();

  Future<void> showContextMenu({
    required BuildContext context,
    required List<ContextMenuAction> actions,
    BuildContext? anchorContext,
    Offset? globalPosition,
  }) async {
    if (actions.isEmpty) {
      return;
    }

    if (isCompactLayout(context)) {
      await _showMobileBottomSheet(context: context, actions: actions);
      return;
    }

    await _showDesktopContextMenu(
      context: context,
      anchorContext: anchorContext ?? context,
      actions: actions,
      globalPosition: globalPosition,
    );
  }

  Future<void> showMobileDialog({
    required BuildContext context,
    required List<ContextMenuAction> actions,
    String? title,
  }) async {
    if (actions.isEmpty) {
      return;
    }

    if (isCompactLayout(context)) {
      await showGeneralDialog<void>(
        context: context,
        barrierDismissible: true,
        barrierLabel: 'Kontextmenü schließen',
        pageBuilder: (BuildContext dialogContext, _, _) => Dialog(
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(_mobileDialogRadius),
          ),
          child: SizedBox(
            width: 240,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                if (title != null)
                  Padding(
                    padding: const EdgeInsets.fromLTRB(16, 14, 16, 6),
                    child: Text(
                      title,
                      style: TextStyle(
                        color: Theme.of(dialogContext).colorScheme.onSurface,
                        fontSize: 15,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                _buildMobileContextMenuList(
                  context: context,
                  menuContext: dialogContext,
                  actions: actions,
                ),
              ],
            ),
          ),
        ),
      );
      return;
    }
  }

  Future<void> showCustomContextMenu({
    required BuildContext context,
    required BuildContext anchorContext,
    required Widget child,
    required double width,
  }) async {
    if (isCompactLayout(context)) {
      await _showMobileCustomDialog(
        context: context,
        child: child,
        width: width,
      );
      return;
    }

    await _showDesktopCustomMenu(
      context: context,
      anchorContext: anchorContext,
      child: child,
      width: width,
    );
  }

  Future<void> _showMobileCustomDialog({
    required BuildContext context,
    required Widget child,
    required double width,
  }) async {
    await showGeneralDialog<void>(
      context: context,
      barrierDismissible: true,
      barrierLabel: 'Kontextmenü schließen',
      pageBuilder: (_, _, _) => Dialog(
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(_mobileDialogRadius),
        ),
        child: SizedBox(width: width, child: child),
      ),
    );
  }

  Future<void> _showDesktopCustomMenu({
    required BuildContext context,
    required BuildContext anchorContext,
    required Widget child,
    required double width,
  }) async {
    final RenderBox overlayBox =
        Navigator.of(context).overlay!.context.findRenderObject()! as RenderBox;
    final RenderBox anchorBox = anchorContext.findRenderObject()! as RenderBox;
    final Offset anchorCenter = anchorBox.localToGlobal(
      anchorBox.size.center(Offset.zero),
      ancestor: overlayBox,
    );

    await showGeneralDialog<void>(
      context: context,
      barrierDismissible: true,
      barrierLabel: 'Kontextmenü schließen',
      barrierColor: Colors.transparent,
      transitionDuration: _desktopMenuTransitionDuration,
      transitionBuilder: (_, animation, _, dialogChild) =>
          _buildDesktopMenuFade(animation, dialogChild),
      pageBuilder: (_, _, _) => CustomSingleChildLayout(
        delegate: _DesktopCustomMenuLayout(
          anchorCenter: anchorCenter,
          width: width,
        ),
        child: SizedBox(width: width, child: child),
      ),
    );
  }

  Future<void> _showDesktopContextMenu({
    required BuildContext context,
    required BuildContext anchorContext,
    required List<ContextMenuAction> actions,
    Offset? globalPosition,
  }) async {
    final RenderBox overlayBox =
        Navigator.of(context).overlay!.context.findRenderObject()! as RenderBox;

    final double menuHeight = _measureMenuHeight(actions);
    double left;
    double top;

    if (globalPosition != null) {
      // Open directly at the pointer like a classic right-click context menu.
      final Offset pointerPosition = overlayBox.globalToLocal(globalPosition);
      left = pointerPosition.dx;
      top = pointerPosition.dy;
      if (left + _desktopMenuWidth >
          overlayBox.size.width - _desktopMenuViewportInset) {
        left = pointerPosition.dx - _desktopMenuWidth;
      }
      if (top + menuHeight >
          overlayBox.size.height - _desktopMenuViewportInset) {
        top = pointerPosition.dy - menuHeight;
      }
    } else {
      final RenderBox anchorBox =
          anchorContext.findRenderObject()! as RenderBox;
      final Offset anchorCenter = anchorBox.localToGlobal(
        anchorBox.size.center(Offset.zero),
        ancestor: overlayBox,
      );

      left = anchorCenter.dx + _desktopMenuAnchorGap;
      top = anchorCenter.dy + _desktopMenuAnchorGap;

      if (left + _desktopMenuWidth >
          overlayBox.size.width - _desktopMenuViewportInset) {
        left = anchorCenter.dx - _desktopMenuWidth - _desktopMenuAnchorGap;
      }
      if (top + menuHeight >
          overlayBox.size.height - _desktopMenuViewportInset) {
        top = anchorCenter.dy - menuHeight - _desktopMenuAnchorGap;
      }
    }

    left = left.clamp(
      _desktopMenuViewportInset,
      overlayBox.size.width - _desktopMenuWidth - _desktopMenuViewportInset,
    );
    top = top.clamp(
      _desktopMenuViewportInset,
      overlayBox.size.height - menuHeight - _desktopMenuViewportInset,
    );

    final ContextMenuAction? selected =
        await showGeneralDialog<ContextMenuAction>(
          context: context,
          barrierDismissible: true,
          barrierLabel: 'Kontextmenü schließen',
          barrierColor: Colors.transparent,
          transitionDuration: _desktopMenuTransitionDuration,
          transitionBuilder: (_, animation, _, child) =>
              _buildDesktopMenuFade(animation, child),
          pageBuilder: (BuildContext dialogContext, _, _) {
            return Stack(
              children: <Widget>[
                Positioned(
                  left: left,
                  top: top,
                  width: _desktopMenuWidth,
                  child: _DesktopContextMenu(
                    actions: actions,
                    onSelected: (ContextMenuAction action) {
                      Navigator.of(dialogContext).pop(action);
                    },
                  ),
                ),
              ],
            );
          },
        );
    if (selected != null && selected.isEnabled && !selected.isSeparator) {
      await selected.onSelected();
    }
  }

  Future<void> _showMobileBottomSheet({
    required BuildContext context,
    required List<ContextMenuAction> actions,
  }) async {
    final double maxSheetHeight = MediaQuery.sizeOf(context).height * 0.9;
    await showModalBottomSheet<void>(
      context: context,
      useRootNavigator: true,
      isScrollControlled: true,
      showDragHandle: false,
      enableDrag: false,
      constraints: BoxConstraints(maxHeight: maxSheetHeight),
      backgroundColor: Colors.transparent,
      elevation: 0,
      builder: (BuildContext sheetContext) => _MobileContextMenuSheet(
        maxDragOffset: maxSheetHeight,
        onDismiss: () => Navigator.of(sheetContext).pop(),
        childBuilder: (ScrollController scrollController) =>
            _buildMobileContextMenuList(
              context: context,
              menuContext: sheetContext,
              actions: actions,
              scrollController: scrollController,
            ),
      ),
    );
  }

  Widget _buildMobileContextMenuList({
    required BuildContext context,
    required BuildContext menuContext,
    required List<ContextMenuAction> actions,
    ScrollController? scrollController,
  }) => ListView.builder(
    primary: scrollController == null,
    controller: scrollController,
    shrinkWrap: true,
    itemCount: actions.length,
    itemBuilder: (BuildContext itemContext, int index) {
      final ContextMenuAction action = actions[index];
      final colors = itemContext.mashinColors;

      if (action.isSeparator) {
        return Container(
          height: 9,
          alignment: Alignment.center,
          padding: const EdgeInsets.symmetric(horizontal: 16),
          child: Container(height: 1, color: colors.bgTertiary),
        );
      }

      final Color textColor = action.isDestructive
          ? Theme.of(itemContext).colorScheme.error
          : colors.textPrimary;
      final Color iconColor = action.isDestructive
          ? Theme.of(itemContext).colorScheme.error
          : colors.textSecondary;

      return ListTile(
        enabled: action.isEnabled,
        leading: RotatedBox(
          quarterTurns: action.iconQuarterTurns,
          child: Icon(action.icon, size: 20, weight: 300, color: iconColor),
        ),
        title: Text(
          action.label,
          style: TextStyle(
            color: textColor,
            fontSize: 14,
            fontWeight: FontWeight.w400,
          ),
        ),
        trailing: action.hasSubmenu
            ? Icon(
                Icons.chevron_right_outlined,
                size: 18,
                weight: 300,
                color: colors.textSecondary,
              )
            : null,
        onTap: !action.isEnabled
            ? null
            : () async {
                if (action.hasSubmenu) {
                  await showMobileDialog(
                    context: context,
                    actions: action.submenu,
                    title: 'Zu Playlist hinzufügen',
                  );
                  return;
                }
                Navigator.of(menuContext).pop();
                await action.onSelected();
              },
      );
    },
  );
}

class _MobileContextMenuSheet extends StatefulWidget {
  const _MobileContextMenuSheet({
    required this.childBuilder,
    required this.maxDragOffset,
    required this.onDismiss,
  });

  final Widget Function(ScrollController scrollController) childBuilder;
  final double maxDragOffset;
  final VoidCallback onDismiss;

  @override
  State<_MobileContextMenuSheet> createState() =>
      _MobileContextMenuSheetState();
}

class _MobileContextMenuSheetState extends State<_MobileContextMenuSheet> {
  final ScrollController _scrollController = ScrollController();
  bool _isDragging = false;
  double? _nonScrollableDragPointerY;
  double _dragOffset = 0;
  Duration _dragOffsetAnimationDuration = Duration.zero;

  void _updateDismissDrag(double delta) {
    if (delta <= 0) {
      return;
    }
    setState(() {
      _dragOffsetAnimationDuration = Duration.zero;
      _dragOffset = (_dragOffset + delta).clamp(0, widget.maxDragOffset);
    });
  }

  void _handleContentPointerDown(PointerDownEvent event) {
    if (!_scrollController.hasClients ||
        _scrollController.position.maxScrollExtent >
            _scrollController.position.minScrollExtent) {
      _nonScrollableDragPointerY = null;
      return;
    }
    _nonScrollableDragPointerY = event.position.dy;
  }

  void _handleContentPointerMove(PointerMoveEvent event) {
    final double? previousY = _nonScrollableDragPointerY;
    if (previousY == null) {
      return;
    }

    _nonScrollableDragPointerY = event.position.dy;
    final double downwardDelta = event.position.dy - previousY;
    if (downwardDelta > 0) {
      if (!_isDragging) {
        setState(() => _isDragging = true);
      }
      _updateDismissDrag(downwardDelta);
    }
  }

  void _finishContentPointerDrag() {
    _nonScrollableDragPointerY = null;
    _finishDismissDrag(0);
  }

  void _finishDismissDrag(double velocity) {
    if (!_isDragging) {
      return;
    }
    setState(() => _isDragging = false);
    if (_dragOffset >= AppContextMenuController._mobileDismissThreshold ||
        velocity > 700) {
      widget.onDismiss();
      return;
    }
    setState(() {
      _dragOffsetAnimationDuration =
          AppContextMenuController._mobileDragResetDuration;
      _dragOffset = 0;
    });
  }

  bool _handleMenuScrollNotification(ScrollNotification notification) {
    if (notification.metrics.maxScrollExtent <=
        notification.metrics.minScrollExtent) {
      return false;
    }
    if (notification.metrics.pixels > notification.metrics.minScrollExtent) {
      return false;
    }
    if (notification is OverscrollNotification && notification.overscroll < 0) {
      if (!_isDragging) {
        setState(() => _isDragging = true);
      }
      _updateDismissDrag(-notification.overscroll);
    } else if (notification is ScrollEndNotification) {
      _finishDismissDrag(notification.dragDetails?.primaryVelocity ?? 0);
    }
    return false;
  }

  @override
  void dispose() {
    _scrollController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return AnimatedSlide(
      duration: _dragOffsetAnimationDuration,
      curve: Curves.easeOutCubic,
      offset: Offset(0, _dragOffset / widget.maxDragOffset),
      onEnd: () {
        if (mounted && _dragOffsetAnimationDuration != Duration.zero) {
          setState(() => _dragOffsetAnimationDuration = Duration.zero);
        }
      },
      child: Material(
        color: colors.bgSecondary,
        clipBehavior: Clip.antiAlias,
        shape: const RoundedRectangleBorder(
          borderRadius: BorderRadius.vertical(top: Radius.circular(12)),
        ),
        child: SafeArea(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: <Widget>[
              GestureDetector(
                onVerticalDragStart: (_) => setState(() => _isDragging = true),
                onVerticalDragUpdate: (DragUpdateDetails details) {
                  _updateDismissDrag(details.delta.dy);
                },
                onVerticalDragEnd: (DragEndDetails details) {
                  _finishDismissDrag(details.primaryVelocity ?? 0);
                },
                onVerticalDragCancel: () => _finishDismissDrag(0),
                behavior: HitTestBehavior.opaque,
                child: Padding(
                  padding: const EdgeInsets.only(top: 8, bottom: 0),
                  child: MobileOverlayDragHandle(isDragging: _isDragging),
                ),
              ),
              Flexible(
                child: Listener(
                  behavior: HitTestBehavior.translucent,
                  onPointerDown: _handleContentPointerDown,
                  onPointerMove: _handleContentPointerMove,
                  onPointerUp: (_) => _finishContentPointerDrag(),
                  onPointerCancel: (_) => _finishContentPointerDrag(),
                  child: NotificationListener<ScrollNotification>(
                    onNotification: _handleMenuScrollNotification,
                    child: widget.childBuilder(_scrollController),
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _DesktopContextMenu extends StatefulWidget {
  final List<ContextMenuAction> actions;
  final ValueChanged<ContextMenuAction> onSelected;
  final _DesktopContextMenuSubmenuController? submenuController;

  const _DesktopContextMenu({
    required this.actions,
    required this.onSelected,
    this.submenuController,
  });

  @override
  State<_DesktopContextMenu> createState() => _DesktopContextMenuState();
}

class _DesktopContextMenuState extends State<_DesktopContextMenu> {
  late final _DesktopContextMenuSubmenuController _submenuController =
      widget.submenuController ?? _DesktopContextMenuSubmenuController();

  @override
  void dispose() {
    if (widget.submenuController == null) {
      _submenuController.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return MouseRegion(
      onEnter: (_) => _submenuController.cancelPendingClose(),
      onExit: (_) => _submenuController.scheduleClose(),
      child: Material(
        color: colors.bgSecondary,
        elevation: 12,
        borderRadius: BorderRadius.circular(8),
        child: Container(
          padding: const EdgeInsets.symmetric(
            vertical: AppContextMenuController._desktopMenuVerticalPadding,
          ),
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(8),
            border: Border.all(color: colors.border),
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: widget.actions
                .asMap()
                .entries
                .map((entry) {
                  final int index = entry.key;
                  final ContextMenuAction action = entry.value;
                  final Key key = ValueKey<int>(index);
                  return action.isSeparator
                      ? _DesktopContextMenuSeparator(key: key)
                      : _DesktopContextMenuActionTile(
                          key: key,
                          action: action,
                          onSelected: widget.onSelected,
                          submenuController: _submenuController,
                        );
                })
                .toList(growable: false),
          ),
        ),
      ),
    );
  }
}

class _DesktopContextMenuSubmenuController {
  static const Duration _closeDelay = Duration(milliseconds: 220);

  _DesktopContextMenuActionTileState? _activeTile;
  Timer? _closeTimer;

  void activate(_DesktopContextMenuActionTileState tile) {
    cancelPendingClose();
    if (identical(_activeTile, tile)) {
      return;
    }
    _activeTile?._hideSubmenu();
    _activeTile = tile;
  }

  void deactivate(_DesktopContextMenuActionTileState tile) {
    if (identical(_activeTile, tile)) {
      _activeTile = null;
    }
  }

  void cancelPendingClose() {
    _closeTimer?.cancel();
    _closeTimer = null;
  }

  void scheduleClose() {
    cancelPendingClose();
    _closeTimer = Timer(_closeDelay, () {
      _closeTimer = null;
      _activeTile?._hideSubmenu();
    });
  }

  void dispose() {
    cancelPendingClose();
    final _DesktopContextMenuActionTileState? activeTile = _activeTile;
    _activeTile = null;
    activeTile?._removeSubmenuImmediately();
  }
}

class _DesktopContextMenuSeparator extends StatelessWidget {
  const _DesktopContextMenuSeparator({super.key});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return SizedBox(
      height: AppContextMenuController._desktopMenuSeparatorHeight,
      child: Center(
        child: Container(
          height: 1,
          margin: const EdgeInsets.symmetric(horizontal: 10),
          color: colors.border,
        ),
      ),
    );
  }
}

class _DesktopContextMenuActionTile extends StatefulWidget {
  final ContextMenuAction action;
  final ValueChanged<ContextMenuAction> onSelected;
  final _DesktopContextMenuSubmenuController submenuController;

  const _DesktopContextMenuActionTile({
    super.key,
    required this.action,
    required this.onSelected,
    required this.submenuController,
  });

  @override
  State<_DesktopContextMenuActionTile> createState() =>
      _DesktopContextMenuActionTileState();
}

class _DesktopContextMenuActionTileState
    extends State<_DesktopContextMenuActionTile>
    with SingleTickerProviderStateMixin {
  OverlayEntry? _submenuEntry;
  bool _isSubmenuClosing = false;
  late final AnimationController _submenuAnimationController;
  late final Animation<double> _submenuAnimation;

  @override
  void initState() {
    super.initState();
    _submenuAnimationController = AnimationController(
      duration: AppContextMenuController._desktopMenuTransitionDuration,
      reverseDuration: AppContextMenuController._desktopMenuTransitionDuration,
      vsync: this,
    )..addStatusListener(_onSubmenuAnimationStatus);
    _submenuAnimation = CurvedAnimation(
      parent: _submenuAnimationController,
      curve: Curves.easeOut,
      reverseCurve: Curves.easeIn,
    );
  }

  bool get _hasSubmenu => widget.action.hasSubmenu && widget.action.isEnabled;

  void _showSubmenu() {
    if (!mounted || !_hasSubmenu) {
      return;
    }
    widget.submenuController.activate(this);
    if (_submenuEntry != null) {
      _isSubmenuClosing = false;
      _submenuAnimationController.forward();
      return;
    }

    final OverlayState overlay = Overlay.of(context);
    final RenderBox overlayBox =
        overlay.context.findRenderObject() as RenderBox? ??
        Navigator.of(context).overlay!.context.findRenderObject()! as RenderBox;

    const double submenuWidth = AppContextMenuController._desktopMenuWidth;
    final double submenuHeight = AppContextMenuController._measureMenuHeight(
      widget.action.submenu,
    );

    final RenderBox tileBox = context.findRenderObject()! as RenderBox;
    final Offset tileTopLeft = tileBox.localToGlobal(
      Offset.zero,
      ancestor: overlayBox,
    );
    final Offset tileBottomRight = tileBox.localToGlobal(
      tileBox.size.bottomRight(Offset.zero),
      ancestor: overlayBox,
    );

    double left = tileBottomRight.dx;
    double top = tileTopLeft.dy;
    if (left + submenuWidth >
        overlayBox.size.width -
            AppContextMenuController._desktopMenuViewportInset) {
      left = tileTopLeft.dx - submenuWidth;
    }
    if (top + submenuHeight >
        overlayBox.size.height -
            AppContextMenuController._desktopMenuViewportInset) {
      top = tileBottomRight.dy - submenuHeight;
    }
    left = left.clamp(
      AppContextMenuController._desktopMenuViewportInset,
      overlayBox.size.width -
          submenuWidth -
          AppContextMenuController._desktopMenuViewportInset,
    );
    top = top.clamp(
      AppContextMenuController._desktopMenuViewportInset,
      overlayBox.size.height -
          submenuHeight -
          AppContextMenuController._desktopMenuViewportInset,
    );

    _submenuEntry = OverlayEntry(
      builder: (BuildContext submenuContext) {
        return Stack(
          children: <Widget>[
            Positioned(
              left: left,
              top: top,
              width: submenuWidth,
              child: FadeTransition(
                opacity: _submenuAnimation,
                child: _DesktopContextMenu(
                  actions: widget.action.submenu,
                  onSelected: (ContextMenuAction action) {
                    _hideSubmenu();
                    widget.onSelected(action);
                  },
                  submenuController: widget.submenuController,
                ),
              ),
            ),
          ],
        );
      },
    );

    overlay.insert(_submenuEntry!);
    _isSubmenuClosing = false;
    _submenuAnimationController.forward(from: 0);
  }

  void _hideSubmenu() {
    widget.submenuController.deactivate(this);
    if (_submenuEntry == null) {
      return;
    }
    _isSubmenuClosing = true;
    _submenuAnimationController.reverse();
  }

  void _removeSubmenuImmediately() {
    _isSubmenuClosing = false;
    _submenuEntry?.remove();
    _submenuEntry = null;
  }

  void _onSubmenuAnimationStatus(AnimationStatus status) {
    if (status == AnimationStatus.dismissed && _isSubmenuClosing) {
      _submenuEntry?.remove();
      _submenuEntry = null;
      _isSubmenuClosing = false;
    }
  }

  @override
  void dispose() {
    _removeSubmenuImmediately();
    _submenuAnimationController.dispose();
    widget.submenuController.deactivate(this);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final Color color = widget.action.isDestructive
        ? Theme.of(context).colorScheme.error
        : colors.textPrimary;

    final Widget tile = SizedBox(
      height: AppContextMenuController._desktopMenuItemHeight,
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          onTap: widget.action.isEnabled && !_hasSubmenu
              ? () => widget.onSelected(widget.action)
              : null,
          hoverColor: colors.hover,
          splashColor: Colors.transparent,
          highlightColor: Colors.transparent,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 14),
            child: Row(
              children: <Widget>[
                RotatedBox(
                  quarterTurns: widget.action.iconQuarterTurns,
                  child: Icon(widget.action.icon, size: 18, color: color),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Text(
                    widget.action.label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: color),
                  ),
                ),
                if (_hasSubmenu)
                  Icon(
                    Icons.chevron_right_outlined,
                    size: 16,
                    color: colors.textSecondary,
                  ),
              ],
            ),
          ),
        ),
      ),
    );

    if (!_hasSubmenu) {
      return tile;
    }

    return MouseRegion(
      onEnter: (_) => _showSubmenu(),
      onExit: (_) => widget.submenuController.scheduleClose(),
      child: tile,
    );
  }
}
