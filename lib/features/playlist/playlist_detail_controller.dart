import 'dart:async';

import 'package:flutter/foundation.dart';

import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';

/// Loads and holds the data of a playlist detail page (metadata + tracks) and
/// exposes the playback/favorite/edit actions used by the desktop and mobile pages.
class PlaylistDetailController extends ChangeNotifier {
  PlaylistDetailController({
    required this.client,
    required this.playbackCoordinator,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
  });

  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final String itemId;
  final String providerInstanceIdOrDomain;

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
      return '0 Min.';
    }

    final int hours = seconds ~/ 3600;
    final int minutes = (seconds % 3600) ~/ 60;
    if (hours <= 0) {
      return '$minutes Min.';
    }
    return '$hours Std. $minutes Min.';
  }

  // Data loading
  Future<void> loadPlaylist() async {
    _isLoadingMetadata = true;
    _isLoadingTracks = true;
    notifyListeners();

    await _loadMetadata();
    await _loadTracks();
  }

  Future<void> _loadMetadata() async {
    try {
      _playlist = await client.getPlaylist(
        itemId,
        providerInstanceIdOrDomain,
      );
      _isPlaylistFavorite = _playlist?.favorite ?? false;
    } catch (_) {
      _playlist = null;
    } finally {
      _isLoadingMetadata = false;
      notifyListeners();
    }
  }

  Future<void> _loadTracks() async {
    try {
      _tracks = List<Track>.unmodifiable(
        await client.getPlaylistTracks(
          itemId,
          providerInstanceIdOrDomain,
          forceRefresh: true,
        ),
      );
    } catch (_) {
      _tracks = const <Track>[];
    } finally {
      _isLoadingTracks = false;
      notifyListeners();
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
      if (next) {
        final String uri = item.uri ?? '';
        if (uri.isNotEmpty) {
          await client.addFavorite(uri);
        }
      } else {
        await client.removeFavorite(
          mediaType: item.mediaType,
          libraryItemId: item.itemId,
        );
      }
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
      if (next) {
        final String uri = current.uri ?? '';
        if (uri.isNotEmpty) {
          await client.addFavorite(uri);
        }
      } else {
        await client.removeFavorite(
          mediaType: MediaType.playlist,
          libraryItemId: current.itemId,
        );
      }
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
      // The API positions are 1-based.
      await client.removePlaylistTracks(current.itemId, <int>[position + 1]);
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
    await client.addPlaylistTracks(playlistId, uris);
  }

  String _favoriteKey(MediaItem item) =>
      '${item.mediaType.name}|${item.provider}|${item.itemId}';
}
