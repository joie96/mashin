import 'dart:async';
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
  }) : _random = random ?? math.Random() {
    _eventSubscription = coreBridge.events.listen((event) {
      if (_hasStartedLoading && event['type'] == 'recommendations_changed') {
        unawaited(load());
      }
    });
  }

  final MashinCoreBridgeApi coreBridge;
  final UserData? userDataController;
  final math.Random _random;
  StreamSubscription<Map<String, dynamic>>? _eventSubscription;
  Future<void>? _snapshotRefresh;
  bool _needsSnapshotRefresh = false;
  RecommendationSnapshot _snapshot = const RecommendationSnapshot(
    isLoading: true,
  );
  bool _hasLoadedExploration = false;
  bool _hasLoadedSimilarUsersJam = false;
  bool _hasStartedLoading = false;

  bool _isDisposed = false;
  bool _isLoading = true;
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
    unawaited(_eventSubscription?.cancel());
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
  bool isFolderLoading(String itemId) => _snapshot.isFolderLoading(itemId);

  Future<void> load() {
    if (_isDisposed) return Future.value();
    _hasStartedLoading = true;
    _needsSnapshotRefresh = true;
    return _snapshotRefresh ??= Future<void>.microtask(() async {
      try {
        while (_needsSnapshotRefresh && !_isDisposed) {
          _needsSnapshotRefresh = false;
          await _loadSnapshot();
        }
      } finally {
        _snapshotRefresh = null;
      }
    });
  }

  Future<void> _loadSnapshot() async {
    _isLoading = _snapshot.isLoading;
    _hasLoadError = false;
    notifyListeners();
    try {
      final snapshot = await coreBridge.getRecommendations(
        view: RecommendationView.explore,
      );
      if (_isDisposed) return;
      _applySnapshot(snapshot);
    } catch (_) {
      if (!_isDisposed) {
        _hasLoadError = true;
        _snapshot = RecommendationSnapshot(folders: _snapshot.folders);
      }
    } finally {
      if (!_isDisposed) {
        _isLoading = _snapshot.isLoading;
        notifyListeners();
      }
    }
  }

  void _applySnapshot(RecommendationSnapshot snapshot) {
    _snapshot = snapshot;
    _isLoading = snapshot.isLoading;
    final folders = snapshot.folders;
    if (!_hasLoadedExploration) {
      _weeklyExploration = _tracksFromPlaylistFolders(
        folders,
        'user_exploration_weekly',
      )..shuffle(_random);
      _hasLoadedExploration = !snapshot.isFolderLoading(
        'user_exploration_weekly',
      );
    }
    _weeklyAlbums = _itemsFromFolders<Album>(
      folders,
      'sitewide_top_albums_weekly',
    );
    if (!_hasLoadedSimilarUsersJam) {
      _similarUsersJam = _tracksFromPlaylistFolders(
        folders,
        'similarusers_jam_weekly',
      )..shuffle(_random);
      _hasLoadedSimilarUsersJam = !snapshot.isFolderLoading(
        'similarusers_jam_weekly',
      );
    }
    _weeklyTracks = _itemsFromFolders<Track>(
      folders,
      'sitewide_top_tracks_weekly',
    );
    _weeklyArtists = _itemsFromFolders<Artist>(
      folders,
      'sitewide_top_artists_weekly',
    );
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
  List<Track> _tracksFromPlaylistFolders(
    List<RecommendationFolder> folders,
    String itemId,
  ) {
    return _withFavoriteStatuses(
      folders
          .where((folder) => folder.itemId == itemId)
          .expand(
            (folder) => <Track>[
              ...folder.items.whereType<Track>(),
              ...folder.playlistTracks,
            ],
          )
          .toList(growable: false),
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
