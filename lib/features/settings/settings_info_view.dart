import 'dart:io';

import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../app/theme/app_theme.dart';
import '../../app/updates/app_update_dialog.dart';
import '../../app/updates/app_update_service.dart';

class SettingsInfoView extends StatefulWidget {
  const SettingsInfoView({super.key, this.service, this.openProjectLink});

  final AppUpdateService? service;
  final Future<bool> Function(Uri uri)? openProjectLink;

  @override
  State<SettingsInfoView> createState() => _SettingsInfoViewState();
}

class _SettingsInfoViewState extends State<SettingsInfoView> {
  static final _projectUri = Uri.https('github.com', '/joie96/mashin');

  late final _service = widget.service ?? AppUpdateService();
  String _version = 'Wird geladen ...';
  DateTime? _lastCheckedAt;
  bool _isChecking = false;
  bool _isShowingUpdate = false;

  // App information and request lifetime.
  @override
  void initState() {
    super.initState();
    _loadVersion();
  }

  @override
  void dispose() {
    _service.cancelDownload();
    super.dispose();
  }

  // Project and update commands.
  Future<void> _openProject() async {
    try {
      final hasOpened =
          await (widget.openProjectLink?.call(_projectUri) ??
              launchUrl(_projectUri, mode: LaunchMode.externalApplication));
      if (hasOpened) return;
    } catch (error) {
      debugPrint('Project link failed: $error');
    }
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(content: Text('GitHub konnte nicht geöffnet werden.')),
    );
  }

  Future<void> _checkForUpdate() async {
    if (_isChecking || _isShowingUpdate) return;
    setState(() => _isChecking = true);
    try {
      final update = await _service.checkForUpdate();
      if (!mounted) return;
      setState(() {
        _isChecking = false;
        _lastCheckedAt = DateTime.now();
      });
      if (update != null) {
        await _showUpdate(update);
      } else {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Mashin ist auf dem neuesten Stand.')),
        );
      }
    } catch (error) {
      debugPrint('Manual update check failed: $error');
      if (!mounted) return;
      setState(() => _isChecking = false);
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
          content: Text(
            'Update-Prüfung fehlgeschlagen. Bitte erneut versuchen.',
          ),
        ),
      );
    }
  }

  Future<void> _showUpdate(AppUpdate update) async {
    if (_isShowingUpdate) return;
    setState(() => _isShowingUpdate = true);
    try {
      await showDialog<void>(
        context: context,
        barrierDismissible: false,
        builder: (_) => AppUpdateDialog(update: update, service: _service),
      );
    } finally {
      if (mounted) setState(() => _isShowingUpdate = false);
    }
  }

  // App information and update status.
  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final isCompact = MediaQuery.sizeOf(context).width < 720;
    final isBusy = _isChecking || _isShowingUpdate;
    final lastCheckedAt = _lastCheckedAt;
    final localizations = MaterialLocalizations.of(context);
    final updateDetails = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text('Updates', style: Theme.of(context).textTheme.titleMedium),
        if (lastCheckedAt != null) ...<Widget>[
          const SizedBox(height: 6),
          Text(
            'Zuletzt geprüft: ${localizations.formatShortDate(lastCheckedAt)}, '
            '${localizations.formatTimeOfDay(TimeOfDay.fromDateTime(lastCheckedAt), alwaysUse24HourFormat: MediaQuery.alwaysUse24HourFormatOf(context))}',
            style: TextStyle(color: colors.textSecondary),
          ),
        ],
      ],
    );
    final updateButton = OutlinedButton.icon(
      onPressed: isBusy ? null : _checkForUpdate,
      icon: _isChecking
          ? SizedBox.square(
              dimension: 18,
              child: CircularProgressIndicator(
                strokeWidth: 2,
                color: colors.textSecondary,
              ),
            )
          : const Icon(Icons.refresh, size: 18),
      label: const Text('Update suchen'),
      style: OutlinedButton.styleFrom(foregroundColor: colors.textPrimary),
    );
    final platform = switch (Platform.operatingSystem) {
      'windows' => 'Windows',
      'android' => 'Android',
      'linux' => 'Linux',
      'macos' => 'macOS',
      'ios' => 'iOS',
      final name => name,
    };
    return ListView(
      padding: isCompact
          ? const EdgeInsets.all(20)
          : const EdgeInsets.fromLTRB(24, 12, 24, 24),
      children: [
        Align(
          alignment: Alignment.centerLeft,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 640),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                _SettingsInfoRow(
                  label: 'Version',
                  isCompact: isCompact,
                  child: SelectableText(_version),
                ),
                SizedBox(height: isCompact ? 16 : 8),
                _SettingsInfoRow(
                  label: 'Plattform',
                  isCompact: isCompact,
                  child: Text(platform),
                ),
                SizedBox(height: isCompact ? 16 : 8),
                _SettingsInfoRow(
                  label: 'Projekt',
                  isCompact: isCompact,
                  child: TextButton.icon(
                    onPressed: _openProject,
                    icon: const Icon(Icons.open_in_new, size: 18),
                    label: const Text('github.com/joie96/mashin'),
                    style: TextButton.styleFrom(
                      foregroundColor: colors.textPrimary,
                      padding: EdgeInsets.zero,
                      minimumSize: const Size(0, 40),
                      alignment: Alignment.centerLeft,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 24),
        Divider(color: colors.border),
        const SizedBox(height: 24),
        if (isCompact)
          Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              updateDetails,
              const SizedBox(height: 12),
              updateButton,
            ],
          )
        else
          Row(
            children: <Widget>[
              Expanded(child: updateDetails),
              const SizedBox(width: 24),
              updateButton,
            ],
          ),
      ],
    );
  }

  // Installed version loading uses the same metadata as update discovery.
  Future<void> _loadVersion() async {
    try {
      final version = await _service.getInstalledVersion();
      if (mounted) setState(() => _version = '$version');
    } catch (error) {
      debugPrint('App version unavailable: $error');
      if (mounted) setState(() => _version = 'Nicht verfügbar');
    }
  }
}

class _SettingsInfoRow extends StatelessWidget {
  const _SettingsInfoRow({
    required this.label,
    required this.isCompact,
    required this.child,
  });

  final String label;
  final bool isCompact;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final title = Text(
      label,
      style: Theme.of(context).textTheme.bodyMedium?.copyWith(
        color: context.mashinColors.textSecondary,
        fontWeight: FontWeight.normal,
      ),
    );
    if (isCompact) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[title, const SizedBox(height: 6), child],
      );
    }
    return ConstrainedBox(
      constraints: const BoxConstraints(minHeight: 40),
      child: Row(
        children: <Widget>[
          SizedBox(width: 120, child: title),
          const SizedBox(width: 24),
          Expanded(
            child: Align(alignment: Alignment.centerLeft, child: child),
          ),
        ],
      ),
    );
  }
}
