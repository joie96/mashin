import 'package:flutter/material.dart';

import '../../app/layout/layout_utils.dart';
import '../../app/session/app_session_controller.dart';
import '../core/core_models.dart';
import 'settings_screen_desktop.dart';
import 'settings_screen_mobile.dart';

class SettingsScreen extends StatelessWidget {
  const SettingsScreen({super.key, required this.session});

  final AppSessionController session;

  @override
  Widget build(BuildContext context) => SettingsEditor(
    musicAssistantUrl: session.baseUrl,
    settings: SessionSettingsUpdate(
      localCacheSizeBytes: session.localCacheSizeBytes,
      isDarkTheme: session.isDarkTheme,
      logLevel: session.logLevel,
    ),
    onSave: session.saveSessionSettings,
  );
}

class SettingsEditor extends StatefulWidget {
  const SettingsEditor({
    super.key,
    required this.musicAssistantUrl,
    required this.settings,
    required this.onSave,
  });

  final String musicAssistantUrl;
  final SessionSettingsUpdate settings;
  final Future<void> Function(SessionSettingsUpdate) onSave;

  @override
  State<SettingsEditor> createState() => _SettingsEditorState();
}

class _SettingsEditorState extends State<SettingsEditor> {
  late final TextEditingController _cacheSizeController;
  late SessionSettingsUpdate _savedSettings;
  late bool _isDarkTheme;
  late LogLevelPreference _logLevel;
  bool _isSaving = false;

  // Draft construction and lifecycle.
  @override
  void initState() {
    super.initState();
    _cacheSizeController = TextEditingController();
    _savedSettings = widget.settings;
    _restoreDraft();
  }

  @override
  void dispose() {
    _cacheSizeController.dispose();
    super.dispose();
  }

  // Draft commands; no setting is applied before the single save operation.
  void _restoreDraft() {
    _cacheSizeController.text =
        '${_savedSettings.localCacheSizeBytes ~/ _bytesPerMegabyte}';
    _isDarkTheme = _savedSettings.isDarkTheme;
    _logLevel = _savedSettings.logLevel;
  }

  void _cancel() {
    FocusScope.of(context).unfocus();
    setState(_restoreDraft);
  }

  Future<void> _save() async {
    final int? cacheSizeMb = int.tryParse(_cacheSizeController.text.trim());
    if (cacheSizeMb == null || cacheSizeMb < 25 || cacheSizeMb > 1000) {
      _showMessage('Cache-Groesse muss zwischen 25 und 1000 MB liegen.');
      return;
    }
    final update = SessionSettingsUpdate(
      localCacheSizeBytes: cacheSizeMb * _bytesPerMegabyte,
      isDarkTheme: _isDarkTheme,
      logLevel: _logLevel,
    );
    FocusScope.of(context).unfocus();
    setState(() => _isSaving = true);
    try {
      await widget.onSave(update);
      if (!mounted) {
        return;
      }
      _savedSettings = update;
      _showMessage('Einstellungen gespeichert.');
    } catch (_) {
      if (mounted) {
        _showMessage(
          'Einstellungen konnten nicht gespeichert werden. Verbindung und Eingaben pruefen.',
        );
      }
    } finally {
      if (mounted) {
        setState(() => _isSaving = false);
      }
    }
  }

  void _showMessage(String message) => ScaffoldMessenger.of(
    context,
  ).showSnackBar(SnackBar(content: Text(message)));

  // Responsive presentation with one shared draft across tabs and layouts.
  @override
  Widget build(BuildContext context) {
    final screen = isCompactLayout(context)
        ? SettingsScreenMobile(
            musicAssistantUrl: widget.musicAssistantUrl,
            cacheSizeController: _cacheSizeController,
            isDarkTheme: _isDarkTheme,
            logLevel: _logLevel,
            onThemeChanged: (value) => setState(() => _isDarkTheme = value),
            onLogLevelChanged: (value) => setState(() => _logLevel = value),
            onSave: _save,
            onCancel: _cancel,
            isSaving: _isSaving,
          )
        : SettingsScreenDesktop(
            musicAssistantUrl: widget.musicAssistantUrl,
            cacheSizeController: _cacheSizeController,
            isDarkTheme: _isDarkTheme,
            logLevel: _logLevel,
            onThemeChanged: (value) => setState(() => _isDarkTheme = value),
            onLogLevelChanged: (value) => setState(() => _logLevel = value),
            onSave: _save,
            onCancel: _cancel,
            isSaving: _isSaving,
          );
    return screen;
  }
}

const int _bytesPerMegabyte = 1024 * 1024;
