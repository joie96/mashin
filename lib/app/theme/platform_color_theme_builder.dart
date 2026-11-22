import 'dart:async';

import 'package:dynamic_color/dynamic_color.dart';
import 'package:flutter/material.dart';

/// Refreshes platform colors without replacing the app router or session.
class PlatformColorThemeBuilder extends StatefulWidget {
  const PlatformColorThemeBuilder({super.key, required this.builder});

  final Widget Function(ColorScheme? light, ColorScheme? dark) builder;

  @override
  State<PlatformColorThemeBuilder> createState() =>
      _PlatformColorThemeBuilderState();
}

class _PlatformColorThemeBuilderState extends State<PlatformColorThemeBuilder>
    with WidgetsBindingObserver {
  final _dynamicColorKey = GlobalKey<DynamicColorBuilderState>();

  // Platform palette lifecycle.
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      unawaited(_dynamicColorKey.currentState?.initPlatformState());
    }
  }

  // The package uses material_ui; map its roles into Flutter's ColorScheme.
  @override
  Widget build(BuildContext context) => DynamicColorBuilder(
    key: _dynamicColorKey,
    builder: (light, dark) {
      final schemes = [light, dark].map((scheme) {
        if (scheme == null) return null;
        return ColorScheme.fromSeed(
          seedColor: scheme.primary,
          brightness: scheme.brightness,
        ).copyWith(
          primary: scheme.primary,
          onPrimary: scheme.onPrimary,
          primaryContainer: scheme.primaryContainer,
          onPrimaryContainer: scheme.onPrimaryContainer,
          secondary: scheme.secondary,
          onSecondary: scheme.onSecondary,
          secondaryContainer: scheme.secondaryContainer,
          onSecondaryContainer: scheme.onSecondaryContainer,
          tertiary: scheme.tertiary,
          onTertiary: scheme.onTertiary,
          tertiaryContainer: scheme.tertiaryContainer,
          onTertiaryContainer: scheme.onTertiaryContainer,
          error: scheme.error,
          onError: scheme.onError,
          errorContainer: scheme.errorContainer,
          onErrorContainer: scheme.onErrorContainer,
          surface: scheme.surface,
          onSurface: scheme.onSurface,
          onSurfaceVariant: scheme.onSurfaceVariant,
          surfaceDim: scheme.surfaceDim,
          surfaceBright: scheme.surfaceBright,
          surfaceContainerLowest: scheme.surfaceContainerLowest,
          surfaceContainerLow: scheme.surfaceContainerLow,
          surfaceContainer: scheme.surfaceContainer,
          surfaceContainerHigh: scheme.surfaceContainerHigh,
          surfaceContainerHighest: scheme.surfaceContainerHighest,
          outline: scheme.outline,
          outlineVariant: scheme.outlineVariant,
          inverseSurface: scheme.inverseSurface,
          onInverseSurface: scheme.onInverseSurface,
          inversePrimary: scheme.inversePrimary,
          shadow: scheme.shadow,
          scrim: scheme.scrim,
          surfaceTint: scheme.surfaceTint,
        );
      }).toList();
      return widget.builder(schemes[0], schemes[1]);
    },
  );
}
