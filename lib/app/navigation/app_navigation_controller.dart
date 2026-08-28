import 'package:flutter/material.dart';

import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../../features/album/album_detail_page.dart';
import '../../features/artist/artist_detail_page.dart';
import '../../features/favorites/favorites_screen.dart';
import '../../features/home/home_screen.dart';
import '../../features/playback/playback_coordinator.dart';
import '../../features/playlist/playlist_detail_page.dart';
import '../../features/search/search_screen.dart';
import '../../features/shared/userdata_controller.dart';

enum AppDetailType { artist, album, playlist }

enum AppSection { home, explore, favorites, playlists, search }

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

class _AppNavigationSnapshot {
  const _AppNavigationSnapshot({
    required this.section,
    required this.desktopSearchQuery,
    required this.detailStack,
  });

  final AppSection section;
  final String desktopSearchQuery;
  final List<AppDetailDestination> detailStack;
}

/// Single source of truth for in-app navigation state.
class AppNavigationController extends ChangeNotifier {
  AppSection _currentSection = AppSection.home;
  String _desktopSearchQuery = '';
  final List<AppDetailDestination> _detailStack = <AppDetailDestination>[];
  final List<_AppNavigationSnapshot> _history = <_AppNavigationSnapshot>[];

  AppSection get currentSection => _currentSection;
  String get desktopSearchQuery => _desktopSearchQuery;
  AppDetailDestination? get activeDetail =>
      _detailStack.isEmpty ? null : _detailStack.last;
  bool get canGoBack => _history.isNotEmpty;

  void selectSection(AppSection section) {
    final bool isSameSection = _currentSection == section;
    final bool hasDetail = activeDetail != null;
    if (isSameSection && !hasDetail) {
      return;
    }

    _recordCurrentState();
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

  void openAlbum(Album album) {
    openDetail(
      type: AppDetailType.album,
      itemId: album.itemId,
      providerInstanceIdOrDomain: album.provider,
    );
  }

  void openPlaylist(Playlist playlist) {
    openDetail(
      type: AppDetailType.playlist,
      itemId: playlist.itemId,
      providerInstanceIdOrDomain: playlist.provider,
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

    _recordCurrentState();
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
    return goBack();
  }

  bool goBack() {
    if (_history.isEmpty) {
      return false;
    }

    final _AppNavigationSnapshot previous = _history.removeLast();
    _currentSection = previous.section;
    _desktopSearchQuery = previous.desktopSearchQuery;
    _detailStack
      ..clear()
      ..addAll(previous.detailStack);
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

  void submitDesktopSearch(String query) {
    final String trimmed = query.trim();
    if (trimmed.isNotEmpty &&
        (_currentSection != AppSection.search || activeDetail != null)) {
      _recordCurrentState();
      _currentSection = AppSection.search;
      _detailStack.clear();
    }
    _desktopSearchQuery = trimmed;
    notifyListeners();
  }

  void clearDesktopSearch() {
    _desktopSearchQuery = '';
    notifyListeners();
  }

  void _recordCurrentState() {
    _history.add(
      _AppNavigationSnapshot(
        section: _currentSection,
        desktopSearchQuery: _desktopSearchQuery,
        detailStack: List<AppDetailDestination>.of(_detailStack),
      ),
    );
  }

  Widget buildPage(
    MusicAssistantClient client,
    PlaybackCoordinator playbackCoordinator, {
    bool desktopSearchDriven = false,
    UserDataController? userDataController,
    VoidCallback? onPlaylistDeleted,
  }) {
    final AppDetailDestination? detail = activeDetail;

    if (detail != null && detail.type == AppDetailType.artist) {
      return ArtistDetailPage(
        key: ValueKey<String>(
          'artist:${detail.itemId}:${detail.providerInstanceIdOrDomain}',
        ),
        client: client,
        playbackCoordinator: playbackCoordinator,
        itemId: detail.itemId,
        providerInstanceIdOrDomain: detail.providerInstanceIdOrDomain,
        userDataController: userDataController,
        onOpenArtist: openArtist,
        onOpenAlbum: openAlbum,
      );
    }

    if (detail != null && detail.type == AppDetailType.album) {
      return AlbumDetailPage(
        key: ValueKey<String>(
          'album:${detail.itemId}:${detail.providerInstanceIdOrDomain}',
        ),
        client: client,
        playbackCoordinator: playbackCoordinator,
        itemId: detail.itemId,
        providerInstanceIdOrDomain: detail.providerInstanceIdOrDomain,
        userDataController: userDataController,
        onOpenArtist: openArtist,
        onOpenAlbum: openAlbum,
      );
    }

    if (detail != null && detail.type == AppDetailType.playlist) {
      return PlaylistDetailPage(
        key: ValueKey<String>(
          'playlist:${detail.itemId}:${detail.providerInstanceIdOrDomain}',
        ),
        client: client,
        playbackCoordinator: playbackCoordinator,
        itemId: detail.itemId,
        providerInstanceIdOrDomain: detail.providerInstanceIdOrDomain,
        userDataController: userDataController,
        onOpenArtist: openArtist,
        onOpenAlbum: openAlbum,
        onPlaylistDeleted: onPlaylistDeleted,
      );
    }

    switch (currentSection) {
      case AppSection.home:
        return HomeScreen(client: client);
      case AppSection.explore:
        return const _SectionPlaceholder(
          title: 'Entdecken',
          subtitle: 'Genres, neue Releases, kuratierte Inhalte',
          icon: Icons.explore_outlined,
        );
      case AppSection.favorites:
        if (userDataController == null) {
          return const _SectionPlaceholder(
            title: 'Favoriten',
            subtitle: 'Deine favorisierten Tracks, Alben und Artists',
            icon: Icons.favorite_outlined,
          );
        }
        return FavoritesScreen(
          client: client,
          playbackCoordinator: playbackCoordinator,
          userDataController: userDataController,
          onOpenArtist: openArtist,
          onOpenAlbum: openAlbum,
          onOpenPlaylist: openPlaylist,
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
          userDataController: userDataController,
          desktopSearchDriven: desktopSearchDriven,
          desktopSearchQuery: desktopSearchQuery,
          onOpenArtist: openArtist,
          onOpenAlbum: openAlbum,
          onOpenPlaylist: openPlaylist,
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
