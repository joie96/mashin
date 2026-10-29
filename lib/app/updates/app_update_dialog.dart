import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:url_launcher/url_launcher.dart';

import 'app_update_installer.dart';
import 'app_update_service.dart';

/// Checks once per app start without delaying session restoration or navigation.
class AppUpdateHost extends StatefulWidget {
  const AppUpdateHost({
    super.key,
    required this.navigatorKey,
    required this.child,
    this.isReady = true,
    this.service,
  });

  final GlobalKey<NavigatorState> navigatorKey;
  final Widget child;
  final bool isReady;
  final AppUpdateService? service;

  @override
  State<AppUpdateHost> createState() => _AppUpdateHostState();
}

class _AppUpdateHostState extends State<AppUpdateHost> {
  late final _service = widget.service ?? AppUpdateService();
  AppUpdate? _pendingUpdate;
  bool _hasScheduledDialog = false;

  // Startup and request lifetime.
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback(
      (_) => unawaited(_checkForUpdate()),
    );
  }

  @override
  void didUpdateWidget(covariant AppUpdateHost oldWidget) {
    super.didUpdateWidget(oldWidget);
    _showWhenReady();
  }

  @override
  void dispose() {
    _service.cancelDownload();
    super.dispose();
  }

  Future<void> _checkForUpdate() async {
    try {
      _pendingUpdate = await _service.checkAtStartup();
      if (mounted) _showWhenReady();
    } catch (error) {
      debugPrint('Update check failed: $error');
    }
  }

  // Pageless dialogs must wait until the router has replaced its loading page.
  void _showWhenReady() {
    final update = _pendingUpdate;
    if (!widget.isReady || update == null || _hasScheduledDialog) return;
    _hasScheduledDialog = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      final context = widget.navigatorKey.currentState?.overlay?.context;
      if (context == null || !context.mounted) return;
      unawaited(
        showDialog<void>(
          context: context,
          barrierDismissible: false,
          builder: (_) => AppUpdateDialog(update: update, service: _service),
        ),
      );
    });
    WidgetsBinding.instance.ensureVisualUpdate();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}

class AppUpdateDialog extends StatefulWidget {
  const AppUpdateDialog({
    super.key,
    required this.update,
    required this.service,
    this.install = AppUpdateInstaller.install,
  });

  final AppUpdate update;
  final AppUpdateService service;
  final Future<void> Function(File file) install;

  @override
  State<AppUpdateDialog> createState() => _AppUpdateDialogState();
}

class _AppUpdateDialogState extends State<AppUpdateDialog> {
  bool _isDownloading = false;
  bool _isInstalling = false;
  double _progress = 0;
  String? _error;
  File? _installerFile;

  // Dialog lifetime and update commands.
  @override
  void dispose() {
    widget.service.cancelDownload();
    super.dispose();
  }

  Future<void> _startUpdate() async {
    setState(() {
      _isDownloading = _installerFile == null;
      _isInstalling = _installerFile != null;
      _error = null;
      _progress = 0;
    });
    try {
      _installerFile ??= await widget.service.download(
        widget.update,
        onProgress: (progress) {
          if (mounted) setState(() => _progress = progress);
        },
      );
      if (!mounted) return;
      setState(() {
        _isDownloading = false;
        _isInstalling = true;
      });
      await widget.install(_installerFile!);
      if (mounted) Navigator.of(context).pop();
    } on UpdateCancelled {
      return;
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _isDownloading = false;
        _isInstalling = false;
        _error = switch (error) {
          PlatformException(:final message) =>
            message ?? 'Installation fehlgeschlagen.',
          _ => 'Update fehlgeschlagen. Bitte erneut versuchen.\n$error',
        };
      });
    }
  }

  Future<void> _openLink(String? href) async {
    final uri = Uri.tryParse(href ?? '');
    if (uri == null || uri.scheme != 'https') return;
    try {
      if (await launchUrl(uri, mode: LaunchMode.externalApplication)) return;
    } catch (error) {
      debugPrint('Release link failed: $error');
    }
    if (mounted) {
      setState(() => _error = 'Der Release-Link konnte nicht geöffnet werden.');
    }
  }

  // Release notes and download state.
  @override
  Widget build(BuildContext context) {
    final isBusy = _isDownloading || _isInstalling;
    final colors = Theme.of(context).colorScheme;
    return PopScope(
      canPop: !_isInstalling,
      child: AlertDialog(
        title: Text('Update ${widget.update.version} verfügbar'),
        content: SizedBox(
          width: 520,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Flexible(
                child: ConstrainedBox(
                  constraints: BoxConstraints(
                    maxHeight: MediaQuery.sizeOf(context).height * 0.45,
                  ),
                  child: SingleChildScrollView(
                    child: MarkdownBody(
                      data: widget.update.notes.isEmpty
                          ? 'Keine Release-Notizen vorhanden.'
                          : widget.update.notes,
                      selectable: true,
                      onTapLink: (_, href, _) => unawaited(_openLink(href)),
                    ),
                  ),
                ),
              ),
              if (isBusy) ...[
                const SizedBox(height: 20),
                LinearProgressIndicator(
                  value: _isInstalling ? null : _progress,
                ),
                const SizedBox(height: 8),
                Text(
                  _isInstalling
                      ? 'Installation wird geöffnet ...'
                      : 'Download: ${(_progress * 100).floor()} %',
                ),
              ],
              if (_error != null) ...[
                const SizedBox(height: 16),
                Text(_error!, style: TextStyle(color: colors.error)),
              ],
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: _isInstalling
                ? null
                : () {
                    widget.service.cancelDownload();
                    Navigator.of(context).pop();
                  },
            child: const Text('Abbrechen'),
          ),
          FilledButton(
            onPressed: isBusy ? null : _startUpdate,
            style: FilledButton.styleFrom(backgroundColor: colors.primary),
            child: const Text('Update'),
          ),
        ],
      ),
    );
  }
}
