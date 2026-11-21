import 'dart:async';

import 'package:flutter/foundation.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_userdata.dart';

class ExploreController extends ChangeNotifier {
  ExploreController({required this.coreBridge, this.userDataController}) {
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
  StreamSubscription<Map<String, dynamic>>? _eventSubscription;
  Future<void>? _snapshotRefresh;
  bool _needsSnapshotRefresh = false;
  bool _shouldRefreshRecommendations = false;
  RecommendationSnapshot _snapshot = const RecommendationSnapshot(
    isLoading: true,
  );
  int _recommendationSequence = 0;
  bool _hasStartedLoading = false;

  bool _isDisposed = false;
  bool _isLoading = true;
  bool _hasLoadError = false;
  List<RecommendationFolder> _folders = const [];

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
  bool isFolderLoading(String itemId) => _snapshot.isFolderLoading(itemId);

  List<T> itemsForFolder<T extends MediaItem>(String itemId) => _folders
      .where((folder) => folder.itemId == itemId)
      .expand((folder) => folder.items)
      .whereType<T>()
      .toList(growable: false);

  List<RecommendationFolder> foldersWithPrefix(String prefix) => _folders
      .where((folder) => folder.itemId.startsWith(prefix))
      .toList(growable: false);

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
      try {
        snapshot = snapshot.applyTo(
          isNewRead ? const RecommendationSnapshot() : _snapshot,
        );
      } on StateError {
        unawaited(load(refresh: false));
        return;
      }
      _recommendationSequence = delta.sequence;
    } else {
      if (snapshot.revision != 0 &&
          snapshot.revision == _snapshot.revision &&
          _recommendationSequence > 0) {
        return;
      }
      _recommendationSequence = 0;
    }
    final previous = _folders;
    final hadLoading = _isLoading;
    final hadLoadError = _hasLoadError;
    _snapshot = snapshot;
    _isLoading = snapshot.isLoading;
    _hasLoadError = snapshot.hasLoadError;
    final previousByIdentity = {
      for (final folder in previous) (folder.provider, folder.itemId): folder,
    };
    _folders = [
      for (final folder in snapshot.folders)
        _projectFolder(
          folder,
          previousByIdentity[(folder.provider, folder.itemId)],
        ),
      if (snapshot.isLoading || snapshot.hasLoadError)
        for (final folder in previous)
          if (!snapshot.folders.any(
            (incoming) =>
                incoming.provider == folder.provider &&
                incoming.itemId == folder.itemId,
          ))
            folder,
    ];
    final hasChanges =
        previous.length != _folders.length ||
        _folders.indexed.any(
          (entry) => !identical(entry.$2, previous[entry.$1]),
        );
    if (hasChanges || hadLoading != _isLoading || hadLoadError != _hasLoadError) {
      notifyListeners();
    }
  }

  // Favorite commands and updates.
  Future<void> setFavorite(MediaItem item, bool favorite) async {
    await userDataController?.setFavorite(<MediaItem>[item], favorite);
  }

  void refreshFavoriteStatuses() {
    if (_isDisposed) return;
    _folders = [for (final folder in _folders) _projectFolder(folder, null)];
    notifyListeners();
  }

  // Folder mapping.
  RecommendationFolder _projectFolder(
    RecommendationFolder folder,
    RecommendationFolder? previous,
  ) {
    final items = (folder.isLoading || folder.hasLoadError) && previous != null
        ? previous.items
        : _withFavoriteStatuses(
            folder.items
                .where(
                  (item) =>
                      item.itemId.trim().isNotEmpty &&
                      item.provider.trim().isNotEmpty,
                )
                .toList(growable: false),
          );
    if (previous != null &&
        previous.title == folder.title &&
        previous.subtitle == folder.subtitle &&
        previous.isLoading == folder.isLoading &&
        previous.hasLoadError == folder.hasLoadError &&
        hasSameMediaListContent(previous.items, items)) {
      return previous;
    }
    return RecommendationFolder(
      itemId: folder.itemId,
      provider: folder.provider,
      title: folder.title,
      subtitle: folder.subtitle,
      items: items,
      isLoading: folder.isLoading,
      hasLoadError: folder.hasLoadError,
    );
  }

  List<T> _withFavoriteStatuses<T extends MediaItem>(List<T> items) {
    final UserData? controller = userDataController;
    return controller == null
        ? items
        : items
              .map((T item) => controller.withFavoriteStatus(item))
              .toList(growable: false);
  }
}
