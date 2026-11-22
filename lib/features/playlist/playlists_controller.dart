import 'package:flutter/foundation.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_userdata.dart';

class PlaylistsController extends ChangeNotifier {
  PlaylistsController({
    required this.coreBridge,
    required this.userDataController,
  }) {
    _updatePlaylistSummaries();
    userDataController.addListener(_onUserDataChanged);
  }

  final MashinCoreBridgeApi coreBridge;
  final UserData userDataController;
  bool _isDisposed = false;
  bool _isLoadingSnapshot = true;
  bool _hasLoadError = false;
  List<Playlist> _observedPlaylists = const [];
  bool _wasLoadingPlaylists = true;
  final _playlistSummaries =
      <String, ({List<Track> tracks, int totalSeconds})>{};

  // Snapshot reads update status without gating cached or independently published playlists.
  Future<void> load() async {
    if (_isDisposed) return;
    _isLoadingSnapshot = true;
    _hasLoadError = false;
    notifyListeners();
    try {
      await userDataController.refreshLibrarySnapshot();
      if (!_isDisposed) {
        _updatePlaylistSummaries();
      }
    } catch (_) {
      if (!_isDisposed) _hasLoadError = userDataController.playlists.isEmpty;
    } finally {
      _isLoadingSnapshot = false;
      if (!_isDisposed) notifyListeners();
    }
  }

  // Current page projection.
  List<Playlist> get playlists => userDataController.playlists;
  bool get isLoading =>
      !_hasLoadError &&
      (_isLoadingSnapshot || userDataController.isLoadingPlaylists);
  bool get hasLoadError => _hasLoadError;

  ({int trackCount, int totalSeconds}) summaryFor(Playlist playlist) {
    final summary = _playlistSummaries[playlist.itemId];
    return (
      trackCount: summary?.tracks.length ?? 0,
      totalSeconds: summary?.totalSeconds ?? 0,
    );
  }

  List<Track> tracksFor(List<Playlist> playlists) => playlists
      .expand(
        (Playlist playlist) =>
            userDataController.tracksForPlaylist(playlist.itemId),
      )
      .toList(growable: false);

  // Playlist playback commands.
  Future<void> play(List<Track> tracks) => coreBridge.queuePlayMedia(tracks);

  Future<void> shuffle(List<Track> tracks) =>
      coreBridge.queuePlayMedia(tracks, shuffle: true);

  Future<void> playNext(List<Track> tracks) =>
      coreBridge.queuePlayMedia(tracks, option: QueueOption.next);

  Future<void> playLast(List<Track> tracks) =>
      coreBridge.queuePlayMedia(tracks, option: QueueOption.add);

  Future<Playlist?> createPlaylist(String name) =>
      userDataController.createPrivatePlaylist(name);

  Future<bool> renamePlaylist(Playlist playlist, String name) =>
      userDataController.renamePlaylist(playlist, name);

  Future<bool> refreshPrivatePlaylistCover(Playlist playlist) =>
      userDataController.refreshPrivatePlaylistCover(playlist);

  Future<void> deletePlaylists(List<Playlist> playlists) async {
    for (final Playlist playlist in playlists) {
      await userDataController.removePlaylist(playlist);
    }
  }

  @override
  void dispose() {
    _isDisposed = true;
    userDataController.removeListener(_onUserDataChanged);
    super.dispose();
  }

  // Playlist observation and summary projection.
  void _onUserDataChanged() {
    final hasChanged = _updatePlaylistSummaries();
    if (hasChanged) {
      if (userDataController.playlists.isNotEmpty) _hasLoadError = false;
      notifyListeners();
    }
  }

  // UserData retains unchanged track lists, so unrelated updates do not rescan tracks.
  bool _updatePlaylistSummaries() {
    final playlists = userDataController.playlists;
    final isLoadingPlaylists = userDataController.isLoadingPlaylists;
    var hasChanged =
        !identical(_observedPlaylists, playlists) ||
        _wasLoadingPlaylists != isLoadingPlaylists;
    _observedPlaylists = playlists;
    _wasLoadingPlaylists = isLoadingPlaylists;
    final retainedIds = {for (final playlist in playlists) playlist.itemId};
    _playlistSummaries.removeWhere(
      (itemId, _) => !retainedIds.contains(itemId),
    );
    for (final playlist in playlists) {
      final tracks = userDataController.tracksForPlaylist(playlist.itemId);
      final previous = _playlistSummaries[playlist.itemId];
      if (identical(previous?.tracks, tracks)) {
        continue;
      }
      final totalSeconds = tracks.fold<int>(
        0,
        (sum, track) => sum + track.duration,
      );
      _playlistSummaries[playlist.itemId] = (
        tracks: tracks,
        totalSeconds: totalSeconds,
      );
      hasChanged |=
          previous == null ||
          previous.tracks.length != tracks.length ||
          previous.totalSeconds != totalSeconds;
    }
    return hasChanged;
  }
}
