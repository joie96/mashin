import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';

class SelectionActionsOverlay extends StatelessWidget {
  final bool allSelected;
  final int selectedCount;
  final String itemTypeLabel;
  final VoidCallback onToggleAll;
  final VoidCallback? onMenuTap;
  final VoidCallback onClose;

  const SelectionActionsOverlay({
    super.key,
    required this.allSelected,
    required this.selectedCount,
    required this.itemTypeLabel,
    required this.onToggleAll,
    required this.onMenuTap,
    required this.onClose,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    Widget actionButton({
      required IconData icon,
      required VoidCallback? onTap,
    }) {
      return SizedBox(
        width: 40,
        height: 40,
        child: Material(
          color: Colors.transparent,
          shape: const CircleBorder(),
          child: InkWell(
            customBorder: const CircleBorder(),
            splashColor: Colors.transparent,
            highlightColor: Colors.transparent,
            onTap: onTap,
            child: Icon(icon, size: 18, color: colors.textPrimary),
          ),
        ),
      );
    }

    return Material(
      color: Colors.transparent,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        decoration: BoxDecoration(
          color: colors.bgSecondary,
          borderRadius: BorderRadius.circular(16),
          border: Border.all(color: colors.border),
          boxShadow: <BoxShadow>[
            BoxShadow(
              color: Colors.black.withValues(alpha: 0.22),
              blurRadius: 14,
              offset: const Offset(0, 4),
            ),
          ],
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            Text(
              '$selectedCount $itemTypeLabel',
              style: TextStyle(
                color: colors.textPrimary,
                fontWeight: FontWeight.w700,
                fontSize: 13,
              ),
            ),
            const SizedBox(width: 10),
            Container(width: 1, height: 22, color: colors.border),
            const SizedBox(width: 4),
            actionButton(
              icon: allSelected
                  ? Icons.deselect_outlined
                  : Icons.select_all_outlined,
              onTap: onToggleAll,
            ),
            actionButton(icon: Icons.more_vert_outlined, onTap: onMenuTap),
            actionButton(icon: Icons.close_outlined, onTap: onClose),
          ],
        ),
      ),
    );
  }
}
