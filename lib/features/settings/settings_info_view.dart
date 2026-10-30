import 'dart:io';

import 'package:flutter/material.dart';
import 'package:pub_semver/pub_semver.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../app/theme/app_theme.dart';
import '../../app/updates/app_update_dialog.dart';
import '../../app/updates/update_channel.dart';
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
  Version? _installedVersion;
  UpdateChannel? _channel;
  DateTime? _lastCheckedAt;
  bool _isChecking = false;
  bool _isShowingUpdate = false;
  bool _isSavingChannel = false;
  bool _isLoadingChannel = true;

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
  Future<void> _saveUpdateChannel(UpdateChannel channel) async {
    if (_isSavingChannel ||
        _isChecking ||
        _isShowingUpdate ||
        channel == _channel) {
      return;
    }
    final previousChannel = _channel;
    setState(() {
      _isSavingChannel = true;
      _channel = channel;
    });
    try {
      await _service.saveUpdateChannel(channel);
      if (!mounted) return;
      setState(() {
        _channel = channel;
        _lastCheckedAt = null;
        _isSavingChannel = false;
      });
      await _checkForUpdate();
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _isSavingChannel = false;
        _channel = previousChannel;
      });
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
          content: Text('Update-Kanal konnte nicht gespeichert werden.'),
        ),
      );
    }
  }

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
    if (_isChecking ||
        _isShowingUpdate ||
        _isSavingChannel ||
        _channel == null) {
      return;
    }
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
          SnackBar(
            content: Text(
              _isWaitingForChannel
                  ? 'Kein neueres Release im gewählten Kanal verfügbar.'
                  : 'Mashin ist auf dem neuesten Stand.',
            ),
          ),
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
  bool get _isWaitingForChannel {
    final installed = _installedVersion;
    final channel = _channel;
    return installed != null &&
        channel != null &&
        !channel.allows(installed, isPrerelease: installed.isPreRelease);
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final isCompact = MediaQuery.sizeOf(context).width < 720;
    final isBusy =
        _isChecking ||
        _isShowingUpdate ||
        _isSavingChannel ||
        _isLoadingChannel;
    final lastCheckedAt = _lastCheckedAt;
    final localizations = MaterialLocalizations.of(context);
    final channelField = DropdownButtonFormField<UpdateChannel>(
      key: ValueKey(_channel),
      initialValue: _channel,
      isExpanded: true,
      decoration: InputDecoration(
        hintText: _isLoadingChannel ? 'Wird geladen ...' : 'Nicht verfügbar',
        isDense: !isCompact,
        filled: !isCompact,
        fillColor: colors.bgSecondary,
        contentPadding: !isCompact
            ? const EdgeInsets.symmetric(horizontal: 12, vertical: 0)
            : null,
        constraints: !isCompact
            ? const BoxConstraints.tightFor(height: 36)
            : null,
        prefixIcon: const Icon(Icons.system_update_alt, size: 18),
        prefixIconConstraints: !isCompact
            ? const BoxConstraints(minWidth: 38, minHeight: 36)
            : null,
        border: !isCompact
            ? OutlineInputBorder(
                borderRadius: BorderRadius.circular(6),
                borderSide: BorderSide.none,
              )
            : null,
      ),
      items: const [
        DropdownMenuItem(value: UpdateChannel.stable, child: Text('Stabil')),
        DropdownMenuItem(value: UpdateChannel.beta, child: Text('Beta')),
        DropdownMenuItem(value: UpdateChannel.dev, child: Text('Dev')),
      ],
      onChanged: isBusy || _channel == null
          ? null
          : (channel) {
              if (channel != null) _saveUpdateChannel(channel);
            },
    );
    final updateDetails = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text('Update-Prüfung', style: Theme.of(context).textTheme.titleMedium),
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
      onPressed: isBusy
          ? null
          : (_channel == null ? _loadVersion : _checkForUpdate),
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
          : const EdgeInsets.only(top: 12, bottom: 32),
      children: [
        Text('App', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 20),
        Padding(
          padding: EdgeInsets.symmetric(horizontal: isCompact ? 0 : 24),
          child: Align(
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
        ),
        const SizedBox(height: 28),
        Divider(color: colors.border),
        const SizedBox(height: 28),
        Text('Updates', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 20),
        Padding(
          padding: EdgeInsets.symmetric(horizontal: isCompact ? 0 : 24),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (isCompact) ...[
                Text(
                  'Update-Kanal',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
                const SizedBox(height: 12),
                channelField,
              ] else
                Row(
                  children: [
                    Expanded(
                      child: Text(
                        'Update-Kanal',
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                    ),
                    const SizedBox(width: 24),
                    SizedBox(width: 280, child: channelField),
                  ],
                ),
              if (_isWaitingForChannel) ...[
                const SizedBox(height: 12),
                Text(
                  'Die installierte Vorabversion bleibt bestehen, bis ein neueres Release im gewählten Kanal verfügbar ist.',
                  style: TextStyle(color: colors.textSecondary),
                ),
              ],
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
          ),
        ),
      ],
    );
  }

  // Installed version loading uses the same metadata as update discovery.
  Future<void> _loadVersion() async {
    setState(() => _isLoadingChannel = true);
    try {
      final version = await _service.getInstalledVersion();
      if (!mounted) return;
      setState(() {
        _version = '$version';
        _installedVersion = version;
      });
      final channel = await _service.getUpdateChannel();
      if (mounted) {
        setState(() {
          _channel = channel;
          _isLoadingChannel = false;
        });
      }
    } catch (error) {
      debugPrint('App version unavailable: $error');
      if (mounted) {
        setState(() {
          _isLoadingChannel = false;
          if (_installedVersion == null) _version = 'Nicht verfügbar';
        });
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Update-Einstellungen konnten nicht geladen werden.'),
          ),
        );
      }
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
