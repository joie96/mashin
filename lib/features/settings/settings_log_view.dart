import 'package:flutter/material.dart';

import '../../app/logging/app_log_buffer.dart';
import '../../app/theme/app_theme.dart';
import '../core/core_models.dart';

class SettingsLogLevelField extends StatelessWidget {
  const SettingsLogLevelField({
    super.key,
    required this.logLevel,
    required this.onChanged,
  });

  final LogLevelPreference logLevel;
  final ValueChanged<LogLevelPreference> onChanged;

  @override
  Widget build(BuildContext context) {
    return DropdownButtonFormField<LogLevelPreference>(
      initialValue: logLevel,
      decoration: const InputDecoration(
        labelText: 'Log-Level',
        prefixIcon: Icon(Icons.tune_outlined),
      ),
      items: const <DropdownMenuItem<LogLevelPreference>>[
        DropdownMenuItem(value: LogLevelPreference.trace, child: Text('Trace')),
        DropdownMenuItem(value: LogLevelPreference.debug, child: Text('Debug')),
        DropdownMenuItem(value: LogLevelPreference.info, child: Text('Info')),
        DropdownMenuItem(
          value: LogLevelPreference.warn,
          child: Text('Warnung'),
        ),
        DropdownMenuItem(
          value: LogLevelPreference.error,
          child: Text('Fehler'),
        ),
      ],
      onChanged: (LogLevelPreference? value) {
        if (value != null) {
          onChanged(value);
        }
      },
    );
  }
}

class SettingsLogOutput extends StatelessWidget {
  const SettingsLogOutput({super.key});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return AnimatedBuilder(
      animation: AppLogBuffer.shared,
      builder: (BuildContext context, Widget? child) {
        final lines = AppLogBuffer.shared.lines;
        if (lines.isEmpty) {
          return Center(
            child: Text(
              'Noch keine Logausgaben.',
              style: TextStyle(color: colors.textSecondary),
            ),
          );
        }

        return DecoratedBox(
          decoration: BoxDecoration(
            color: colors.bgSecondary,
            border: Border.all(color: colors.border),
            borderRadius: BorderRadius.circular(4),
          ),
          child: ListView.builder(
            padding: const EdgeInsets.all(12),
            itemCount: lines.length,
            itemBuilder: (BuildContext context, int index) => SelectableText(
              lines[index],
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: colors.textSecondary,
                fontFamily: 'monospace',
              ),
            ),
          ),
        );
      },
    );
  }
}
