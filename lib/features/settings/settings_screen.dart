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
      isDarkTheme: session.isDarkTheme,
      logLevel: session.logLevel,
      shouldShowMultiDeviceButton: session.shouldShowMultiDeviceButton,
    ),
    onSave: session.saveSessionSettings,
    onLogout: session.logout,
  );
}

class SettingsEditor extends StatefulWidget {
  const SettingsEditor({
    super.key,
    required this.musicAssistantUrl,
    required this.settings,
    required this.onSave,
    required this.onLogout,
  });

  final String musicAssistantUrl;
  final SessionSettingsUpdate settings;
  final Future<void> Function(SessionSettingsUpdate) onSave;
  final Future<void> Function() onLogout;

  @override
  State<SettingsEditor> createState() => _SettingsEditorState();
}

class _SettingsEditorState extends State<SettingsEditor> {
  late SessionSettingsUpdate _savedSettings;
  late SessionSettingsUpdate _settings;
  Future<void> _pendingSave = Future<void>.value();
  bool _isLoggingOut = false;

  // Editor construction and lifecycle.
  @override
  void initState() {
    super.initState();
    _savedSettings = widget.settings;
    _settings = widget.settings;
  }

  // Ordered settings persistence.
  void _saveSettings(SessionSettingsUpdate update) {
    if (update.isDarkTheme == _settings.isDarkTheme &&
        update.logLevel == _settings.logLevel &&
        update.shouldShowMultiDeviceButton ==
            _settings.shouldShowMultiDeviceButton) {
      return;
    }
    _settings = update;
    _pendingSave = _pendingSave.then((_) async {
      try {
        await widget.onSave(update);
        _savedSettings = update;
      } catch (_) {
        if (mounted) {
          if (identical(_settings, update)) {
            setState(() => _settings = _savedSettings);
          }
          _showMessage(
            'Einstellungen konnten nicht gespeichert werden. Verbindung und Eingaben pruefen.',
          );
        }
      }
    });
  }

  void _saveTheme(bool value) {
    setState(
      () => _saveSettings(
        SessionSettingsUpdate(
          isDarkTheme: value,
          logLevel: _settings.logLevel,
          shouldShowMultiDeviceButton: _settings.shouldShowMultiDeviceButton,
        ),
      ),
    );
  }

  void _saveLogLevel(LogLevelPreference value) {
    setState(
      () => _saveSettings(
        SessionSettingsUpdate(
          isDarkTheme: _settings.isDarkTheme,
          logLevel: value,
          shouldShowMultiDeviceButton: _settings.shouldShowMultiDeviceButton,
        ),
      ),
    );
  }

  void _saveMultiDeviceButtonVisibility(bool value) {
    setState(
      () => _saveSettings(
        SessionSettingsUpdate(
          isDarkTheme: _settings.isDarkTheme,
          logLevel: _settings.logLevel,
          shouldShowMultiDeviceButton: value,
        ),
      ),
    );
  }

  Future<void> _logout() async {
    if (_isLoggingOut) return;
    setState(() => _isLoggingOut = true);
    try {
      await _pendingSave;
      await widget.onLogout();
    } catch (_) {
      if (mounted) {
        _showMessage('Abmelden fehlgeschlagen. Bitte erneut versuchen.');
      }
    } finally {
      if (mounted) setState(() => _isLoggingOut = false);
    }
  }

  void _showMessage(String message) => ScaffoldMessenger.of(
    context,
  ).showSnackBar(SnackBar(content: Text(message)));

  // Responsive presentation with shared settings across tabs and layouts.
  @override
  Widget build(BuildContext context) {
    final screen = isCompactLayout(context)
        ? SettingsScreenMobile(
            musicAssistantUrl: widget.musicAssistantUrl,
            onLogout: _isLoggingOut ? null : _logout,
            isDarkTheme: _settings.isDarkTheme,
            logLevel: _settings.logLevel,
            onThemeChanged: _saveTheme,
            onLogLevelChanged: _saveLogLevel,
            shouldShowMultiDeviceButton: _settings.shouldShowMultiDeviceButton,
            onMultiDeviceButtonVisibilityChanged:
                _saveMultiDeviceButtonVisibility,
          )
        : SettingsScreenDesktop(
            musicAssistantUrl: widget.musicAssistantUrl,
            isDarkTheme: _settings.isDarkTheme,
            logLevel: _settings.logLevel,
            onThemeChanged: _saveTheme,
            onLogLevelChanged: _saveLogLevel,
            shouldShowMultiDeviceButton: _settings.shouldShowMultiDeviceButton,
            onMultiDeviceButtonVisibilityChanged:
                _saveMultiDeviceButtonVisibility,
          );
    return screen;
  }
}
