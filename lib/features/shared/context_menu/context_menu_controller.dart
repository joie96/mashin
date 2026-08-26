import 'dart:async';

import 'package:flutter/material.dart';

import '../../../app/config/app_config.dart';
import '../../../app/theme/app_theme.dart';

typedef ContextMenuActionHandler = FutureOr<void> Function();

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

/// Presents page-specific actions as an anchored popup on desktop and as a bottom sheet on mobile
class AppContextMenuController {
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

  Future<void> show({
    required BuildContext context,
    required List<ContextMenuAction> actions,
    BuildContext? anchorContext,
    Offset? globalPosition,
  }) async {
    if (actions.isEmpty) {
      return;
    }

    if (AppConfig.isMobile()) {
      await _showMobile(context: context, actions: actions);
      return;
    }

    await _showDesktop(
      context: context,
      anchorContext: anchorContext ?? context,
      actions: actions,
      globalPosition: globalPosition,
    );
  }

  Future<void> _showDesktop({
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

  Future<void> _showMobile({
    required BuildContext context,
    required List<ContextMenuAction> actions,
  }) async {
    await showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (BuildContext sheetContext) => SafeArea(
        child: ListView.separated(
          shrinkWrap: true,
          itemCount: actions.length,
          separatorBuilder: (_, index) =>
              actions[index].isSeparator || actions[index + 1].isSeparator
              ? const SizedBox.shrink()
              : const Divider(height: 1),
          itemBuilder: (BuildContext itemContext, int index) {
            final ContextMenuAction action = actions[index];
            final colors = itemContext.mashinColors;

            if (action.isSeparator) {
              return Container(
                height: 9,
                alignment: Alignment.center,
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: Container(height: 1, color: colors.border),
              );
            }

            final Color color = action.isDestructive
                ? Theme.of(itemContext).colorScheme.error
                : colors.textPrimary;

            return ListTile(
              enabled: action.isEnabled,
              leading: RotatedBox(
                quarterTurns: action.iconQuarterTurns,
                child: Icon(action.icon, color: color),
              ),
              title: Text(action.label, style: TextStyle(color: color)),
              trailing: action.hasSubmenu
                  ? Icon(
                      Icons.chevron_right_outlined,
                      size: 18,
                      color: colors.textSecondary,
                    )
                  : null,
              onTap: !action.isEnabled
                  ? null
                  : () async {
                      if (action.hasSubmenu) {
                        Navigator.of(sheetContext).pop();
                        await _showMobile(
                          context: context,
                          actions: action.submenu,
                        );
                        return;
                      }
                      Navigator.of(sheetContext).pop();
                      await action.onSelected();
                    },
            );
          },
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
