import 'package:flutter/material.dart';
import 'package:material_symbols_icons/symbols.dart';

import '../../core/core_models.dart';
import 'media_list_section_desktop.dart' show accentColorFromText;

const _moodStyles = <String, ({IconData icon, double hue})>{
  'relax': (icon: Symbols.self_improvement, hue: 155),
  'focus': (icon: Symbols.mindfulness, hue: 200),
  'energy': (icon: Symbols.power, hue: 35),
  'melancholy': (icon: Symbols.sentiment_sad, hue: 225),
  'love': (icon: Symbols.favorite, hue: 345),
  'party': (icon: Symbols.celebration, hue: 285),
  'happy': (icon: Symbols.sentiment_excited, hue: 50),
  'nostalgic': (icon: Symbols.local_cafe, hue: 25),
  'angry': (icon: Symbols.sentiment_extremely_dissatisfied, hue: 0),
  'epic': (icon: Symbols.mode_heat, hue: 12),
};

// Stable provider IDs keep artwork independent of translated playlist names.
MoodPlaylistCover? buildMoodPlaylistCover(Playlist playlist) {
  const prefix = 'global_mood_playlist_';
  if (!playlist.itemId.startsWith(prefix)) return null;
  final moodKey = playlist.itemId.substring(prefix.length);
  final style = _moodStyles[moodKey];
  if (style == null) return null;
  final accent = HSLColor.fromColor(accentColorFromText(moodKey))
      .withHue(style.hue)
      .toColor();
  return MoodPlaylistCover(icon: style.icon, accentColor: accent);
}

class MoodPlaylistCover extends StatelessWidget {
  const MoodPlaylistCover({
    super.key,
    required this.icon,
    required this.accentColor,
  });

  final IconData icon;
  final Color accentColor;

  @override
  Widget build(BuildContext context) {
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final accent = HSLColor.fromColor(accentColor);
    final foreground = accent.withLightness(isDark ? 0.68 : 0.34).toColor();
    final background = accent
        .withSaturation(accent.saturation * 0.55)
        .withLightness(isDark ? 0.15 : 0.92)
        .toColor();

    return ColoredBox(
      color: background,
      child: LayoutBuilder(
        builder: (context, constraints) => Center(
          child: Icon(
            icon,
            color: foreground,
            size: constraints.biggest.shortestSide * 0.44,
            fill: 1,
            weight: 400,
            opticalSize: 48,
          ),
        ),
      ),
    );
  }
}