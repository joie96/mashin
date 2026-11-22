import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/app/theme/platform_color_theme_builder.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/settings/accent_color_settings.dart';

void main() {
  test(
    'Normal text is neutral while explicit accents retain their palette colors',
    () {
      addTearDown(() => debugDefaultTargetPlatformOverride = null);
      for (final seed in [
        Colors.yellow,
        Colors.blue,
        Colors.green,
        Colors.red,
      ]) {
        for (final brightness in Brightness.values) {
          final original = ColorScheme.fromSeed(
            seedColor: seed,
            brightness: brightness,
          );
          final neutralTextScheme = original.copyWith(
            onSurface: HSLColor.fromColor(
              original.onSurface,
            ).withSaturation(0).toColor(),
            onSurfaceVariant: HSLColor.fromColor(
              original.onSurfaceVariant,
            ).withSaturation(0).toColor(),
            onInverseSurface: HSLColor.fromColor(
              original.onInverseSurface,
            ).withSaturation(0).toColor(),
          );
          for (final platform in [
            TargetPlatform.windows,
            TargetPlatform.android,
          ]) {
            debugDefaultTargetPlatformOverride = platform;
            final theme = brightness == Brightness.dark
                ? AppTheme.dark(colorScheme: original)
                : AppTheme.light(colorScheme: original);
            final scheme = theme.colorScheme;
            final colors = theme.extension<MashinColors>()!;
            final seededTheme = brightness == Brightness.dark
                ? AppTheme.dark(seedColor: seed)
                : AppTheme.light(seedColor: seed);
            expect(seededTheme.colorScheme, scheme);
            expect(scheme.brightness, brightness);
            expect(colors.accent, scheme.primary);
            expect(colors.selection, scheme.primaryContainer);
            expect(colors.bgPrimary, scheme.surface);
            expect(colors.bgSecondary, scheme.surfaceContainer);
            expect(colors.bgTertiary, scheme.surfaceContainerHigh);
            expect(colors.textPrimary, scheme.onSurface);
            expect(colors.textSecondary, scheme.onSurfaceVariant);
            expect(colors.textTertiary, scheme.onSurfaceVariant);
            expect(colors.accent, original.primary);
            expect(colors.selection, original.primaryContainer);
            expect(colors.textPrimary, neutralTextScheme.onSurface);
            expect(colors.textSecondary, neutralTextScheme.onSurfaceVariant);
            for (final pair in [
              (scheme.onSurface, original.onSurface),
              (scheme.onSurfaceVariant, original.onSurfaceVariant),
              (scheme.onInverseSurface, original.onInverseSurface),
            ]) {
              expect(pair.$1.r, pair.$1.g);
              expect(pair.$1.g, pair.$1.b);
              expect(pair.$1.a, pair.$2.a);
              expect(
                HSLColor.fromColor(pair.$1).lightness,
                closeTo(HSLColor.fromColor(pair.$2).lightness, 0.005),
              );
            }
            expect(theme.textTheme.bodyMedium!.color, scheme.onSurface);
            expect(theme.sliderTheme.activeTrackColor, scheme.primary);
            expect(theme.sliderTheme.thumbColor, original.primary);
            if (platform == TargetPlatform.android) {
              expect(scheme, neutralTextScheme);
            } else {
              expect(scheme, isNot(original));
              expect(
                scheme.copyWith(
                  surface: original.surface,
                  surfaceDim: original.surfaceDim,
                  surfaceBright: original.surfaceBright,
                  surfaceContainerLowest: original.surfaceContainerLowest,
                  surfaceContainerLow: original.surfaceContainerLow,
                  surfaceContainer: original.surfaceContainer,
                  surfaceContainerHigh: original.surfaceContainerHigh,
                  surfaceContainerHighest: original.surfaceContainerHighest,
                  outline: original.outline,
                  outlineVariant: original.outlineVariant,
                  inverseSurface: original.inverseSurface,
                ),
                neutralTextScheme,
              );
              for (final pair in [
                (scheme.surface, original.surface),
                (scheme.surfaceDim, original.surfaceDim),
                (scheme.surfaceBright, original.surfaceBright),
                (
                  scheme.surfaceContainerLowest,
                  original.surfaceContainerLowest,
                ),
                (scheme.surfaceContainerLow, original.surfaceContainerLow),
                (scheme.surfaceContainer, original.surfaceContainer),
                (scheme.surfaceContainerHigh, original.surfaceContainerHigh),
                (
                  scheme.surfaceContainerHighest,
                  original.surfaceContainerHighest,
                ),
                (scheme.outline, original.outline),
                (scheme.outlineVariant, original.outlineVariant),
                (scheme.inverseSurface, original.inverseSurface),
              ]) {
                final actual = HSLColor.fromColor(pair.$1);
                final previous = HSLColor.fromColor(pair.$2);
                final actualChannels = [pair.$1.r, pair.$1.g, pair.$1.b]
                  ..sort();
                final previousChannels = [pair.$2.r, pair.$2.g, pair.$2.b]
                  ..sort();
                expect(
                  actualChannels.last - actualChannels.first,
                  closeTo(
                    (previousChannels.last - previousChannels.first) * 0.35,
                    1 / 255 + 0.000001,
                  ),
                );
                expect(actual.lightness, closeTo(previous.lightness, 0.005));
                expect(pair.$1.a, pair.$2.a);
              }
            }
            expect(
              _contrast(colors.bgPrimary, colors.textPrimary),
              greaterThanOrEqualTo(4.5),
            );
            expect(
              _contrast(colors.bgSecondary, colors.textSecondary),
              greaterThanOrEqualTo(4.5),
            );
            expect(
              _contrast(colors.bgTertiary, colors.textPrimary),
              greaterThanOrEqualTo(4.5),
            );
          }
        }
      }
    },
  );

  test('Windows navigation retains the unmodified readable palette accent', () {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    for (final seed in [Colors.yellow, Colors.blue, Colors.green, Colors.red]) {
      for (final brightness in Brightness.values) {
        final theme = brightness == Brightness.dark
            ? AppTheme.dark(seedColor: seed)
            : AppTheme.light(seedColor: seed);
        final colors = theme.extension<MashinColors>()!;
        final foreground = colors.accent;
        expect(
          foreground,
          ColorScheme.fromSeed(seedColor: seed, brightness: brightness).primary,
        );
        for (final background in [
          colors.bgPrimary,
          colors.bgSecondary,
          colors.hover,
        ]) {
          expect(_contrast(background, foreground), greaterThanOrEqualTo(4.5));
        }
      }
    }
  });

  testWidgets(
    'manual color controls fit mobile and retain the selected color',
    (tester) async {
      tester.view.resetPhysicalSize();
      tester.view.physicalSize = const Size(320, 700);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      var preference = const AccentColorPreference();
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.light(),
          home: Scaffold(
            body: StatefulBuilder(
              builder: (context, setState) => Padding(
                padding: const EdgeInsets.all(20),
                child: AccentColorSettings(
                  preference: preference,
                  onChanged: (value) => setState(() => preference = value),
                ),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Manuell'));
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip('Petrol'));
      await tester.pumpAndSettle();
      expect(preference.manualColor, 0xFF00796B);
      await tester.enterText(
        find.byKey(const Key('settings-accent-hex')),
        '#123ABC',
      );
      await tester.tap(find.byTooltip('Farbe anwenden'));
      await tester.pumpAndSettle();
      expect(preference.manualColor, 0xFF123ABC);
      await tester.tap(find.text('Auto'));
      await tester.pumpAndSettle();
      expect(preference.mode, AccentColorMode.auto);
      expect(preference.manualColor, 0xFF123ABC);
      expect(find.byKey(const Key('settings-accent-hex')), findsNothing);
      expect(tester.takeException(), isNull);
    },
  );

  test(
    'accent settings retain the manual color across mode and theme changes',
    () {
      const accent = AccentColorPreference(
        mode: AccentColorMode.manual,
        manualColor: 0xFF00796B,
      );
      const settings = SessionSettingsUpdate(
        themePreference: ThemePreference.light,
        logLevel: LogLevelPreference.info,
        accentColor: accent,
      );
      final auto = accent.copyWith(mode: AccentColorMode.auto);
      expect(auto.manualColor, accent.manualColor);
      expect(
        SessionSettings.fromJson(
          settings.copyWith(themePreference: ThemePreference.dark).toJson(),
        ).accentColor,
        accent,
      );
      expect(
        SessionSettings.fromJson({}).accentColor.mode,
        AccentColorMode.auto,
      );
      expect(
        () => AccentColorPreference.fromJson({'manualColor': 0x00796B}),
        throwsFormatException,
      );
    },
  );

  testWidgets(
    'platform palettes refresh on resume and fall back when unavailable',
    (tester) async {
      const channel = MethodChannel('io.material.plugins/dynamic_color');
      int? accent;
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(
            channel,
            (call) async => call.method == 'getAccentColor' ? accent : null,
          );
      addTearDown(
        () => TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
            .setMockMethodCallHandler(channel, null),
      );
      ColorScheme? observed;
      await tester.pumpWidget(
        PlatformColorThemeBuilder(
          builder: (light, dark) {
            observed = light;
            return const SizedBox.shrink();
          },
        ),
      );
      await tester.pumpAndSettle();
      expect(observed, isNull);
      accent = 0xFF00796B;
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
      await tester.pumpAndSettle();
      expect(
        observed!.primary,
        ColorScheme.fromSeed(seedColor: Color(accent)).primary,
      );
      expect(observed!.brightness, Brightness.light);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  test('tonal themes keep brightness and readable foregrounds', () {
    for (final seed in [Colors.yellow, Colors.blue, Colors.green, Colors.red]) {
      for (final brightness in Brightness.values) {
        final scheme = ColorScheme.fromSeed(
          seedColor: seed,
          brightness: brightness,
        );
        final theme = brightness == Brightness.dark
            ? AppTheme.dark(colorScheme: scheme)
            : AppTheme.light(colorScheme: scheme);
        final colors = theme.extension<MashinColors>()!;
        expect(theme.brightness, brightness);
        expect(colors.bgPrimary, theme.colorScheme.surface);
        expect(colors.selection, theme.colorScheme.primaryContainer);
        expect(
          _contrast(colors.accent, colors.onAccent),
          greaterThanOrEqualTo(4.5),
        );
        expect(
          _contrast(colors.selection, colors.onSelection),
          greaterThanOrEqualTo(4.5),
        );
        expect(
          _contrast(colors.bgPrimary, colors.textPrimary),
          greaterThanOrEqualTo(4.5),
        );
        expect(
          _contrast(colors.playbackProgress, colors.textPrimary),
          greaterThanOrEqualTo(4.5),
        );
        expect(
          theme.extension<MashinTableTextStyles>()!.bodyPrimary.color,
          colors.textPrimary,
        );
      }
    }
  });

  testWidgets(
    'Android platform palette roles are retained rather than reseeded',
    (tester) async {
      const channel = MethodChannel('io.material.plugins/dynamic_color');
      const platformColor = 0xFF345678;
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(
            channel,
            (call) async => call.method == 'getCorePalette'
                ? Int32List.fromList(List<int>.filled(65, platformColor))
                : null,
          );
      addTearDown(
        () => TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
            .setMockMethodCallHandler(channel, null),
      );
      ColorScheme? observedLight;
      ColorScheme? observedDark;
      await tester.pumpWidget(
        PlatformColorThemeBuilder(
          builder: (light, dark) {
            observedLight = light;
            observedDark = dark;
            return const SizedBox.shrink();
          },
        ),
      );
      await tester.pumpAndSettle();
      expect(observedLight!.primary, const Color(platformColor));
      expect(observedLight!.onSurface, const Color(platformColor));
      expect(observedDark!.secondaryContainer, const Color(platformColor));
      expect(observedDark!.brightness, Brightness.dark);
      expect(
        observedLight!.onSurface,
        isNot(
          ColorScheme.fromSeed(seedColor: const Color(platformColor)).onSurface,
        ),
      );
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );
}

double _contrast(Color background, Color foreground) {
  final first = background.computeLuminance();
  final second = foreground.computeLuminance();
  return ((first > second ? first : second) + 0.05) /
      ((first > second ? second : first) + 0.05);
}
