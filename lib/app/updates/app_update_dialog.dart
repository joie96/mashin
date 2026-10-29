import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:url_launcher/url_launcher.dart';

import '../theme/app_theme.dart';
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
      _pendingUpdate = await _service.checkForUpdate();
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
  final _notesScrollController = ScrollController();
  bool _isDownloading = false;
  bool _isInstalling = false;
  double _progress = 0;
  String? _error;
  File? _installerFile;

  // Dialog lifetime and update commands.
  @override
  void dispose() {
    widget.service.cancelDownload();
    _notesScrollController.dispose();
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
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    final palette =
        theme.extension<MashinColors>() ??
        (theme.brightness == Brightness.dark
            ? const MashinColors.dark()
            : const MashinColors.light());
    final viewport = MediaQuery.sizeOf(context);
    final isDesktop = viewport.width >= 720;
    final padding = viewport.height < 480 ? 12.0 : (isDesktop ? 28.0 : 20.0);
    final markdownTheme = theme.copyWith(
      colorScheme: colors.copyWith(primary: palette.textPrimary),
    );
    const changelogPrefix = '**Full Changelog**: ';
    final changelogLine = widget.update.notes
        .split('\n')
        .where((line) => line.startsWith(changelogPrefix))
        .firstOrNull;
    final changelogUrl = changelogLine == null
        ? 'https://github.com/joie96/mashin/releases/tag/v${widget.update.version}'
        : changelogLine.substring(changelogPrefix.length).trim();
    return PopScope(
      canPop: !_isInstalling,
      child: Dialog(
        backgroundColor: palette.bgSecondary,
        surfaceTintColor: Colors.transparent,
        insetPadding: EdgeInsets.symmetric(
          horizontal: isDesktop ? 40 : 16,
          vertical: 24,
        ),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
        child: SizedBox(
          width: 880,
          height: (viewport.height * 0.84).clamp(0, 760).toDouble(),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: EdgeInsets.all(padding),
                child: Text(
                  'Update ${widget.update.version} verfügbar',
                  style: theme.textTheme.titleLarge?.copyWith(
                    color: palette.textPrimary,
                    fontSize: isDesktop ? 24 : 20,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              Divider(height: 1, color: palette.border),
              Expanded(
                child: Scrollbar(
                  controller: _notesScrollController,
                  thumbVisibility: true,
                  child: SingleChildScrollView(
                    controller: _notesScrollController,
                    padding: EdgeInsets.all(padding),
                    child: Theme(
                      data: markdownTheme,
                      child: MarkdownBody(
                        data: widget.update.notes.isEmpty
                            ? 'Keine Release-Notizen vorhanden.'
                            : widget.update.notes,
                        styleSheet: MarkdownStyleSheet.fromTheme(markdownTheme),
                        selectable: true,
                        onTapLink: (_, href, _) => unawaited(_openLink(href)),
                      ),
                    ),
                  ),
                ),
              ),
              Divider(height: 1, color: palette.border),
              Padding(
                padding: EdgeInsets.all(padding),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Align(
                      alignment: Alignment.centerLeft,
                      child: TextButton.icon(
                        onPressed: () => unawaited(_openLink(changelogUrl)),
                        icon: const Icon(Icons.open_in_new, size: 16),
                        label: const Text('See full changelog'),
                        style: TextButton.styleFrom(
                          foregroundColor: palette.textSecondary,
                          padding: const EdgeInsets.symmetric(horizontal: 8),
                        ),
                      ),
                    ),
                    if (isBusy) ...[
                      const SizedBox(height: 12),
                      Text(
                        _isInstalling
                            ? 'Installation wird geöffnet ...'
                            : 'Download: ${(_progress * 100).floor()} %',
                        style: theme.textTheme.bodyMedium?.copyWith(
                          color: palette.textPrimary,
                        ),
                      ),
                      const SizedBox(height: 10),
                      LinearProgressIndicator(
                        value: _isInstalling ? null : _progress,
                        minHeight: 6,
                        color: palette.textPrimary,
                        backgroundColor: palette.selection,
                        borderRadius: BorderRadius.circular(3),
                        stopIndicatorColor: Colors.transparent,
                      ),
                    ],
                    if (_error != null) ...[
                      const SizedBox(height: 12),
                      ConstrainedBox(
                        constraints: const BoxConstraints(maxHeight: 72),
                        child: SingleChildScrollView(
                          child: Text(
                            _error!,
                            style: TextStyle(color: colors.error),
                          ),
                        ),
                      ),
                    ],
                    const SizedBox(height: 16),
                    OverflowBar(
                      alignment: MainAxisAlignment.end,
                      overflowAlignment: OverflowBarAlignment.end,
                      spacing: 12,
                      overflowSpacing: 8,
                      children: [
                        TextButton(
                          onPressed: _isInstalling
                              ? null
                              : () {
                                  widget.service.cancelDownload();
                                  Navigator.of(context).pop();
                                },
                          style: TextButton.styleFrom(
                            foregroundColor: palette.textSecondary,
                          ),
                          child: const Text('Abbrechen'),
                        ),
                        FilledButton(
                          onPressed: isBusy ? null : _startUpdate,
                          style: FilledButton.styleFrom(
                            backgroundColor: colors.primary,
                            foregroundColor: colors.onPrimary,
                          ),
                          child: const Text('Update'),
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
