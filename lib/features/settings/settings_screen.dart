import 'package:flutter/material.dart';

import '../../app/layout/layout_utils.dart';
import '../../app/session/app_session_controller.dart';
import 'settings_screen_desktop.dart';
import 'settings_screen_mobile.dart';

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key, required this.session});

  final AppSessionController session;

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  late final TextEditingController _cacheSizeController;

  @override
  void initState() {
    super.initState();
    _cacheSizeController = TextEditingController(
      text: '${widget.session.localCacheSizeBytes ~/ _bytesPerMegabyte}',
    );
  }

  @override
  void dispose() {
    _cacheSizeController.dispose();
    super.dispose();
  }

  Future<void> _setCacheSize(String value) async {
    final int? cacheSizeMb = int.tryParse(value.trim());
    if (cacheSizeMb == null || cacheSizeMb < 25 || cacheSizeMb > 1000) {
      return;
    }

    try {
      await widget.session.setLocalCacheSizeBytes(
        cacheSizeMb * _bytesPerMegabyte,
      );
    } catch (_) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Einstellung konnte nicht gespeichert werden.'),
          ),
        );
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final screen = isCompactLayout(context)
        ? SettingsScreenMobile(
            musicAssistantUrl: widget.session.baseUrl,
            cacheSizeController: _cacheSizeController,
            isDarkTheme: widget.session.isDarkTheme,
            logLevel: widget.session.logLevel,
            onCacheSizeChanged: _setCacheSize,
            onThemeChanged: widget.session.setDarkTheme,
            onLogLevelChanged: widget.session.setLogLevel,
          )
        : SettingsScreenDesktop(
            musicAssistantUrl: widget.session.baseUrl,
            cacheSizeController: _cacheSizeController,
            isDarkTheme: widget.session.isDarkTheme,
            logLevel: widget.session.logLevel,
            onCacheSizeChanged: _setCacheSize,
            onThemeChanged: widget.session.setDarkTheme,
            onLogLevelChanged: widget.session.setLogLevel,
          );
    return screen;
  }
}

const int _bytesPerMegabyte = 1024 * 1024;
