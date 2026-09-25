import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../core/core_models.dart';
import 'settings_log_view.dart';

class SettingsScreenMobile extends StatelessWidget {
  const SettingsScreenMobile({
    super.key,
    required this.musicAssistantUrl,
    required this.cacheSizeController,
    required this.isDarkTheme,
    required this.logLevel,
    required this.onCacheSizeChanged,
    required this.onThemeChanged,
    required this.onLogLevelChanged,
    required this.onSaveLogSnapshot,
  });

  final String musicAssistantUrl;
  final TextEditingController cacheSizeController;
  final bool isDarkTheme;
  final LogLevelPreference logLevel;
  final ValueChanged<String> onCacheSizeChanged;
  final ValueChanged<bool> onThemeChanged;
  final ValueChanged<LogLevelPreference> onLogLevelChanged;
  final Future<String> Function() onSaveLogSnapshot;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return DefaultTabController(
      length: 2,
      child: Scaffold(
        backgroundColor: colors.bgPrimary,
        appBar: AppBar(
          title: const Text('Einstellungen'),
          backgroundColor: colors.bgPrimary,
          surfaceTintColor: Colors.transparent,
          bottom: const TabBar(
            tabs: <Widget>[
              Tab(text: 'Allgemein'),
              Tab(text: 'Log'),
            ],
          ),
        ),
        body: TabBarView(
          children: <Widget>[
            ListView(
              padding: const EdgeInsets.all(20),
              children: <Widget>[
                Text('Server', style: Theme.of(context).textTheme.titleMedium),
                const SizedBox(height: 12),
                TextFormField(
                  initialValue: musicAssistantUrl,
                  readOnly: true,
                  decoration: const InputDecoration(
                    labelText: 'Music Assistant URL',
                    prefixIcon: Icon(Icons.dns_outlined),
                  ),
                ),
                const SizedBox(height: 10),
                Text(
                  'Die URL wird mit der Anmeldung gespeichert.',
                  style: TextStyle(color: colors.textSecondary),
                ),
                const SizedBox(height: 32),
                Text(
                  'Lokaler Cache',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: cacheSizeController,
                  keyboardType: TextInputType.number,
                  onChanged: onCacheSizeChanged,
                  decoration: const InputDecoration(
                    labelText: 'Cache-Größe',
                    suffixText: 'MB',
                    prefixIcon: Icon(Icons.storage_outlined),
                  ),
                ),
                const SizedBox(height: 10),
                Text(
                  'Zwischen 25 und 1000 MB. Die Einstellung wird in der aktiven Sendspin-Konfiguration gespeichert.',
                  style: TextStyle(color: colors.textSecondary),
                ),
                const SizedBox(height: 32),
                Text('Theme', style: Theme.of(context).textTheme.titleMedium),
                const SizedBox(height: 12),
                SegmentedButton<bool>(
                  style: ButtonStyle(
                    backgroundColor: WidgetStateProperty.resolveWith<Color?>(
                      (Set<WidgetState> states) =>
                          states.contains(WidgetState.selected)
                          ? colors.selection
                          : colors.bgSecondary,
                    ),
                    foregroundColor: WidgetStatePropertyAll<Color>(
                      colors.textPrimary,
                    ),
                  ),
                  segments: const <ButtonSegment<bool>>[
                    ButtonSegment<bool>(
                      value: false,
                      icon: Icon(Icons.light_mode_outlined),
                      label: Text('Hell'),
                    ),
                    ButtonSegment<bool>(
                      value: true,
                      icon: Icon(Icons.dark_mode_outlined),
                      label: Text('Dunkel'),
                    ),
                  ],
                  selected: <bool>{isDarkTheme},
                  onSelectionChanged: (Set<bool> selection) =>
                      onThemeChanged(selection.first),
                ),
              ],
            ),
            Padding(
              padding: const EdgeInsets.all(20),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  Text(
                    'Protokollierung',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: 12),
                  Row(
                    children: <Widget>[
                      Expanded(
                        child: SettingsLogLevelField(
                          logLevel: logLevel,
                          onChanged: onLogLevelChanged,
                        ),
                      ),
                      const SizedBox(width: 8),
                      SettingsSaveLogButton(onSave: onSaveLogSnapshot),
                    ],
                  ),
                  const SizedBox(height: 32),
                  Text(
                    'Logausgabe',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: 12),
                  const Expanded(child: SettingsLogOutput()),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
