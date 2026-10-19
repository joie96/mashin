import 'package:flutter/material.dart';

class AppTheme {
  const AppTheme._();

  static const Duration tooltipWaitDuration = Duration(milliseconds: 600);

  static const Color _darkBgPrimary = Color(0xFF0C0C0C);
  static const Color _darkBgSecondary = Color(0xFF141414);
  static const Color _darkBgTertiary = Color(0xFF1F1F21);
  static const Color _darkTextPrimary = Color(0xFFE1E1E1);
  static const Color _darkTextSecondary = Color(0xFFCFCFCF);
  static const Color _darkTextTertiary = Color(0xFF969696);
  static const Color _darkAccent = Color(0xFFFF5A6F);
  static const Color _darkBorder = Color(0xFF242424);
  static const Color _darkHover = Color(0xFF242424);
  static const Color _darkSelection = Color(0xFF303030);
  static const double _darkBlurredImageOverlayOpacity = 0.46;
  static const double _darkCoverHoverOverlayOpacity = 0.70;

  static const Color _lightBgPrimary = Color(0xFFEBEBEB);
  static const Color _lightBgSecondary = Color(0xFFF0F0F0);
  static const Color _lightBgTertiary = Color(0xFFE1E1E1);
  static const Color _lightTextPrimary = Color(0xFF191919);
  static const Color _lightTextSecondary = Color(0xFF505050);
  static const Color _lightTextTertiary = Color(0xFF6F6F6F);
  static const Color _lightAccent = Color(0xFFE94757);
  static const Color _lightBorder = Color(0xFFD0D0D0);
  static const Color _lightHover = Color(0xFFE1E1E1);
  static const Color _lightSelection = Color(0xFFD8D8D8);
  static const double _lightBlurredImageOverlayOpacity = 0.68;
  static const double _lightCoverHoverOverlayOpacity = 0.50;

  static const Color _overlayPlayButtonBackground = Color(0xFFFFFFFF);
  static const Color _overlayPlayButtonForeground = Color(0xFF000000);
  static const Color _overlayIcon = Color(0xFFFFFFFF);

  static ThemeData dark() {
    final base = ThemeData.dark(useMaterial3: true);
    return base.copyWith(
      tooltipTheme: const TooltipThemeData(waitDuration: tooltipWaitDuration),
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
        MashinTableTextStyles.dark(),
      ],
    );
  }

  static ThemeData light() {
    final base = ThemeData.light(useMaterial3: true);
    return base.copyWith(
      tooltipTheme: const TooltipThemeData(waitDuration: tooltipWaitDuration),
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
        MashinTableTextStyles.light(),
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
  final Color selection;
  final double blurredImageOverlayOpacity;
  final double coverHoverOverlayOpacity;
  final Color overlayPlayButtonBackground;
  final Color overlayPlayButtonForeground;
  final Color overlayIcon;

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
    required this.selection,
    required this.blurredImageOverlayOpacity,
    required this.coverHoverOverlayOpacity,
    required this.overlayPlayButtonBackground,
    required this.overlayPlayButtonForeground,
    required this.overlayIcon,
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
      hover = AppTheme._darkHover,
      selection = AppTheme._darkSelection,
      blurredImageOverlayOpacity = AppTheme._darkBlurredImageOverlayOpacity,
      coverHoverOverlayOpacity = AppTheme._darkCoverHoverOverlayOpacity,
      overlayPlayButtonBackground = AppTheme._overlayPlayButtonBackground,
      overlayPlayButtonForeground = AppTheme._overlayPlayButtonForeground,
      overlayIcon = AppTheme._overlayIcon;

  const MashinColors.light()
    : bgPrimary = AppTheme._lightBgPrimary,
      bgSecondary = AppTheme._lightBgSecondary,
      bgTertiary = AppTheme._lightBgTertiary,
      textPrimary = AppTheme._lightTextPrimary,
      textSecondary = AppTheme._lightTextSecondary,
      textTertiary = AppTheme._lightTextTertiary,
      accent = AppTheme._lightAccent,
      border = AppTheme._lightBorder,
      hover = AppTheme._lightHover,
      selection = AppTheme._lightSelection,
      blurredImageOverlayOpacity = AppTheme._lightBlurredImageOverlayOpacity,
      coverHoverOverlayOpacity = AppTheme._lightCoverHoverOverlayOpacity,
      overlayPlayButtonBackground = AppTheme._overlayPlayButtonBackground,
      overlayPlayButtonForeground = AppTheme._overlayPlayButtonForeground,
      overlayIcon = AppTheme._overlayIcon;

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
    Color? selection,
    double? blurredImageOverlayOpacity,
    double? coverHoverOverlayOpacity,
    Color? overlayPlayButtonBackground,
    Color? overlayPlayButtonForeground,
    Color? overlayIcon,
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
      selection: selection ?? this.selection,
      blurredImageOverlayOpacity:
          blurredImageOverlayOpacity ?? this.blurredImageOverlayOpacity,
      coverHoverOverlayOpacity:
          coverHoverOverlayOpacity ?? this.coverHoverOverlayOpacity,
      overlayPlayButtonBackground:
          overlayPlayButtonBackground ?? this.overlayPlayButtonBackground,
      overlayPlayButtonForeground:
          overlayPlayButtonForeground ?? this.overlayPlayButtonForeground,
      overlayIcon: overlayIcon ?? this.overlayIcon,
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
      textSecondary:
          Color.lerp(textSecondary, other.textSecondary, t) ?? textSecondary,
      textTertiary:
          Color.lerp(textTertiary, other.textTertiary, t) ?? textTertiary,
      accent: Color.lerp(accent, other.accent, t) ?? accent,
      border: Color.lerp(border, other.border, t) ?? border,
      hover: Color.lerp(hover, other.hover, t) ?? hover,
      selection: Color.lerp(selection, other.selection, t) ?? selection,
      blurredImageOverlayOpacity:
          blurredImageOverlayOpacity +
          ((other.blurredImageOverlayOpacity - blurredImageOverlayOpacity) * t),
      coverHoverOverlayOpacity:
          coverHoverOverlayOpacity +
          ((other.coverHoverOverlayOpacity - coverHoverOverlayOpacity) * t),
      overlayPlayButtonBackground:
          Color.lerp(
            overlayPlayButtonBackground,
            other.overlayPlayButtonBackground,
            t,
          ) ??
          overlayPlayButtonBackground,
      overlayPlayButtonForeground:
          Color.lerp(
            overlayPlayButtonForeground,
            other.overlayPlayButtonForeground,
            t,
          ) ??
          overlayPlayButtonForeground,
      overlayIcon: Color.lerp(overlayIcon, other.overlayIcon, t) ?? overlayIcon,
    );
  }
}

@immutable
class MashinTableTextStyles extends ThemeExtension<MashinTableTextStyles> {
  final TextStyle header;
  final TextStyle bodyPrimary;
  final TextStyle bodySecondary;
  final TextStyle bodyTertiary;
  final TextStyle mobileTitle;
  final TextStyle mobileSubtitle;
  final TextStyle mobileTrailing;
  final TextStyle emptyState;
  final TextStyle providerFallback;

  const MashinTableTextStyles({
    required this.header,
    required this.bodyPrimary,
    required this.bodySecondary,
    required this.bodyTertiary,
    required this.mobileTitle,
    required this.mobileSubtitle,
    required this.mobileTrailing,
    required this.emptyState,
    required this.providerFallback,
  });

  const MashinTableTextStyles.dark()
    : header = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._darkTextSecondary,
        fontWeight: FontWeight.w700,
        fontSize: 12,
      ),
      bodyPrimary = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._darkTextPrimary,
        fontSize: 13,
      ),
      bodySecondary = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._darkTextSecondary,
        fontSize: 13,
      ),
      bodyTertiary = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._darkTextTertiary,
        fontSize: 13,
      ),
      mobileTitle = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._darkTextPrimary,
        fontSize: 14,
      ),
      mobileSubtitle = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._darkTextSecondary,
        fontSize: 12,
      ),
      mobileTrailing = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._darkTextSecondary,
        fontSize: 12,
      ),
      emptyState = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._darkTextSecondary,
        fontSize: 14,
      ),
      providerFallback = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._darkTextSecondary,
        fontWeight: FontWeight.w700,
        fontSize: 12,
      );

  const MashinTableTextStyles.light()
    : header = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._lightTextSecondary,
        fontWeight: FontWeight.w700,
        fontSize: 12,
      ),
      bodyPrimary = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._lightTextPrimary,
        fontSize: 13,
      ),
      bodySecondary = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._lightTextSecondary,
        fontSize: 13,
      ),
      bodyTertiary = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._lightTextTertiary,
        fontSize: 13,
      ),
      mobileTitle = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._lightTextPrimary,
        fontSize: 14,
      ),
      mobileSubtitle = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._lightTextSecondary,
        fontSize: 12,
      ),
      mobileTrailing = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._lightTextSecondary,
        fontSize: 12,
      ),
      emptyState = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._lightTextSecondary,
        fontSize: 14,
      ),
      providerFallback = const TextStyle(
        fontFamily: 'PoppinsRegular',
        color: AppTheme._lightTextSecondary,
        fontWeight: FontWeight.w700,
        fontSize: 12,
      );

  @override
  MashinTableTextStyles copyWith({
    TextStyle? header,
    TextStyle? bodyPrimary,
    TextStyle? bodySecondary,
    TextStyle? bodyTertiary,
    TextStyle? mobileTitle,
    TextStyle? mobileSubtitle,
    TextStyle? mobileTrailing,
    TextStyle? emptyState,
    TextStyle? providerFallback,
  }) {
    return MashinTableTextStyles(
      header: header ?? this.header,
      bodyPrimary: bodyPrimary ?? this.bodyPrimary,
      bodySecondary: bodySecondary ?? this.bodySecondary,
      bodyTertiary: bodyTertiary ?? this.bodyTertiary,
      mobileTitle: mobileTitle ?? this.mobileTitle,
      mobileSubtitle: mobileSubtitle ?? this.mobileSubtitle,
      mobileTrailing: mobileTrailing ?? this.mobileTrailing,
      emptyState: emptyState ?? this.emptyState,
      providerFallback: providerFallback ?? this.providerFallback,
    );
  }

  @override
  MashinTableTextStyles lerp(
    ThemeExtension<MashinTableTextStyles>? other,
    double t,
  ) {
    if (other is! MashinTableTextStyles) {
      return this;
    }

    return MashinTableTextStyles(
      header: TextStyle.lerp(header, other.header, t) ?? header,
      bodyPrimary:
          TextStyle.lerp(bodyPrimary, other.bodyPrimary, t) ?? bodyPrimary,
      bodySecondary:
          TextStyle.lerp(bodySecondary, other.bodySecondary, t) ??
          bodySecondary,
      bodyTertiary:
          TextStyle.lerp(bodyTertiary, other.bodyTertiary, t) ?? bodyTertiary,
      mobileTitle:
          TextStyle.lerp(mobileTitle, other.mobileTitle, t) ?? mobileTitle,
      mobileSubtitle:
          TextStyle.lerp(mobileSubtitle, other.mobileSubtitle, t) ??
          mobileSubtitle,
      mobileTrailing:
          TextStyle.lerp(mobileTrailing, other.mobileTrailing, t) ??
          mobileTrailing,
      emptyState: TextStyle.lerp(emptyState, other.emptyState, t) ?? emptyState,
      providerFallback:
          TextStyle.lerp(providerFallback, other.providerFallback, t) ??
          providerFallback,
    );
  }
}

extension MashinColorsX on BuildContext {
  MashinColors get mashinColors {
    final extension = Theme.of(this).extension<MashinColors>();
    assert(extension != null, 'MashinColors extension is not configured.');
    return extension!;
  }

  MashinTableTextStyles get mashinTableTextStyles {
    final extension = Theme.of(this).extension<MashinTableTextStyles>();
    assert(
      extension != null,
      'MashinTableTextStyles extension is not configured.',
    );
    return extension!;
  }
}
