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
  String _status = 'Noch nicht auf Updates geprüft.';
  bool _isChecking = false;
  bool _isShowingUpdate = false;
  bool _hasError = false;
  AppUpdate? _availableUpdate;

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
    setState(() {
      _isChecking = true;
      _hasError = false;
      _availableUpdate = null;
      _status = 'Updates werden gesucht ...';
    });
    try {
      final update = await _service.checkForUpdate();
      if (!mounted) return;
      setState(() {
        _isChecking = false;
        _availableUpdate = update;
        _status = update == null
            ? 'Mashin ist auf dem neuesten Stand.'
            : 'Update ${update.version} verfügbar';
      });
      if (update != null) await _showUpdate(update);
    } catch (error) {
      debugPrint('Manual update check failed: $error');
      if (!mounted) return;
      setState(() {
        _isChecking = false;
        _hasError = true;
        _status = 'Update-Prüfung fehlgeschlagen. Bitte erneut versuchen.';
      });
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
    final theme = Theme.of(context);
    final colors = context.mashinColors;
    final isBusy = _isChecking || _isShowingUpdate;
    final platform = switch (Platform.operatingSystem) {
      'windows' => 'Windows',
      'android' => 'Android',
      'linux' => 'Linux',
      'macos' => 'macOS',
      'ios' => 'iOS',
      final name => name,
    };
    return ListView(
      padding: MediaQuery.sizeOf(context).width >= 720
          ? const EdgeInsets.symmetric(vertical: 24)
          : const EdgeInsets.all(20),
      children: [
        Text(
          'Mashin',
          style: theme.textTheme.headlineSmall?.copyWith(
            fontWeight: FontWeight.w600,
          ),
        ),
        const SizedBox(height: 8),
        Text(
          'Music Assistant Client',
          style: TextStyle(color: colors.textSecondary),
        ),
        const SizedBox(height: 28),
        Text('Version', style: theme.textTheme.titleSmall),
        const SizedBox(height: 6),
        SelectableText(_version),
        const SizedBox(height: 20),
        Text('Plattform', style: theme.textTheme.titleSmall),
        const SizedBox(height: 6),
        Text(platform),
        const SizedBox(height: 20),
        Text('Projekt', style: theme.textTheme.titleSmall),
        const SizedBox(height: 6),
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton.icon(
            onPressed: _openProject,
            icon: const Icon(Icons.open_in_new, size: 18),
            label: const Text('github.com/joie96/mashin'),
            style: TextButton.styleFrom(foregroundColor: colors.textPrimary),
          ),
        ),
        const SizedBox(height: 24),
        Divider(color: colors.border),
        const SizedBox(height: 24),
        Text('Updates', style: theme.textTheme.titleMedium),
        const SizedBox(height: 12),
        Text(
          _status,
          style: TextStyle(
            color: _hasError ? theme.colorScheme.error : colors.textSecondary,
          ),
        ),
        const SizedBox(height: 20),
        Wrap(
          spacing: 12,
          runSpacing: 12,
          children: [
            OutlinedButton.icon(
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
              label: const Text('Nach Updates suchen'),
              style: OutlinedButton.styleFrom(
                foregroundColor: colors.textPrimary,
              ),
            ),
            FilledButton.icon(
              onPressed: isBusy || _availableUpdate == null
                  ? null
                  : () => _showUpdate(_availableUpdate!),
              icon: const Icon(Icons.system_update_alt, size: 18),
              label: const Text('Update'),
            ),
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
