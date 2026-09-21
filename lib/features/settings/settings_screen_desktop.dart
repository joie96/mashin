import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../core/core_models.dart';
import 'settings_log_view.dart';

const double _desktopSettingsControlWidth = 280;
const BoxConstraints _desktopSettingsFieldConstraints = BoxConstraints.tightFor(
  height: 36,
);
const BoxConstraints _desktopSettingsFieldIconConstraints = BoxConstraints(
  minWidth: 38,
  minHeight: 36,
);

InputDecoration _desktopSettingsFieldDecoration({
  required MashinColors colors,
  required IconData icon,
  String? suffixText,
}) => InputDecoration(
  isDense: true,
  filled: true,
  fillColor: colors.bgSecondary,
  contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 0),
  constraints: _desktopSettingsFieldConstraints,
  prefixIcon: Icon(icon, size: 18),
  prefixIconConstraints: _desktopSettingsFieldIconConstraints,
  suffixText: suffixText,
  border: OutlineInputBorder(
    borderRadius: BorderRadius.circular(6),
    borderSide: BorderSide.none,
  ),
);

class SettingsScreenDesktop extends StatelessWidget {
  const SettingsScreenDesktop({
    super.key,
    required this.musicAssistantUrl,
    required this.cacheSizeController,
    required this.isDarkTheme,
    required this.logLevel,
    required this.onCacheSizeChanged,
    required this.onThemeChanged,
    required this.onLogLevelChanged,
  });

  final String musicAssistantUrl;
  final TextEditingController cacheSizeController;
  final bool isDarkTheme;
  final LogLevelPreference logLevel;
  final ValueChanged<String> onCacheSizeChanged;
  final ValueChanged<bool> onThemeChanged;
  final ValueChanged<LogLevelPreference> onLogLevelChanged;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return ColoredBox(
      color: colors.bgPrimary,
      child: DefaultTabController(
        length: 2,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(32, 32, 32, 0),
          child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Text(
                  'Einstellungen',
                  style: Theme.of(context).textTheme.headlineMedium?.copyWith(
                    fontWeight: FontWeight.bold,
                  ),
                ),
                const SizedBox(height: 20),
                TabBar(
                  isScrollable: true,
                  tabAlignment: TabAlignment.start,
                  labelPadding: const EdgeInsets.symmetric(horizontal: 24),
                  indicatorSize: TabBarIndicatorSize.tab,
                  indicatorColor: colors.accent,
                  dividerColor: colors.border,
                  labelColor: colors.textPrimary,
                  unselectedLabelColor: colors.textPrimary,
                  labelStyle: const TextStyle(fontWeight: FontWeight.bold),
                  unselectedLabelStyle: const TextStyle(
                    fontWeight: FontWeight.bold,
                  ),
                  tabs: <Widget>[
                    const Tab(text: 'Allgemein'),
                    const Tab(text: 'Log'),
                  ],
                ),
                const SizedBox(height: 12),
                Expanded(
                  child: TabBarView(
                    children: <Widget>[
                      _DesktopSettingsForm(
                        musicAssistantUrl: musicAssistantUrl,
                        cacheSizeController: cacheSizeController,
                        isDarkTheme: isDarkTheme,
                        onCacheSizeChanged: onCacheSizeChanged,
                        onThemeChanged: onThemeChanged,
                      ),
                      _DesktopLogTab(
                        logLevel: logLevel,
                        onLogLevelChanged: onLogLevelChanged,
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

class _DesktopLogTab extends StatelessWidget {
  const _DesktopLogTab({
    required this.logLevel,
    required this.onLogLevelChanged,
  });

  final LogLevelPreference logLevel;
  final ValueChanged<LogLevelPreference> onLogLevelChanged;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        const SizedBox(height: 12),
        Row(
          children: <Widget>[
            Text('Log-Level', style: Theme.of(context).textTheme.titleMedium),
            const Spacer(),
            SizedBox(
              width: _desktopSettingsControlWidth,
              child: SettingsLogLevelField(
                logLevel: logLevel,
                onChanged: onLogLevelChanged,
                shouldShowLabel: false,
                shouldUseCompactFieldLayout: true,
              ),
            ),
          ],
        ),
        const SizedBox(height: 12),
        const Expanded(
          child: Padding(
            padding: EdgeInsets.only(bottom: 8),
            child: SettingsLogOutput(),
          ),
        ),
      ],
    );
  }
}

class _DesktopSettingsForm extends StatelessWidget {
  const _DesktopSettingsForm({
    required this.musicAssistantUrl,
    required this.cacheSizeController,
    required this.isDarkTheme,
    required this.onCacheSizeChanged,
    required this.onThemeChanged,
  });

  final String musicAssistantUrl;
  final TextEditingController cacheSizeController;
  final bool isDarkTheme;
  final ValueChanged<String> onCacheSizeChanged;
  final ValueChanged<bool> onThemeChanged;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return ListView(
      padding: const EdgeInsets.only(top: 12),
      children: <Widget>[
        _DesktopSettingsSection(
          title: 'Verbindung',
          child: _DesktopSettingRow(
            title: 'Music Assistant URL',
            description: 'Die URL wird mit der Anmeldung gespeichert.',
            control: SizedBox(
              width: _desktopSettingsControlWidth,
              child: TextFormField(
                initialValue: musicAssistantUrl,
                readOnly: true,
                decoration: _desktopSettingsFieldDecoration(
                  colors: colors,
                  icon: Icons.dns_outlined,
                ),
              ),
            ),
          ),
        ),
        const _DesktopSettingsDivider(),
        _DesktopSettingsSection(
          title: 'Wiedergabe',
          child: _DesktopSettingRow(
            title: 'Lokaler Cache',
            description:
                'Zwischen 25 und 1000 MB. Die Einstellung wird in der aktiven Sendspin-Konfiguration gespeichert.',
            control: SizedBox(
              width: _desktopSettingsControlWidth,
              child: TextField(
                controller: cacheSizeController,
                keyboardType: TextInputType.number,
                onChanged: onCacheSizeChanged,
                decoration: _desktopSettingsFieldDecoration(
                  colors: colors,
                  icon: Icons.storage_outlined,
                  suffixText: 'MB',
                ),
              ),
            ),
          ),
        ),
        const _DesktopSettingsDivider(),
        _DesktopSettingsSection(
          title: 'Darstellung',
          child: _DesktopSettingRow(
            title: 'Theme',
            description: 'Wähle die helle oder dunkle Darstellung der App.',
            control: SizedBox(
              width: _desktopSettingsControlWidth,
              child: SegmentedButton<bool>(
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
            ),
          ),
        ),
      ],
    );
  }
}

class _DesktopSettingsSection extends StatelessWidget {
  const _DesktopSettingsSection({required this.title, required this.child});

  final String title;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text(title, style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 20),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 24),
          child: child,
        ),
      ],
    );
  }
}

class _DesktopSettingsDivider extends StatelessWidget {
  const _DesktopSettingsDivider();

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 28),
    child: Divider(color: context.mashinColors.border),
  );
}

class _DesktopSettingRow extends StatelessWidget {
  const _DesktopSettingRow({
    required this.title,
    required this.description,
    required this.control,
  });

  final String title;
  final String description;
  final Widget control;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return LayoutBuilder(
      builder: (BuildContext context, BoxConstraints constraints) {
        return Row(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: <Widget>[
            ConstrainedBox(
              constraints: BoxConstraints(maxWidth: constraints.maxWidth * .5),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  Text(title, style: Theme.of(context).textTheme.titleMedium),
                  const SizedBox(height: 6),
                  Text(
                    description,
                    style: TextStyle(color: colors.textSecondary),
                  ),
                ],
              ),
            ),
            const Spacer(),
            control,
          ],
        );
      },
    );
  }
}
