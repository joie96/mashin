import 'package:flutter/material.dart';

import '../../data/music_assistant/ma_client.dart';
import '../../features/playback/playback_coordinator.dart';
import '../../features/home/home_screen.dart';
import '../../features/search/search_screen.dart';
import 'app_section.dart';

class AppRouter {
  const AppRouter._();

  static Widget pageFor(
    AppSection section,
    MusicAssistantClient client,
    PlaybackCoordinator playbackCoordinator,
  ) {
    switch (section) {
      case AppSection.home:
        return HomeScreen(
          client: client,
        );
      case AppSection.explore:
        return const _SectionPlaceholder(
          title: 'Entdecken',
          subtitle: 'Genres, neue Releases, kuratierte Inhalte',
          icon: Icons.explore_outlined,
        );
      case AppSection.favorites:
        return const _SectionPlaceholder(
          title: 'Favoriten',
          subtitle: 'Deine favorisierten Tracks, Alben und Artists',
          icon: Icons.favorite_outlined,
        );
      case AppSection.playlists:
        return const _SectionPlaceholder(
          title: 'Playlists',
          subtitle: 'Lokale und remote Playlists',
          icon: Icons.queue_music_outlined,
        );
      case AppSection.search:
        return SearchScreen(
          client: client,
          playbackCoordinator: playbackCoordinator,
        );
    }
  }
}

class _SectionPlaceholder extends StatelessWidget {
  final String title;
  final String subtitle;
  final IconData icon;

  const _SectionPlaceholder({
    required this.title,
    required this.subtitle,
    required this.icon,
  });

  @override
  Widget build(BuildContext context) {
    final colorScheme = Theme.of(context).colorScheme;
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            Icon(icon, size: 46, color: colorScheme.primary),
            const SizedBox(height: 14),
            Text(
              title,
              style: Theme.of(context).textTheme.headlineSmall,
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 6),
            Text(
              subtitle,
              style: Theme.of(context).textTheme.bodyMedium,
              textAlign: TextAlign.center,
            ),
          ],
        ),
      ),
    );
  }
}
