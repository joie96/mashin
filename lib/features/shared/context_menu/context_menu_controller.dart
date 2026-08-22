import 'dart:async';

import 'package:flutter/material.dart';

import '../../../app/config/app_config.dart';
import '../../../app/theme/app_theme.dart';

typedef ContextMenuActionHandler = FutureOr<void> Function();

class ContextMenuAction {
  final String label;
  final IconData icon;
  final ContextMenuActionHandler onSelected;
  final bool isDestructive;
  final bool isEnabled;

  const ContextMenuAction({
    required this.label,
    required this.icon,
    required this.onSelected,
    this.isDestructive = false,
    this.isEnabled = true,
  });
}

/// Presents page-specific actions as an anchored popup on desktop and as a bottom sheet on mobile
class AppContextMenuController {
  static const double _desktopMenuWidth = 240;
  static const double _desktopMenuItemHeight = 48;
  static const double _desktopMenuVerticalPadding = 8;
  static const double _desktopMenuViewportInset = 8;
  static const double _desktopMenuAnchorGap = 4;

  const AppContextMenuController();

  Future<void> show({
    required BuildContext context,
    required List<ContextMenuAction> actions,
    BuildContext? anchorContext,
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
    );
  }

  Future<void> _showDesktop({
    required BuildContext context,
    required BuildContext anchorContext,
    required List<ContextMenuAction> actions,
  }) async {
    final RenderBox overlayBox =
        Navigator.of(context).overlay!.context.findRenderObject()! as RenderBox;
    final RenderBox anchorBox = anchorContext.findRenderObject()! as RenderBox;
    final Offset anchorCenter = anchorBox.localToGlobal(
      anchorBox.size.center(Offset.zero),
      ancestor: overlayBox,
    );

    final double menuHeight =
        (actions.length * _desktopMenuItemHeight) +
        (_desktopMenuVerticalPadding * 2);
    double left = anchorCenter.dx + _desktopMenuAnchorGap;
    double top = anchorCenter.dy + _desktopMenuAnchorGap;

    if (left + _desktopMenuWidth >
        overlayBox.size.width - _desktopMenuViewportInset) {
      left = anchorCenter.dx - _desktopMenuWidth - _desktopMenuAnchorGap;
    }
    if (top + menuHeight > overlayBox.size.height - _desktopMenuViewportInset) {
      top = anchorCenter.dy - menuHeight - _desktopMenuAnchorGap;
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
    if (selected != null && selected.isEnabled) {
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
          separatorBuilder: (_, _) => const Divider(height: 1),
          itemBuilder: (BuildContext itemContext, int index) {
            final ContextMenuAction action = actions[index];
            final colors = itemContext.mashinColors;
            final Color color = action.isDestructive
                ? Theme.of(itemContext).colorScheme.error
                : colors.textPrimary;

            return ListTile(
              enabled: action.isEnabled,
              leading: Icon(action.icon, color: color),
              title: Text(action.label, style: TextStyle(color: color)),
              onTap: !action.isEnabled
                  ? null
                  : () async {
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

class _DesktopContextMenu extends StatelessWidget {
  final List<ContextMenuAction> actions;
  final ValueChanged<ContextMenuAction> onSelected;

  const _DesktopContextMenu({required this.actions, required this.onSelected});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Material(
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
          children: actions
              .map(
                (ContextMenuAction action) => _DesktopContextMenuActionTile(
                  action: action,
                  onSelected: onSelected,
                ),
              )
              .toList(growable: false),
        ),
      ),
    );
  }
}

class _DesktopContextMenuActionTile extends StatelessWidget {
  final ContextMenuAction action;
  final ValueChanged<ContextMenuAction> onSelected;

  const _DesktopContextMenuActionTile({
    required this.action,
    required this.onSelected,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final Color color = action.isDestructive
        ? Theme.of(context).colorScheme.error
        : colors.textPrimary;

    return SizedBox(
      height: AppContextMenuController._desktopMenuItemHeight,
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          onTap: action.isEnabled ? () => onSelected(action) : null,
          hoverColor: colors.hover,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 14),
            child: Row(
              children: <Widget>[
                Icon(action.icon, size: 18, color: color),
                const SizedBox(width: 10),
                Expanded(
                  child: Text(
                    action.label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: color),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
