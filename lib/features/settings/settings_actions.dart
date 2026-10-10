import 'package:flutter/material.dart';

class SettingsActions extends StatelessWidget {
  const SettingsActions({
    super.key,
    required this.onSave,
    required this.onCancel,
    required this.isSaving,
  });

  final VoidCallback onSave;
  final VoidCallback onCancel;
  final bool isSaving;

  @override
  Widget build(BuildContext context) => SafeArea(
    top: false,
    child: Padding(
      padding: const EdgeInsets.symmetric(vertical: 16),
      child: Wrap(
        alignment: WrapAlignment.end,
        spacing: 12,
        runSpacing: 8,
        children: <Widget>[
          TextButton.icon(
            style: TextButton.styleFrom(
              minimumSize: const Size(0, 48),
              padding: const EdgeInsets.symmetric(horizontal: 12),
            ),
            onPressed: isSaving ? null : onCancel,
            icon: const Icon(Icons.close),
            label: const Text('Abbrechen'),
          ),
          FilledButton.icon(
            style: FilledButton.styleFrom(
              minimumSize: const Size(0, 48),
              padding: const EdgeInsets.symmetric(horizontal: 12),
            ),
            onPressed: isSaving ? null : onSave,
            icon: isSaving
                ? const SizedBox(
                    width: 18,
                    height: 18,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.save_outlined),
            label: const Text('Speichern'),
          ),
        ],
      ),
    ),
  );
}
