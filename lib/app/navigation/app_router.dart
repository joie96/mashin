import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

import '../../data/music_assistant/ma_models.dart';
import '../../features/album/album_detail_page.dart';
import '../../features/artist/artist_detail_page.dart';
import '../../features/auth/login_screen.dart';
import '../../features/explore/explore_screen.dart';
import '../../features/favorites/favorites_screen.dart';
import '../../features/home/home_screen.dart';
import '../../features/playlist/playlist_detail_page.dart';
import '../../features/search/search_screen.dart';
import '../config/app_config.dart';
import '../session/app_session_controller.dart';
import '../shell/app_shell.dart';

final GlobalKey<NavigatorState> _shellNavigatorKey =
    GlobalKey<NavigatorState>();
final ShellNavigationObserver _shellNavigationObserver =
    ShellNavigationObserver();

enum AppSection { home, explore, favorites, playlists, search }

class ShellNavigationObserver extends NavigatorObserver with ChangeNotifier {
  @override
  void didPush(Route<dynamic> route, Route<dynamic>? previousRoute) {
    notifyListeners();
  }

  @override
  void didPop(Route<dynamic> route, Route<dynamic>? previousRoute) {
    notifyListeners();
  }

  @override
  void didRemove(Route<dynamic> route, Route<dynamic>? previousRoute) {
    notifyListeners();
  }

  @override
  void didReplace({Route<dynamic>? newRoute, Route<dynamic>? oldRoute}) {
    notifyListeners();
  }
}

abstract final class AppRoutes {
  static const String home = '/home';
  static const String explore = '/explore';
  static const String favorites = '/favorites';
  static const String playlists = '/playlists';
  static const String search = '/search';

  static String locationFor(AppSection section) {
    return switch (section) {
      AppSection.home => home,
      AppSection.explore => explore,
      AppSection.favorites => favorites,
      AppSection.playlists => playlists,
      AppSection.search => search,
    };
  }

  static AppSection? sectionForLocation(String location) {
    return switch (location) {
      home => AppSection.home,
      explore => AppSection.explore,
      favorites => AppSection.favorites,
      playlists => AppSection.playlists,
      search => AppSection.search,
      _ => null,
    };
  }

  static String artist(Artist artist) =>
      _detail('artist', artist.provider, artist.itemId);

  static String album(Album album) =>
      _detail('album', album.provider, album.itemId);

  static String playlist(Playlist playlist) =>
      _detail('playlist', playlist.provider, playlist.itemId);

  static String _detail(String type, String provider, String itemId) {
    return '/$type/${Uri.encodeComponent(provider)}/${Uri.encodeComponent(itemId)}';
  }
}

class AppRouter {
  AppRouter(this.session)
    : router = GoRouter(
        initialLocation: AppRoutes.home,
        refreshListenable: session,
        redirect: (BuildContext context, GoRouterState state) {
          final String location = state.uri.path;
          if (session.isLoading) {
            return location == '/loading' ? null : '/loading';
          }
          if (!session.isAuthenticated) {
            return location == '/login' ? null : '/login';
          }
          if (location == '/loading' || location == '/login') {
            return AppRoutes.home;
          }
          return null;
        },
        routes: <RouteBase>[
          GoRoute(
            path: '/loading',
            builder: (BuildContext context, GoRouterState state) =>
                const Scaffold(
                  body: Center(child: CircularProgressIndicator()),
                ),
          ),
          GoRoute(
            path: '/login',
            builder: (BuildContext context, GoRouterState state) => Scaffold(
              body: SafeArea(
                child: LoginScreen(
                  initialBaseUrl: session.baseUrl,
                  initialUsername: session.username,
                  onSubmit: session.login,
                ),
              ),
            ),
          ),
          ShellRoute(
            navigatorKey: _shellNavigatorKey,
            observers: <NavigatorObserver>[_shellNavigationObserver],
            builder:
                (BuildContext context, GoRouterState state, Widget child) =>
                    AppShell(
                      session: session,
                      navigatorKey: _shellNavigatorKey,
                      navigationObserver: _shellNavigationObserver,
                      child: child,
                    ),
            routes: <RouteBase>[
              GoRoute(
                path: AppRoutes.home,
                builder: (BuildContext context, GoRouterState state) =>
                    HomeScreen(
                      client: session.client!,
                      playbackCoordinator: session.playbackCoordinator,
                      userDataController: session.userDataController,
                      onOpenArtist: (Artist artist) =>
                          context.push(AppRoutes.artist(artist)),
                      onOpenAlbum: (Album album) =>
                          context.push(AppRoutes.album(album)),
                      onOpenPlaylist: (Playlist playlist) =>
                          context.push(AppRoutes.playlist(playlist)),
                    ),
              ),
              GoRoute(
                path: AppRoutes.explore,
                builder: (BuildContext context, GoRouterState state) =>
                    ExploreScreen(
                      client: session.client!,
                      playbackCoordinator: session.playbackCoordinator,
                      userDataController: session.userDataController,
                      onOpenArtist: (Artist artist) =>
                          context.push(AppRoutes.artist(artist)),
                    ),
              ),
              GoRoute(
                path: AppRoutes.favorites,
                builder: (BuildContext context, GoRouterState state) =>
                    FavoritesScreen(
                      client: session.client!,
                      playbackCoordinator: session.playbackCoordinator,
                      userDataController: session.userDataController!,
                      onOpenArtist: (Artist artist) =>
                          context.push(AppRoutes.artist(artist)),
                      onOpenAlbum: (Album album) =>
                          context.push(AppRoutes.album(album)),
                      onOpenPlaylist: (Playlist playlist) =>
                          context.push(AppRoutes.playlist(playlist)),
                    ),
              ),
              GoRoute(
                path: AppRoutes.playlists,
                builder: (BuildContext context, GoRouterState state) =>
                    const _SectionPlaceholder(
                      title: 'Playlists',
                      subtitle: 'Lokale und remote Playlists',
                      icon: Icons.queue_music_outlined,
                    ),
              ),
              GoRoute(
                path: AppRoutes.search,
                builder: (BuildContext context, GoRouterState state) =>
                    SearchScreen(
                      client: session.client!,
                      playbackCoordinator: session.playbackCoordinator,
                      userDataController: session.userDataController,
                      desktopSearchDriven: !AppConfig.isMobile(),
                      desktopSearchQuery: state.uri.queryParameters['q'] ?? '',
                      onOpenArtist: (Artist artist) =>
                          context.push(AppRoutes.artist(artist)),
                      onOpenAlbum: (Album album) =>
                          context.push(AppRoutes.album(album)),
                      onOpenPlaylist: (Playlist playlist) =>
                          context.push(AppRoutes.playlist(playlist)),
                    ),
              ),
              GoRoute(
                path: '/artist/:provider/:itemId',
                parentNavigatorKey: _shellNavigatorKey,
                builder: (BuildContext context, GoRouterState state) =>
                    ArtistDetailPage(
                      client: session.client!,
                      playbackCoordinator: session.playbackCoordinator,
                      itemId: state.pathParameters['itemId']!,
                      providerInstanceIdOrDomain:
                          state.pathParameters['provider']!,
                      userDataController: session.userDataController,
                      onOpenArtist: (Artist artist) =>
                          context.push(AppRoutes.artist(artist)),
                      onOpenAlbum: (Album album) =>
                          context.push(AppRoutes.album(album)),
                    ),
              ),
              GoRoute(
                path: '/album/:provider/:itemId',
                parentNavigatorKey: _shellNavigatorKey,
                builder: (BuildContext context, GoRouterState state) =>
                    AlbumDetailPage(
                      client: session.client!,
                      playbackCoordinator: session.playbackCoordinator,
                      itemId: state.pathParameters['itemId']!,
                      providerInstanceIdOrDomain:
                          state.pathParameters['provider']!,
                      userDataController: session.userDataController,
                      onOpenArtist: (Artist artist) =>
                          context.push(AppRoutes.artist(artist)),
                      onOpenAlbum: (Album album) =>
                          context.push(AppRoutes.album(album)),
                    ),
              ),
              GoRoute(
                path: '/playlist/:provider/:itemId',
                parentNavigatorKey: _shellNavigatorKey,
                builder: (BuildContext context, GoRouterState state) =>
                    PlaylistDetailPage(
                      client: session.client!,
                      playbackCoordinator: session.playbackCoordinator,
                      itemId: state.pathParameters['itemId']!,
                      providerInstanceIdOrDomain:
                          state.pathParameters['provider']!,
                      userDataController: session.userDataController,
                      onOpenArtist: (Artist artist) =>
                          context.push(AppRoutes.artist(artist)),
                      onOpenAlbum: (Album album) =>
                          context.push(AppRoutes.album(album)),
                      onPlaylistDeleted: context.pop,
                    ),
              ),
            ],
          ),
        ],
      );

  final AppSessionController session;
  final GoRouter router;
}

class _SectionPlaceholder extends StatelessWidget {
  const _SectionPlaceholder({
    required this.title,
    required this.subtitle,
    required this.icon,
  });

  final String title;
  final String subtitle;
  final IconData icon;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            Icon(icon, size: 46),
            const SizedBox(height: 14),
            Text(title, style: Theme.of(context).textTheme.headlineSmall),
            const SizedBox(height: 6),
            Text(subtitle, style: Theme.of(context).textTheme.bodyMedium),
          ],
        ),
      ),
    );
  }
}
