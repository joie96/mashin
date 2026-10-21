import 'dart:math' as math;

import 'package:flutter/foundation.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_userdata.dart';

class ExploreController extends ChangeNotifier {
  ExploreController({
    required this.coreBridge,
    this.userDataController,
    math.Random? random,
  }) : _random = random ?? math.Random();

  final MashinCoreBridgeApi coreBridge;
  final UserData? userDataController;
  final math.Random _random;

  bool _isDisposed = false;
  bool _isLoading = false;
  bool _hasLoadError = false;
  List<Track> _weeklyExploration = const <Track>[];
  List<Album> _weeklyAlbums = const <Album>[];
  List<Track> _similarUsersJam = const <Track>[];
  List<Track> _weeklyTracks = const <Track>[];
  List<Artist> _weeklyArtists = const <Artist>[];

  // Lifecycle.
  @override
  void dispose() {
    _isDisposed = true;
    super.dispose();
  }

  // Recommendation snapshots.
  bool get isLoading => _isLoading;
  bool get hasLoadError => _hasLoadError;
  List<Track> get weeklyExploration => _weeklyExploration;
  List<Album> get weeklyAlbums => _weeklyAlbums;
  List<Track> get similarUsersJam => _similarUsersJam;
  List<Track> get weeklyTracks => _weeklyTracks;
  List<Artist> get weeklyArtists => _weeklyArtists;

  Future<void> load() async {
    if (_isDisposed || _isLoading) return;
    _isLoading = true;
    _hasLoadError = false;
    notifyListeners();
    try {
      final List<RecommendationFolder> folders = await coreBridge
          .getRecommendations();
      if (_isDisposed) return;
      final List<List<Track>> playlistTracks =
          await Future.wait(<Future<List<Track>>>[
            _tracksFromPlaylistFolders(folders, 'user_exploration_weekly'),
            _tracksFromPlaylistFolders(folders, 'similarusers_jam_weekly'),
          ]);
      if (_isDisposed) return;
      _weeklyExploration = playlistTracks[0]..shuffle(_random);
      _weeklyAlbums = _itemsFromFolders<Album>(
        folders,
        'sitewide_top_albums_weekly',
      );
      _similarUsersJam = playlistTracks[1];
      _weeklyTracks = _itemsFromFolders<Track>(
        folders,
        'sitewide_top_tracks_weekly',
      );
      _weeklyArtists = _itemsFromFolders<Artist>(
        folders,
        'sitewide_top_artists_weekly',
      );
    } catch (_) {
      if (!_isDisposed) _hasLoadError = true;
    } finally {
      if (!_isDisposed) {
        _isLoading = false;
        notifyListeners();
      }
    }
  }

  // Favorite commands and updates.
  Future<void> setFavorite(MediaItem item, bool favorite) async {
    await userDataController?.setFavorite(<MediaItem>[item], favorite);
  }

  void refreshFavoriteStatuses() {
    if (_isDisposed) return;
    _weeklyExploration = _withFavoriteStatuses(_weeklyExploration);
    _weeklyAlbums = _withFavoriteStatuses(_weeklyAlbums);
    _similarUsersJam = _withFavoriteStatuses(_similarUsersJam);
    _weeklyTracks = _withFavoriteStatuses(_weeklyTracks);
    _weeklyArtists = _withFavoriteStatuses(_weeklyArtists);
    notifyListeners();
  }

  // Folder mapping.
  Future<List<Track>> _tracksFromPlaylistFolders(
    List<RecommendationFolder> folders,
    String itemId,
  ) async {
    final List<Playlist> playlists = _itemsFromFolders<Playlist>(
      folders,
      itemId,
    );
    final List<List<Track>> tracks = await Future.wait(
      playlists.map(
        (Playlist playlist) =>
            coreBridge.getPlaylistTracks(playlist.itemId, playlist.provider),
      ),
    );
    return _withFavoriteStatuses(
      tracks.expand((List<Track> items) => items).toList(growable: false),
    );
  }

  List<T> _itemsFromFolders<T extends MediaItem>(
    List<RecommendationFolder> folders,
    String itemId,
  ) => _withFavoriteStatuses(
    folders
        .where((RecommendationFolder folder) => folder.itemId == itemId)
        .expand((RecommendationFolder folder) => folder.items)
        .whereType<T>()
        .where(
          (T item) =>
              item.itemId.trim().isNotEmpty && item.provider.trim().isNotEmpty,
        )
        .toList(growable: false),
  );

  List<T> _withFavoriteStatuses<T extends MediaItem>(List<T> items) {
    final UserData? controller = userDataController;
    return controller == null
        ? items
        : items
              .map((T item) => controller.withFavoriteStatus(item))
              .toList(growable: false);
  }
}
