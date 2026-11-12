import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/layout/layout_utils.dart';
import '../../app/session/app_session_controller.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import 'settings_screen_desktop.dart';
import 'settings_screen_mobile.dart';

class SettingsScreen extends StatelessWidget {
  const SettingsScreen({super.key, required this.session});

  final AppSessionController session;

  @override
  Widget build(BuildContext context) => SettingsEditor(
    coreBridge: session.coreBridge,
    musicAssistantUrl: session.baseUrl,
    settings: SessionSettingsUpdate(
      themePreference: session.themePreference,
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
    required this.coreBridge,
    required this.musicAssistantUrl,
    required this.settings,
    required this.onSave,
    required this.onLogout,
  });

  final MashinCoreBridgeApi coreBridge;
  final String musicAssistantUrl;
  final SessionSettingsUpdate settings;
  final Future<void> Function(SessionSettingsUpdate) onSave;
  final Future<void> Function() onLogout;

  @override
  State<SettingsEditor> createState() => _SettingsEditorState();
}

enum _CacheArea { metadata, images }

class _SettingsEditorState extends State<SettingsEditor> {
  late SessionSettingsUpdate _savedSettings;
  late SessionSettingsUpdate _settings;
  Future<void> _pendingSave = Future<void>.value();
  bool _isLoggingOut = false;
  final _cacheContentKey = GlobalKey();
  CacheUsage? _cacheUsage;
  Timer? _cacheRefreshTimer;
  bool _isReadingCacheUsage = false;
  bool _hasCacheReadError = false;
  _CacheArea? _clearingCacheArea;

  // Editor construction and lifecycle.
  @override
  void initState() {
    super.initState();
    _savedSettings = widget.settings;
    _settings = widget.settings;
    unawaited(_refreshCacheUsage());
    _cacheRefreshTimer = Timer.periodic(const Duration(seconds: 5), (_) {
      final cacheContext = _cacheContentKey.currentContext;
      if (cacheContext != null && TickerMode.valuesOf(cacheContext).enabled) {
        unawaited(_refreshCacheUsage());
      }
    });
  }

  @override
  void dispose() {
    _cacheRefreshTimer?.cancel();
    super.dispose();
  }

  // Ordered settings persistence.
  void _saveSettings(SessionSettingsUpdate update) {
    if (update.themePreference == _settings.themePreference &&
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

  void _saveTheme(ThemePreference value) {
    setState(
      () => _saveSettings(
        SessionSettingsUpdate(
          themePreference: value,
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
          themePreference: _settings.themePreference,
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
          themePreference: _settings.themePreference,
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

  // Cache maintenance uses the native drain and invalidation commands.
  Future<void> _clearCache(_CacheArea area) async {
    if (_isReadingCacheUsage || _clearingCacheArea != null) return;
    setState(() => _clearingCacheArea = area);
    try {
      if (area == _CacheArea.metadata) {
        await widget.coreBridge.clearDatabaseCache();
      } else {
        await widget.coreBridge.clearImageCache();
      }
    } catch (_) {
      if (mounted) _showMessage('Cache konnte nicht geleert werden.');
    } finally {
      if (mounted) {
        setState(() => _clearingCacheArea = null);
        await _refreshCacheUsage();
      }
    }
  }

  // Physical usage can change again immediately while the cache refills.
  Future<void> _refreshCacheUsage() async {
    if (_isReadingCacheUsage || _clearingCacheArea != null) return;
    setState(() => _isReadingCacheUsage = true);
    try {
      final usage = await widget.coreBridge.getCacheUsage();
      if (mounted) {
        setState(() {
          _cacheUsage = usage;
          _hasCacheReadError = false;
        });
      }
    } catch (_) {
      if (mounted) setState(() => _hasCacheReadError = true);
    } finally {
      if (mounted) setState(() => _isReadingCacheUsage = false);
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
            cacheContent: _buildCacheContent(),
            musicAssistantUrl: widget.musicAssistantUrl,
            onLogout: _isLoggingOut ? null : _logout,
            themePreference: _settings.themePreference,
            logLevel: _settings.logLevel,
            onThemeChanged: _saveTheme,
            onLogLevelChanged: _saveLogLevel,
            shouldShowMultiDeviceButton: _settings.shouldShowMultiDeviceButton,
            onMultiDeviceButtonVisibilityChanged:
                _saveMultiDeviceButtonVisibility,
          )
        : SettingsScreenDesktop(
            cacheContent: _buildCacheContent(),
            musicAssistantUrl: widget.musicAssistantUrl,
            themePreference: _settings.themePreference,
            logLevel: _settings.logLevel,
            onThemeChanged: _saveTheme,
            onLogLevelChanged: _saveLogLevel,
            shouldShowMultiDeviceButton: _settings.shouldShowMultiDeviceButton,
            onMultiDeviceButtonVisibilityChanged:
                _saveMultiDeviceButtonVisibility,
          );
    return screen;
  }

  // Cache rows inherit the section spacing and indentation of each layout.
  Widget _buildCacheContent() => Column(
    key: _cacheContentKey,
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      _buildCacheRow(
        'Metadaten-Cache',
        _CacheArea.metadata,
        _cacheUsage?.metadataBytes,
      ),
      _buildCacheRow('Bild-Cache', _CacheArea.images, _cacheUsage?.imageBytes),
      if (_hasCacheReadError)
        Row(
          children: [
            const Expanded(child: Text('Cache-Größe nicht verfügbar.')),
            IconButton(
              tooltip: 'Cache-Größe erneut laden',
              onPressed: _isReadingCacheUsage ? null : _refreshCacheUsage,
              icon: const Icon(Icons.refresh),
            ),
          ],
        ),
    ],
  );

  Widget _buildCacheRow(String title, _CacheArea area, int? bytes) => ListTile(
    contentPadding: EdgeInsets.zero,
    title: Text(title),
    subtitle: Text(
      bytes == null
          ? (_hasCacheReadError ? 'Nicht verfügbar' : 'Wird ermittelt...')
          : _formatCacheBytes(bytes),
    ),
    trailing: SizedBox(
      width: 48,
      height: 48,
      child: _clearingCacheArea == area
          ? const Padding(
              padding: EdgeInsets.all(14),
              child: CircularProgressIndicator(strokeWidth: 2),
            )
          : IconButton(
              tooltip: '$title leeren',
              onPressed: _isReadingCacheUsage || _clearingCacheArea != null
                  ? null
                  : () => _clearCache(area),
              icon: const Icon(Icons.delete_outline),
            ),
    ),
  );

  // Byte-count presentation.
  String _formatCacheBytes(int bytes) {
    if (bytes < 1024) return '$bytes B';
    var value = bytes.toDouble();
    const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
    var unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit++;
    }
    return '${value.toStringAsFixed(1).replaceAll('.', ',')} ${units[unit]}';
  }
}
