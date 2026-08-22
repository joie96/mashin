import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';

class SelectionActionsOverlay extends StatelessWidget {
  final bool allSelected;
  final VoidCallback onToggleAll;
  final VoidCallback? onMenuTap;
  final VoidCallback onClose;

  const SelectionActionsOverlay({
    super.key,
    required this.allSelected,
    required this.onToggleAll,
    required this.onMenuTap,
    required this.onClose,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    Widget actionButton({required IconData icon, required VoidCallback? onTap}) {
      return Container(
        width: 40,
        height: 40,
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(20),
          border: Border.all(color: colors.border),
        ),
        child: InkWell(
          borderRadius: BorderRadius.circular(20),
          onTap: onTap,
          child: Icon(icon, size: 18, color: colors.textPrimary),
        ),
      );
    }

    return Material(
      color: Colors.transparent,
      child: Container(
        padding: const EdgeInsets.all(6),
        decoration: BoxDecoration(
          color: colors.bgSecondary,
          borderRadius: BorderRadius.circular(16),
          border: Border.all(color: colors.border),
          boxShadow: <BoxShadow>[
            BoxShadow(
              color: Colors.black.withValues(alpha: 0.15),
              blurRadius: 12,
              offset: const Offset(0, 4),
            ),
          ],
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            actionButton(icon: allSelected ? Icons.deselect_outlined : Icons.select_all_outlined, onTap: onToggleAll),
            const SizedBox(width: 6),
            actionButton(icon: Icons.more_vert_outlined, onTap: onMenuTap),
            const SizedBox(width: 6),
            actionButton(icon: Icons.close_outlined, onTap: onClose),
          ],
        ),
      ),
    );
  }
}
