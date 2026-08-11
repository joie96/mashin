import 'package:flutter/material.dart';

class AppTheme {
  const AppTheme._();

  static const Color _darkBgPrimary = Color(0xFF17171A);
  static const Color _darkBgSecondary = Color(0xFF1F1F24);
  static const Color _darkBgTertiary = Color(0xFF2A2B31);
  static const Color _darkTextPrimary = Color(0xFFECECF1);
  static const Color _darkTextSecondary = Color(0xFFB2B4C2);
  static const Color _darkTextTertiary = Color(0xFF8B8D9B);
  static const Color _darkAccent = Color(0xFF5A7DFF);
  static const Color _darkBorder = Color(0xFF2E3038);

  static const Color _lightBgPrimary = Color(0xFFF5F6FA);
  static const Color _lightBgSecondary = Color(0xFFFFFFFF);
  static const Color _lightBgTertiary = Color(0xFFEDEFF5);
  static const Color _lightTextPrimary = Color(0xFF1F2430);
  static const Color _lightTextSecondary = Color(0xFF4E5567);
  static const Color _lightTextTertiary = Color(0xFF7E8597);
  static const Color _lightAccent = Color(0xFF4267F6);
  static const Color _lightBorder = Color(0xFFD8DCE7);

  static ThemeData dark() {
    final base = ThemeData.dark(useMaterial3: true);
    return base.copyWith(
      scaffoldBackgroundColor: _darkBgPrimary,
      colorScheme: const ColorScheme.dark(
        primary: _darkAccent,
        secondary: _darkAccent,
        surface: _darkBgSecondary,
        onPrimary: Colors.white,
        onSecondary: Colors.white,
        onSurface: _darkTextPrimary,
      ),
      appBarTheme: const AppBarTheme(
        elevation: 0,
        backgroundColor: _darkBgPrimary,
        foregroundColor: _darkTextPrimary,
      ),
      dividerColor: _darkBorder,
      cardTheme: const CardThemeData(
        color: _darkBgSecondary,
        surfaceTintColor: Colors.transparent,
      ),
      textTheme: base.textTheme.apply(
        bodyColor: _darkTextPrimary,
        displayColor: _darkTextPrimary,
      ),
      extensions: const <ThemeExtension<dynamic>>[
        MashinColors.dark(),
      ],
    );
  }

  static ThemeData light() {
    final base = ThemeData.light(useMaterial3: true);
    return base.copyWith(
      scaffoldBackgroundColor: _lightBgPrimary,
      colorScheme: const ColorScheme.light(
        primary: _lightAccent,
        secondary: _lightAccent,
        surface: _lightBgSecondary,
        onPrimary: Colors.white,
        onSecondary: Colors.white,
        onSurface: _lightTextPrimary,
      ),
      appBarTheme: const AppBarTheme(
        elevation: 0,
        backgroundColor: _lightBgPrimary,
        foregroundColor: _lightTextPrimary,
      ),
      dividerColor: _lightBorder,
      cardTheme: const CardThemeData(
        color: _lightBgSecondary,
        surfaceTintColor: Colors.transparent,
      ),
      textTheme: base.textTheme.apply(
        bodyColor: _lightTextPrimary,
        displayColor: _lightTextPrimary,
      ),
      extensions: const <ThemeExtension<dynamic>>[
        MashinColors.light(),
      ],
    );
  }
}

@immutable
class MashinColors extends ThemeExtension<MashinColors> {
  final Color bgPrimary;
  final Color bgSecondary;
  final Color bgTertiary;
  final Color textPrimary;
  final Color textSecondary;
  final Color textTertiary;
  final Color accent;
  final Color border;
  final Color hover;

  const MashinColors({
    required this.bgPrimary,
    required this.bgSecondary,
    required this.bgTertiary,
    required this.textPrimary,
    required this.textSecondary,
    required this.textTertiary,
    required this.accent,
    required this.border,
    required this.hover,
  });

  const MashinColors.dark()
      : bgPrimary = AppTheme._darkBgPrimary,
        bgSecondary = AppTheme._darkBgSecondary,
        bgTertiary = AppTheme._darkBgTertiary,
        textPrimary = AppTheme._darkTextPrimary,
        textSecondary = AppTheme._darkTextSecondary,
        textTertiary = AppTheme._darkTextTertiary,
        accent = AppTheme._darkAccent,
        border = AppTheme._darkBorder,
        hover = const Color(0xFF282A32);

  const MashinColors.light()
      : bgPrimary = AppTheme._lightBgPrimary,
        bgSecondary = AppTheme._lightBgSecondary,
        bgTertiary = AppTheme._lightBgTertiary,
        textPrimary = AppTheme._lightTextPrimary,
        textSecondary = AppTheme._lightTextSecondary,
        textTertiary = AppTheme._lightTextTertiary,
        accent = AppTheme._lightAccent,
        border = AppTheme._lightBorder,
        hover = const Color(0xFFE9ECF5);

  @override
  MashinColors copyWith({
    Color? bgPrimary,
    Color? bgSecondary,
    Color? bgTertiary,
    Color? textPrimary,
    Color? textSecondary,
    Color? textTertiary,
    Color? accent,
    Color? border,
    Color? hover,
  }) {
    return MashinColors(
      bgPrimary: bgPrimary ?? this.bgPrimary,
      bgSecondary: bgSecondary ?? this.bgSecondary,
      bgTertiary: bgTertiary ?? this.bgTertiary,
      textPrimary: textPrimary ?? this.textPrimary,
      textSecondary: textSecondary ?? this.textSecondary,
      textTertiary: textTertiary ?? this.textTertiary,
      accent: accent ?? this.accent,
      border: border ?? this.border,
      hover: hover ?? this.hover,
    );
  }

  @override
  MashinColors lerp(ThemeExtension<MashinColors>? other, double t) {
    if (other is! MashinColors) {
      return this;
    }

    return MashinColors(
      bgPrimary: Color.lerp(bgPrimary, other.bgPrimary, t) ?? bgPrimary,
      bgSecondary: Color.lerp(bgSecondary, other.bgSecondary, t) ?? bgSecondary,
      bgTertiary: Color.lerp(bgTertiary, other.bgTertiary, t) ?? bgTertiary,
      textPrimary: Color.lerp(textPrimary, other.textPrimary, t) ?? textPrimary,
      textSecondary: Color.lerp(textSecondary, other.textSecondary, t) ?? textSecondary,
      textTertiary: Color.lerp(textTertiary, other.textTertiary, t) ?? textTertiary,
      accent: Color.lerp(accent, other.accent, t) ?? accent,
      border: Color.lerp(border, other.border, t) ?? border,
      hover: Color.lerp(hover, other.hover, t) ?? hover,
    );
  }
}

extension MashinColorsX on BuildContext {
  MashinColors get mashinColors {
    final extension = Theme.of(this).extension<MashinColors>();
    assert(extension != null, 'MashinColors extension is not configured.');
    return extension!;
  }
}
