import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../core/core_models.dart';

class AccentColorSettings extends StatefulWidget {
  const AccentColorSettings({
    super.key,
    required this.preference,
    required this.onChanged,
  });

  final AccentColorPreference preference;
  final ValueChanged<AccentColorPreference> onChanged;

  @override
  State<AccentColorSettings> createState() => _AccentColorSettingsState();
}

class _AccentColorSettingsState extends State<AccentColorSettings> {
  static const _swatches = [
    (0xFFFF5A6F, 'Koralle'),
    (0xFFC62828, 'Rot'),
    (0xFFEF6C00, 'Orange'),
    (0xFFF9A825, 'Gelb'),
    (0xFF558B2F, 'Gruen'),
    (0xFF00796B, 'Petrol'),
    (0xFF00838F, 'Cyan'),
    (0xFF1565C0, 'Blau'),
    (0xFF3949AB, 'Indigo'),
    (0xFF7B1FA2, 'Violett'),
    (0xFFAD1457, 'Pink'),
    (0xFF546E7A, 'Graphit'),
  ];
  late final TextEditingController _hexController;
  String? _colorError;

  // Color editor lifecycle.
  @override
  void initState() {
    super.initState();
    _hexController = TextEditingController(
      text: _hex(widget.preference.manualColor),
    );
  }

  @override
  void didUpdateWidget(covariant AccentColorSettings oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.preference.manualColor != widget.preference.manualColor) {
      _hexController.text = _hex(widget.preference.manualColor);
      _colorError = null;
    }
  }

  @override
  void dispose() {
    _hexController.dispose();
    super.dispose();
  }

  // Manual colors remain stored when automatic mode is selected.
  void _selectColor(int color) {
    widget.onChanged(
      widget.preference.copyWith(
        mode: AccentColorMode.manual,
        manualColor: color,
      ),
    );
  }

  void _applyHexColor() {
    final text = _hexController.text.trim().replaceFirst(RegExp(r'^#'), '');
    final color = RegExp(r'^[0-9a-fA-F]{6}$').hasMatch(text)
        ? int.tryParse(text, radix: 16)
        : null;
    setState(() => _colorError = color == null ? 'Ungueltiger Farbwert' : null);
    if (color != null) _selectColor(0xFF000000 | color);
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Farbgestaltung', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 12),
        SegmentedButton<AccentColorMode>(
          showSelectedIcon: false,
          segments: const [
            ButtonSegment(
              value: AccentColorMode.auto,
              icon: Icon(Icons.auto_awesome_outlined),
              label: Text('Auto'),
            ),
            ButtonSegment(
              value: AccentColorMode.manual,
              icon: Icon(Icons.palette_outlined),
              label: Text('Manuell'),
            ),
          ],
          selected: {widget.preference.mode},
          onSelectionChanged: (selection) => widget.onChanged(
            widget.preference.copyWith(mode: selection.first),
          ),
        ),
        if (widget.preference.mode == AccentColorMode.manual) ...[
          const SizedBox(height: 16),
          Wrap(
            spacing: 6,
            runSpacing: 6,
            children: [
              for (final swatch in _swatches)
                Tooltip(
                  message: swatch.$2,
                  child: Semantics(
                    button: true,
                    selected: widget.preference.manualColor == swatch.$1,
                    label: swatch.$2,
                    child: InkResponse(
                      onTap: () => _selectColor(swatch.$1),
                      radius: 22,
                      child: SizedBox.square(
                        dimension: 40,
                        child: Center(
                          child: Container(
                            width: 30,
                            height: 30,
                            decoration: BoxDecoration(
                              shape: BoxShape.circle,
                              color: Color(swatch.$1),
                              border: Border.all(color: colors.border),
                            ),
                            child: widget.preference.manualColor == swatch.$1
                                ? Icon(
                                    Icons.check,
                                    size: 18,
                                    color:
                                        ThemeData.estimateBrightnessForColor(
                                              Color(swatch.$1),
                                            ) ==
                                            Brightness.dark
                                        ? Colors.white
                                        : Colors.black,
                                  )
                                : null,
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
            ],
          ),
          const SizedBox(height: 12),
          SizedBox(
            width: 280,
            child: TextField(
              key: const Key('settings-accent-hex'),
              controller: _hexController,
              maxLength: 7,
              autocorrect: false,
              enableSuggestions: false,
              onSubmitted: (_) => _applyHexColor(),
              decoration: InputDecoration(
                labelText: 'Farbe',
                counterText: '',
                errorText: _colorError,
                suffixIcon: IconButton(
                  tooltip: 'Farbe anwenden',
                  onPressed: _applyHexColor,
                  icon: const Icon(Icons.check),
                ),
              ),
            ),
          ),
        ],
      ],
    );
  }

  String _hex(int color) =>
      '#${(color & 0xFFFFFF).toRadixString(16).padLeft(6, '0').toUpperCase()}';
}
