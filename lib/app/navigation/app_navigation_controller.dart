import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../../features/artist/artist_detail_page.dart';
import '../../features/home/home_screen.dart';
import '../../features/playback/playback_coordinator.dart';
import '../../features/search/search_screen.dart';

enum AppDetailType { artist, album, playlist }

enum AppSection {
  home,
  explore,
  favorites,
  playlists,
  search,
}

class AppDetailDestination {
  final AppDetailType type;
  final String itemId;
  final String providerInstanceIdOrDomain;

  const AppDetailDestination({
    required this.type,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
  });
}

/// Single source of truth for in-app navigation state.
class AppNavigationController extends ChangeNotifier {
  AppSection _currentSection = AppSection.home;
  String _desktopSearchQuery = '';
  final List<AppDetailDestination> _detailStack = <AppDetailDestination>[];

  AppSection get currentSection => _currentSection;
  String get desktopSearchQuery => _desktopSearchQuery;
  AppDetailDestination? get activeDetail => _detailStack.isEmpty ? null : _detailStack.last;

  void selectSection(AppSection section) {
    final bool isSameSection = _currentSection == section;
    final bool hasDetail = activeDetail != null;
    if (isSameSection && !hasDetail) {
      return;
    }

    _currentSection = section;
    _detailStack.clear();
    notifyListeners();
  }

  void openArtist(Artist artist) {
    openDetail(
      type: AppDetailType.artist,
      itemId: artist.itemId,
      providerInstanceIdOrDomain: artist.provider,
    );
  }

  void openDetail({
    required AppDetailType type,
    required String itemId,
    required String providerInstanceIdOrDomain,
  }) {
    final String normalizedItemId = itemId.trim();
    final String normalizedProvider = providerInstanceIdOrDomain.trim();
    if (normalizedItemId.isEmpty || normalizedProvider.isEmpty) {
      return;
    }

    final AppDetailDestination? current = activeDetail;
    if (current != null &&
        current.type == type &&
        current.itemId == normalizedItemId &&
        current.providerInstanceIdOrDomain == normalizedProvider) {
      return;
    }

    _detailStack.add(
      AppDetailDestination(
        type: type,
        itemId: normalizedItemId,
        providerInstanceIdOrDomain: normalizedProvider,
      ),
    );
    notifyListeners();
  }

  bool popDetail() {
    if (_detailStack.isEmpty) {
      return false;
    }
    _detailStack.removeLast();
    notifyListeners();
    return true;
  }

  void clearDetail() {
    if (_detailStack.isEmpty) {
      return;
    }
    _detailStack.clear();
    notifyListeners();
  }

  void onDesktopSearchChanged(String query) {
    final String trimmed = query.trim();
    _desktopSearchQuery = trimmed;
    _detailStack.clear();
    if (trimmed.isNotEmpty && _currentSection != AppSection.search) {
      _currentSection = AppSection.search;
    }
    notifyListeners();
  }

  void ensureSearchSectionForInput(String query) {
    final String trimmed = query.trim();
    if (trimmed.isEmpty || _currentSection == AppSection.search) {
      return;
    }

    _currentSection = AppSection.search;
    _detailStack.clear();
    notifyListeners();
  }

  void submitDesktopSearch(String query) {
    final String trimmed = query.trim();
    _desktopSearchQuery = trimmed;
    _detailStack.clear();
    if (trimmed.isNotEmpty) {
      _currentSection = AppSection.search;
    }
    notifyListeners();
  }

  void clearDesktopSearch() {
    _desktopSearchQuery = '';
    _detailStack.clear();
    notifyListeners();
  }

  Widget buildPage(
    MusicAssistantClient client,
    PlaybackCoordinator playbackCoordinator, {
    bool desktopSearchDriven = false,
  }) {
    final AppDetailDestination? detail = activeDetail;

    if (detail != null && detail.type == AppDetailType.artist) {
      return ArtistDetailPage(
        client: client,
        playbackCoordinator: playbackCoordinator,
        itemId: detail.itemId,
        providerInstanceIdOrDomain: detail.providerInstanceIdOrDomain,
        onOpenArtist: openArtist,
      );
    }

    switch (currentSection) {
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
          desktopSearchDriven: desktopSearchDriven,
          desktopSearchQuery: desktopSearchQuery,
          onOpenArtist: openArtist,
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