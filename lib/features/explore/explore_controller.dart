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
      if (!_isDisposed &&
          _hasStartedLoading &&
          event['type'] == 'cache_cleared' &&
          event['cache'] == 'database') {
        unawaited(load(refresh: false));
      }
      if (!_isDisposed &&
          _hasStartedLoading &&
          event['type'] == 'recommendations_updated' &&
          event['view'] == 'explore') {
        _applySnapshot(
          RecommendationSnapshot.fromJson(
            event['snapshot'] as Map<String, dynamic>,
          ),
        );
      }
      if (!_isDisposed &&
          _hasStartedLoading &&
          event['type'] == 'recommendations_changed') {
        unawaited(load(refresh: false));
      }
    });
  }

  final MashinCoreBridgeApi coreBridge;
  final UserData? userDataController;
  final math.Random _random;
  StreamSubscription<Map<String, dynamic>>? _eventSubscription;
  Future<void>? _snapshotRefresh;
  bool _needsSnapshotRefresh = false;
  bool _shouldRefreshRecommendations = false;
  RecommendationSnapshot _snapshot = const RecommendationSnapshot(
    isLoading: true,
  );
  int _recommendationSequence = 0;
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

  Future<void> load({bool refresh = true}) {
    if (_isDisposed) return Future.value();
    _hasStartedLoading = true;
    _needsSnapshotRefresh = true;
    _shouldRefreshRecommendations |= refresh;
    return _snapshotRefresh ??= Future<void>.microtask(() async {
      try {
        while (_needsSnapshotRefresh && !_isDisposed) {
          _needsSnapshotRefresh = false;
          final shouldRefresh = _shouldRefreshRecommendations;
          _shouldRefreshRecommendations = false;
          await _loadSnapshot(refresh: shouldRefresh);
        }
      } finally {
        _snapshotRefresh = null;
      }
    });
  }

  Future<void> _loadSnapshot({required bool refresh}) async {
    _isLoading = _snapshot.isLoading;
    _hasLoadError = false;
    notifyListeners();
    try {
      final snapshot = await coreBridge.getRecommendations(
        view: RecommendationView.explore,
        refresh: refresh,
      );
      if (_isDisposed) return;
      _applySnapshot(snapshot);
    } catch (_) {
      if (!_isDisposed) {
        _hasLoadError = true;
        _snapshot = RecommendationSnapshot(folders: _snapshot.folders);
        _isLoading = false;
        notifyListeners();
      }
    }
  }

  void _applySnapshot(RecommendationSnapshot snapshot) {
    if (_isDisposed) return;
    if (snapshot.revision != 0 && snapshot.revision < _snapshot.revision) {
      return;
    }
    Set<String>? changedFolderIds;
    final delta = snapshot.delta;
    if (delta != null) {
      if (snapshot.revision == _snapshot.revision &&
          delta.sequence <= _recommendationSequence) {
        return;
      }
      final isNewRead = snapshot.revision > _snapshot.revision;
      if (delta.sequence != (isNewRead ? 1 : _recommendationSequence + 1)) {
        unawaited(load(refresh: false));
        return;
      }
      final update = snapshot;
      try {
        snapshot = snapshot.applyTo(
          isNewRead ? const RecommendationSnapshot() : _snapshot,
        );
      } on StateError {
        unawaited(load(refresh: false));
        return;
      }
      final retained = snapshot.folders
          .map((folder) => (folder.provider, folder.itemId))
          .toSet();
      changedFolderIds = {
        for (final folder in update.folders) folder.itemId,
        for (final folder in _snapshot.folders)
          if (!retained.contains((folder.provider, folder.itemId)))
            folder.itemId,
      };
      _recommendationSequence = delta.sequence;
    } else {
      if (snapshot.revision != 0 &&
          snapshot.revision == _snapshot.revision &&
          _recommendationSequence > 0) {
        return;
      }
      _recommendationSequence = 0;
    }
    bool hasChanged(String identity) =>
        changedFolderIds == null || changedFolderIds.contains(identity);
    final previous = _snapshot;
    final exploration = _weeklyExploration;
    final albums = _weeklyAlbums;
    final jam = _similarUsersJam;
    final tracks = _weeklyTracks;
    final artists = _weeklyArtists;
    _snapshot = snapshot;
    _isLoading = snapshot.isLoading;
    _hasLoadError = snapshot.hasLoadError;
    final folders = snapshot.folders;
    if (hasChanged('user_exploration_weekly') &&
        !snapshot.isFolderLoading('user_exploration_weekly') &&
        !snapshot.hasFolderLoadError('user_exploration_weekly')) {
      _weeklyExploration = _tracksFromPlaylistFolders(
        folders,
        'user_exploration_weekly',
      );
      if (!_hasLoadedExploration) {
        _weeklyExploration.shuffle(_random);
      } else {
        _preserveSelectedOrder(exploration, _weeklyExploration);
      }
      _hasLoadedExploration = true;
    }
    if (hasChanged('sitewide_top_albums_weekly') &&
        _canApplyFolder(snapshot, 'sitewide_top_albums_weekly')) {
      _weeklyAlbums = _itemsFromFolders<Album>(
        folders,
        'sitewide_top_albums_weekly',
      );
    }
    if (hasChanged('similarusers_jam_weekly') &&
        !snapshot.isFolderLoading('similarusers_jam_weekly') &&
        !snapshot.hasFolderLoadError('similarusers_jam_weekly')) {
      _similarUsersJam = _tracksFromPlaylistFolders(
        folders,
        'similarusers_jam_weekly',
      );
      if (!_hasLoadedSimilarUsersJam) {
        _similarUsersJam.shuffle(_random);
      } else {
        _preserveSelectedOrder(jam, _similarUsersJam);
      }
      _hasLoadedSimilarUsersJam = true;
    }
    if (hasChanged('sitewide_top_tracks_weekly') &&
        _canApplyFolder(snapshot, 'sitewide_top_tracks_weekly')) {
      _weeklyTracks = _itemsFromFolders<Track>(
        folders,
        'sitewide_top_tracks_weekly',
      );
    }
    if (hasChanged('sitewide_top_artists_weekly') &&
        _canApplyFolder(snapshot, 'sitewide_top_artists_weekly')) {
      _weeklyArtists = _itemsFromFolders<Artist>(
        folders,
        'sitewide_top_artists_weekly',
      );
    }
    _weeklyExploration = _retainMediaList(exploration, _weeklyExploration);
    _weeklyAlbums = _retainMediaList(albums, _weeklyAlbums);
    _similarUsersJam = _retainMediaList(jam, _similarUsersJam);
    _weeklyTracks = _retainMediaList(tracks, _weeklyTracks);
    _weeklyArtists = _retainMediaList(artists, _weeklyArtists);
    final hasChangedStatus =
        previous.isLoading != snapshot.isLoading ||
        previous.hasLoadError != snapshot.hasLoadError ||
        previous.folders.length != snapshot.folders.length ||
        snapshot.folders.indexed.any((entry) {
          final (index, folder) = entry;
          return index >= previous.folders.length ||
              previous.folders[index].itemId != folder.itemId ||
              previous.folders[index].provider != folder.provider ||
              previous.folders[index].isLoading != folder.isLoading ||
              previous.folders[index].hasLoadError != folder.hasLoadError;
        });
    if (hasChangedStatus ||
        !identical(exploration, _weeklyExploration) ||
        !identical(albums, _weeklyAlbums) ||
        !identical(jam, _similarUsersJam) ||
        !identical(tracks, _weeklyTracks) ||
        !identical(artists, _weeklyArtists)) {
      notifyListeners();
    }
  }

  // Preserve unchanged sections and the existing one-time shuffle selection.
  bool _canApplyFolder(RecommendationSnapshot snapshot, String itemId) =>
      !snapshot.isFolderLoading(itemId) && !snapshot.hasFolderLoadError(itemId);

  List<T> _retainMediaList<T extends MediaItem>(
    List<T> previous,
    List<T> incoming,
  ) => hasSameMediaListContent(previous, incoming) ? previous : incoming;

  void _preserveSelectedOrder(List<Track> previous, List<Track> incoming) {
    final order = {
      for (var index = 0; index < previous.length; index++)
        (previous[index].provider, previous[index].itemId): index,
    };
    incoming.sort(
      (first, second) =>
          (order[(first.provider, first.itemId)] ?? previous.length).compareTo(
            order[(second.provider, second.itemId)] ?? previous.length,
          ),
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
