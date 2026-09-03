import 'dart:async';

import 'package:flutter/foundation.dart';

import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/userdata_controller.dart';

/// Loads and holds the data of a playlist detail page (metadata + tracks) and
/// exposes the playback/favorite/edit actions used by the desktop and mobile pages.
class PlaylistDetailController extends ChangeNotifier {
  PlaylistDetailController({
    required this.client,
    required this.playbackCoordinator,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
  });

  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserDataController? userDataController;

  Playlist? _playlist;
  List<Track> _tracks = const <Track>[];
  final Map<String, bool> _favoriteOverrides = <String, bool>{};

  bool _isLoadingMetadata = true;
  bool _isLoadingTracks = true;
  bool _isPlaylistFavorite = false;
  bool _isHeaderCollapsed = false;
  bool _isDisposed = false;

  @override
  void notifyListeners() {
    if (!_isDisposed) {
      super.notifyListeners();
    }
  }

  @override
  void dispose() {
    _isDisposed = true;
    super.dispose();
  }

  Playlist? get playlist => _playlist;
  List<Track> get tracks => _tracks;

  bool get isLoadingMetadata => _isLoadingMetadata;
  bool get isLoadingTracks => _isLoadingTracks;
  bool get isPlaylistFavorite => _isPlaylistFavorite;
  bool get isHeaderCollapsed => _isHeaderCollapsed;
  bool get isEditable => _playlist?.isEditable ?? false;
  bool get isLocalPlaylist =>
      _playlist?.provider.trim().toLowerCase() ==
      UserDataController.localPlaylistProvider;

  String get playlistName {
    final String name = _playlist?.name.trim() ?? '';
    return name.isEmpty ? 'Unbekannte Wiedergabeliste' : name;
  }

  String? get ownerName {
    final String owner = _playlist?.owner?.trim() ?? '';
    return owner.isEmpty ? null : owner;
  }

  String get tracksCountText {
    final int count = _tracks.length;
    return count == 1 ? '1 Titel' : '$count Titel';
  }

  String get totalDurationText {
    final int seconds = _tracks.fold<int>(
      0,
      (int sum, Track track) => sum + track.duration,
    );
    if (seconds <= 0) {
      return '0m';
    }

    final int hours = seconds ~/ 3600;
    final int minutes = (seconds % 3600) ~/ 60;
    if (hours > 0) {
      return '${hours}h ${minutes}m';
    }
    return '${minutes}m';
  }

  // Data loading
  Future<void> loadPlaylist() async {
    _isLoadingMetadata = true;
    _isLoadingTracks = true;
    notifyListeners();

    try {
      final Playlist? local = _resolvePlaylistFromUserData(
        itemId,
        providerInstanceIdOrDomain,
      );

      if (local != null) {
        final bool isUserPlaylist = local.provider.trim().toLowerCase() ==
            UserDataController.localPlaylistProvider;
        final List<Track> storedTracks = userDataController
                ?.tracksForPlaylist(local.itemId)
                .toList(growable: false) ??
            const <Track>[];

        // User playlists always come from local storage.
        // Favorited online playlists may exist locally without tracks and
        // should still load online to get fresh content.
        if (isUserPlaylist || storedTracks.isNotEmpty) {
          _loadLocalPlaylist(local, storedTracks);
          return;
        }
      }

      await _loadMetadata();
      await _loadTracks();
    } finally {
      _isLoadingMetadata = false;
      _isLoadingTracks = false;
      notifyListeners();
    }
  }

  void _loadLocalPlaylist(Playlist playlist, List<Track> storedTracks) {
    _playlist = userDataController?.withFavoriteStatus(playlist) ?? playlist;

    _tracks = List<Track>.unmodifiable(
      storedTracks.map((Track track) {
        final Track withFavorite =
            userDataController?.withFavoriteStatus(track) ?? track;
        _favoriteOverrides[_favoriteKey(track)] = withFavorite.favorite;
        return withFavorite;
      }).toList(growable: false),
    );

    _isPlaylistFavorite = _playlist?.favorite ?? false;
  }

  Future<void> _loadMetadata() async {
    try {
      final Playlist? online = await client.getPlaylist(
        itemId,
        providerInstanceIdOrDomain,
      );
      if (online != null) {
        _playlist = userDataController?.withFavoriteStatus(online) ?? online;
      } else {
        _playlist = null;
      }
      _isPlaylistFavorite = _playlist?.favorite ?? false;
    } catch (_) {
      _playlist = null;
    }
  }

  Future<void> _loadTracks() async {
    try {
      final List<Track> loaded = await client.getPlaylistTracks(
        itemId,
        providerInstanceIdOrDomain,
        forceRefresh: true,
      );

      _tracks = List<Track>.unmodifiable(
        loaded.map((Track track) {
          final Track withFavorite =
              userDataController?.withFavoriteStatus(track) ?? track;
          _favoriteOverrides[_favoriteKey(track)] = withFavorite.favorite;
          return withFavorite;
        }).toList(growable: false),
      );
    } catch (_) {
      _tracks = const <Track>[];
    }
  }

  // Header state
  void toggleHeaderCollapsed() {
    _isHeaderCollapsed = !_isHeaderCollapsed;
    notifyListeners();
  }

  // Playback
  Future<void> playPlaylist() async {
    if (_tracks.isEmpty) {
      return;
    }
    await playbackCoordinator.playMedia(_tracks);
  }

  Future<void> shufflePlaylist() async {
    if (_tracks.isEmpty) {
      return;
    }
    await playbackCoordinator.shufflePlayMedia(_tracks);
  }

  Future<void> startTrackRadio(Track track) async {
    await playbackCoordinator.playMedia(<MediaItem>[track]);
    await playbackCoordinator.playMediaRadioNext(<MediaItem>[track]);
  }

  Future<void> startItemRadio(MediaItem item) async {
    await playbackCoordinator.playMedia(<MediaItem>[item]);
    await playbackCoordinator.playMediaRadioNext(<MediaItem>[item]);
  }

  // Favorites
  bool isItemFavorite(MediaItem item) =>
      _favoriteOverrides[_favoriteKey(item)] ?? item.favorite;

  Future<void> toggleItemFavorite(MediaItem item) async {
    final String key = _favoriteKey(item);
    final bool next = !isItemFavorite(item);
    _favoriteOverrides[key] = next;
    notifyListeners();
    try {
      final UserDataController? controller = userDataController;
      if (controller == null) {
        _favoriteOverrides.remove(key);
        notifyListeners();
        return;
      }
      await controller.setFavorite(<MediaItem>[item], next);
    } catch (_) {
      _favoriteOverrides[key] = !next;
      notifyListeners();
    }
  }

  Future<void> togglePlaylistFavorite() async {
    final Playlist? current = _playlist;
    if (current == null) {
      return;
    }
    final bool next = !_isPlaylistFavorite;
    _isPlaylistFavorite = next;
    notifyListeners();
    try {
      final UserDataController? controller = userDataController;
      if (controller == null) {
        _isPlaylistFavorite = !next;
        notifyListeners();
        return;
      }
      await controller.setFavorite(<MediaItem>[current], next);
    } catch (_) {
      _isPlaylistFavorite = !next;
      notifyListeners();
    }
  }

  // Editing
  Future<void> removeTrack(Track track) async {
    final Playlist? current = _playlist;
    final int position = _tracks.indexOf(track);
    if (current == null || !current.isEditable || position < 0) {
      return;
    }

    final List<Track> previous = _tracks;
    _tracks = List<Track>.unmodifiable(
      List<Track>.of(_tracks)..removeAt(position),
    );
    notifyListeners();

    try {
      final bool isUserPlaylist = current.provider.trim().toLowerCase() ==
          UserDataController.localPlaylistProvider;

      if (isUserPlaylist && userDataController != null) {
        await userDataController!.removePlaylistTracks(
          current.itemId,
          <Track>[track],
        );
      } else {
        // The API positions are 1-based.
        await client.removePlaylistTracks(current.itemId, <int>[position + 1]);
      }
    } catch (_) {
      _tracks = previous;
      notifyListeners();
    }
  }

  Future<void> addTracksToPlaylist(
    String playlistId,
    List<MediaItem> items,
  ) async {
    final List<String> uris = items
        .map((MediaItem item) => item.uri ?? '')
        .where((String uri) => uri.isNotEmpty)
        .toList(growable: false);
    if (playlistId.trim().isEmpty || uris.isEmpty) {
      return;
    }

    final Playlist? current = _playlist;
    final bool isUserPlaylist =
        current?.provider.trim().toLowerCase() ==
        UserDataController.localPlaylistProvider;

    if (isUserPlaylist && userDataController != null) {
      final List<Track> tracks = items.whereType<Track>().toList(growable: false);
      if (tracks.isEmpty) {
        return;
      }

      await userDataController!.addPlaylistTracks(playlistId, tracks);
      return;
    }

    await client.addPlaylistTracks(playlistId, uris);
  }

  Playlist? _resolvePlaylistFromUserData(
    String targetItemId,
    String targetProvider,
  ) {
    final List<Playlist> playlists = userDataController?.playlists ??
        const <Playlist>[];
    if (playlists.isEmpty) {
      return null;
    }

    final String normalizedItemId = targetItemId.trim().toLowerCase();
    final String normalizedProvider = targetProvider.trim().toLowerCase();

    for (final Playlist playlist in playlists) {
      if (playlist.itemId.trim().toLowerCase() == normalizedItemId &&
          playlist.provider.trim().toLowerCase() == normalizedProvider) {
        return playlist;
      }
    }

    for (final Playlist playlist in playlists) {
      if (playlist.itemId.trim().toLowerCase() == normalizedItemId) {
        return playlist;
      }
    }

    return null;
  }

  String _favoriteKey(MediaItem item) =>
      '${item.mediaType.name}|${item.provider}|${item.itemId}';
}
