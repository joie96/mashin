import 'package:flutter/material.dart';

import '../../app/logging/app_log_buffer.dart';
import '../../app/theme/app_theme.dart';
import '../core/core_models.dart';

class SettingsLogLevelField extends StatelessWidget {
  const SettingsLogLevelField({
    super.key,
    required this.logLevel,
    required this.onChanged,
    this.shouldShowLabel = true,
    this.shouldUseCompactFieldLayout = false,
  });

  final LogLevelPreference logLevel;
  final ValueChanged<LogLevelPreference> onChanged;
  final bool shouldShowLabel;
  final bool shouldUseCompactFieldLayout;

  @override
  Widget build(BuildContext context) {
    return DropdownButtonFormField<LogLevelPreference>(
      initialValue: logLevel,
      isDense: shouldUseCompactFieldLayout,
      decoration: InputDecoration(
        labelText: shouldShowLabel ? 'Log-Level' : null,
        isDense: shouldUseCompactFieldLayout,
        filled: shouldUseCompactFieldLayout,
        fillColor: shouldUseCompactFieldLayout
            ? context.mashinColors.bgSecondary
            : null,
        contentPadding: shouldUseCompactFieldLayout
            ? const EdgeInsets.symmetric(horizontal: 12, vertical: 0)
            : null,
        constraints: shouldUseCompactFieldLayout
            ? const BoxConstraints.tightFor(height: 36)
            : null,
        prefixIconConstraints: shouldUseCompactFieldLayout
            ? const BoxConstraints(minWidth: 38, minHeight: 36)
            : null,
        prefixIcon: const Icon(Icons.tune_outlined, size: 18),
        border: shouldUseCompactFieldLayout
            ? OutlineInputBorder(
                borderRadius: BorderRadius.circular(6),
                borderSide: BorderSide.none,
              )
            : null,
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

class SettingsLogOutput extends StatefulWidget {
  const SettingsLogOutput({super.key});

  @override
  State<SettingsLogOutput> createState() => _SettingsLogOutputState();
}

class _SettingsLogOutputState extends State<SettingsLogOutput> {
  final ScrollController _scrollController = ScrollController();
  bool _shouldFollowLog = true;
  bool _isScrollScheduled = false;

  @override
  void initState() {
    super.initState();
    _scrollController.addListener(_updateFollowState);
    AppLogBuffer.shared.addListener(_onLogChanged);
    _scheduleScrollToEnd();
  }

  @override
  void dispose() {
    AppLogBuffer.shared.removeListener(_onLogChanged);
    _scrollController.dispose();
    super.dispose();
  }

  void _updateFollowState() {
    final position = _scrollController.position;
    _shouldFollowLog = position.maxScrollExtent - position.pixels <= 2;
  }

  void _onLogChanged() {
    if (_shouldFollowLog) {
      _scheduleScrollToEnd();
    }
  }

  void _scheduleScrollToEnd() {
    if (_isScrollScheduled) {
      return;
    }
    _isScrollScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _isScrollScheduled = false;
      if (mounted && _shouldFollowLog && _scrollController.hasClients) {
        _scrollController.jumpTo(_scrollController.position.maxScrollExtent);
      }
    });
  }

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
            controller: _scrollController,
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
