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
  late final TextEditingController _cacheSizeController;
  late final FocusNode _cacheSizeFocusNode;
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
    _cacheSizeController = TextEditingController(
      text: '${_settings.localCacheSizeBytes ~/ _bytesPerMegabyte}',
    );
    _cacheSizeFocusNode = FocusNode()..addListener(_onCacheSizeFocusChanged);
  }

  @override
  void dispose() {
    _saveCacheSize(shouldShowError: false);
    _cacheSizeFocusNode.removeListener(_onCacheSizeFocusChanged);
    _cacheSizeFocusNode.dispose();
    _cacheSizeController.dispose();
    super.dispose();
  }

  // Field commits and ordered persistence.
  void _onCacheSizeFocusChanged() {
    if (!_cacheSizeFocusNode.hasFocus) {
      _saveCacheSize();
    }
  }

  void _saveCacheSize({bool shouldShowError = true}) {
    final int? cacheSizeMb = int.tryParse(_cacheSizeController.text.trim());
    if (cacheSizeMb == null || cacheSizeMb < 25 || cacheSizeMb > 1000) {
      _cacheSizeController.text =
          '${_settings.localCacheSizeBytes ~/ _bytesPerMegabyte}';
      if (shouldShowError) {
        _showMessage('Cache-Groesse muss zwischen 25 und 1000 MB liegen.');
      }
      return;
    }
    _cacheSizeController.text = '$cacheSizeMb';
    _saveSettings(
      SessionSettingsUpdate(
        localCacheSizeBytes: cacheSizeMb * _bytesPerMegabyte,
        isDarkTheme: _settings.isDarkTheme,
        logLevel: _settings.logLevel,
        shouldShowMultiDeviceButton: _settings.shouldShowMultiDeviceButton,
      ),
    );
  }

  void _saveSettings(SessionSettingsUpdate update) {
    if (update.localCacheSizeBytes == _settings.localCacheSizeBytes &&
        update.isDarkTheme == _settings.isDarkTheme &&
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
            if (!_cacheSizeFocusNode.hasFocus) {
              _cacheSizeController.text =
                  '${_savedSettings.localCacheSizeBytes ~/ _bytesPerMegabyte}';
            }
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
          localCacheSizeBytes: _settings.localCacheSizeBytes,
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
          localCacheSizeBytes: _settings.localCacheSizeBytes,
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
          localCacheSizeBytes: _settings.localCacheSizeBytes,
          isDarkTheme: _settings.isDarkTheme,
          logLevel: _settings.logLevel,
          shouldShowMultiDeviceButton: value,
        ),
      ),
    );
  }

  Future<void> _logout() async {
    if (_isLoggingOut) return;
    _cacheSizeFocusNode.unfocus();
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
            cacheSizeController: _cacheSizeController,
            cacheSizeFocusNode: _cacheSizeFocusNode,
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
            cacheSizeController: _cacheSizeController,
            cacheSizeFocusNode: _cacheSizeFocusNode,
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

const int _bytesPerMegabyte = 1024 * 1024;
