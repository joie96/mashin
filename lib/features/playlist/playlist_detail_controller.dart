import 'dart:async';

import 'package:flutter/foundation.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_userdata.dart';

/// Loads and holds the data of a playlist detail page (metadata + tracks) and
/// exposes the playback/favorite/edit actions used by the desktop and mobile pages.
class PlaylistDetailController extends ChangeNotifier {
  PlaylistDetailController({
    required this.coreBridge,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
  }) {
    userDataController?.addListener(_onUserDataChanged);
  }

  final MashinCoreBridgeApi coreBridge;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;

  Playlist? _playlist;
  List<Track> _tracks = const <Track>[];
  final Map<String, bool> _favoriteOverrides = <String, bool>{};

  bool _isLoadingMetadata = true;
  bool _isLoadingTracks = true;
  bool _isPlaylistFavorite = false;
  bool _hasLoadedPrivatePlaylist = false;
  bool _isPlaylistDeleted = false;
  bool _isDisposed = false;
  int _loadGeneration = 0;

  @override
  void notifyListeners() {
    if (!_isDisposed) {
      super.notifyListeners();
    }
  }

  @override
  void dispose() {
    _isDisposed = true;
    _loadGeneration++;
    userDataController?.removeListener(_onUserDataChanged);
    super.dispose();
  }

  Playlist? get playlist => _playlist;
  List<Track> get tracks => _tracks;

  bool get isLoadingMetadata => _isLoadingMetadata;
  bool get isLoadingTracks => _isLoadingTracks;
  bool get isPlaylistFavorite => _isPlaylistFavorite;
  bool get isPlaylistDeleted => _isPlaylistDeleted;
  bool get isEditable => _playlist?.isEditable ?? false;
  bool get isPrivatePlaylist =>
      _playlist != null &&
      (userDataController?.isPrivatePlaylist(_playlist!) ?? false);

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
    final int loadGeneration = ++_loadGeneration;
    _isLoadingMetadata = true;
    _isLoadingTracks = true;
    notifyListeners();

    try {
      final Playlist? local = _resolvePlaylistFromUserData(
        itemId,
        providerInstanceIdOrDomain,
      );

      if (local != null) {
        final bool isUserPlaylist =
            userDataController?.isPrivatePlaylist(local) ?? false;
        final List<Track> storedTracks =
            userDataController
                ?.tracksForPlaylist(local.itemId)
                .toList(growable: false) ??
            const <Track>[];

        // Private MA playlists use the Rust-owned snapshot, refreshed by MA events.
        if (isUserPlaylist || storedTracks.isNotEmpty) {
          _loadPrivatePlaylist(local, storedTracks);
          return;
        }
      }

      await _loadMetadata();
      if (!_isDisposed && loadGeneration == _loadGeneration) {
        await _loadTracks(loadGeneration);
      }
    } finally {
      if (!_isDisposed && loadGeneration == _loadGeneration) {
        _isLoadingMetadata = false;
        _isLoadingTracks = false;
        notifyListeners();
      }
    }
  }

  void _onUserDataChanged() {
    if (_isDisposed || _isPlaylistDeleted) return;
    final local = _resolvePlaylistFromUserData(
      itemId,
      providerInstanceIdOrDomain,
    );
    if (local == null && _hasLoadedPrivatePlaylist) {
      _loadGeneration++;
      _isPlaylistDeleted = true;
      _playlist = null;
      _tracks = const <Track>[];
      _favoriteOverrides.clear();
      _isPlaylistFavorite = false;
      _isLoadingMetadata = false;
      _isLoadingTracks = false;
      notifyListeners();
      return;
    }
    if (local == null ||
        !(userDataController?.isPrivatePlaylist(local) ?? false)) {
      return;
    }
    _loadGeneration++;
    _loadPrivatePlaylist(
      local,
      userDataController!.tracksForPlaylist(local.itemId),
    );
    _isLoadingMetadata = false;
    _isLoadingTracks = false;
    notifyListeners();
  }

  void _loadPrivatePlaylist(Playlist playlist, List<Track> storedTracks) {
    _hasLoadedPrivatePlaylist = true;
    _playlist = userDataController?.withFavoriteStatus(playlist) ?? playlist;

    _tracks = List<Track>.unmodifiable(
      storedTracks
          .map((Track track) {
            final Track withFavorite =
                userDataController?.withFavoriteStatus(track) ?? track;
            _favoriteOverrides[_favoriteKey(track)] = withFavorite.favorite;
            return withFavorite;
          })
          .toList(growable: false),
    );

    _isPlaylistFavorite = _playlist?.favorite ?? false;
  }

  Future<void> _loadMetadata() async {
    try {
      final Playlist online = await coreBridge.getPlaylist(
        itemId,
        providerInstanceIdOrDomain,
      );
      _playlist = userDataController?.withFavoriteStatus(online) ?? online;
      _isPlaylistFavorite = _playlist?.favorite ?? false;
    } catch (_) {
      _playlist = null;
    }
  }

  Future<void> _loadTracks(int loadGeneration) async {
    try {
      final List<Track> loaded = await coreBridge.getPlaylistTracks(
        itemId,
        providerInstanceIdOrDomain,
        forceRefresh: true,
      );
      if (_isDisposed || loadGeneration != _loadGeneration) {
        return;
      }

      _tracks = List<Track>.unmodifiable(
        loaded
            .map((Track track) {
              final Track withFavorite =
                  userDataController?.withFavoriteStatus(track) ?? track;
              _favoriteOverrides[_favoriteKey(track)] = withFavorite.favorite;
              return withFavorite;
            })
            .toList(growable: false),
      );
      unawaited(_loadTrackAlbums(loadGeneration, _tracks));
    } catch (_) {
      if (!_isDisposed && loadGeneration == _loadGeneration) {
        _tracks = const <Track>[];
      }
    }
  }

  // Enriches online tracks without delaying the initial playlist display.
  Future<void> _loadTrackAlbums(
    int loadGeneration,
    List<Track> initialTracks,
  ) async {
    final Set<(String, String)> artistKeys = <(String, String)>{};
    for (final Track track in initialTracks) {
      if (track.album == null || track.album!.year != null) {
        continue;
      }
      for (final Artist artist in track.artists) {
        if (artist.itemId.isNotEmpty && artist.provider.isNotEmpty) {
          artistKeys.add((artist.provider, artist.itemId));
        }
      }
    }
    if (artistKeys.isEmpty) {
      return;
    }

    final List<List<Album>> albumGroups = await Future.wait(
      artistKeys.map((artistKey) async {
        try {
          return await coreBridge.getArtistAlbums(artistKey.$2, artistKey.$1);
        } catch (_) {
          return <Album>[];
        }
      }),
    );
    if (_isDisposed || loadGeneration != _loadGeneration) {
      return;
    }

    final Map<(String, String), Album> albumsByKey =
        <(String, String), Album>{};
    for (final List<Album> albums in albumGroups) {
      for (final Album album in albums) {
        albumsByKey[(
              album.provider.toLowerCase(),
              album.itemId.toLowerCase(),
            )] =
            album;
      }
    }
    bool hasChanges = false;
    final List<Track> enriched = _tracks
        .map((Track track) {
          final Album? reference = track.album;
          if (reference == null || reference.year != null) {
            return track;
          }
          final Album? album =
              albumsByKey[(
                reference.provider.toLowerCase(),
                reference.itemId.toLowerCase(),
              )];
          if (album == null) {
            return track;
          }
          hasChanges = true;
          return Track(
            itemId: track.itemId,
            provider: track.provider,
            name: track.name,
            duration: track.duration,
            artists: track.artists,
            version: track.version,
            sortName: track.sortName,
            uri: track.uri,
            externalIds: track.externalIds,
            isPlayable: track.isPlayable,
            available: track.available,
            album: album,
            image: track.image,
            providerMappings: track.providerMappings,
            metadata: track.metadata,
            favorite: track.favorite,
            position: track.position,
            dateAdded: track.dateAdded,
            discNumber: track.discNumber,
            trackNumber: track.trackNumber,
          );
        })
        .toList(growable: false);
    if (hasChanges) {
      _tracks = List<Track>.unmodifiable(enriched);
      notifyListeners();
    }
  }

  // Playback
  Future<void> playPlaylist() async {
    if (_tracks.isEmpty) {
      return;
    }
    await coreBridge.queuePlayMedia(_tracks);
  }

  Future<void> shufflePlaylist() async {
    if (_tracks.isEmpty) {
      return;
    }
    await coreBridge.queuePlayMedia(_tracks, shuffle: true);
  }

  Future<void> startTrackRadio(Track track) async {
    await coreBridge.startRadio(<MediaItem>[track]);
  }

  Future<void> startItemRadio(MediaItem item) async {
    await coreBridge.startRadio(<MediaItem>[item]);
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
      final UserData? controller = userDataController;
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
      final UserData? controller = userDataController;
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
      final bool isUserPlaylist =
          userDataController?.isPrivatePlaylist(current) ?? false;

      if (isUserPlaylist && userDataController != null) {
        await userDataController!.removePlaylistTracks(current.itemId, <Track>[
          track,
        ]);
      } else {
        // The API positions are 1-based.
        await coreBridge.removePlaylistTracks(current.itemId, <int>[
          position + 1,
        ]);
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
        current != null &&
        (userDataController?.isPrivatePlaylist(current) ?? false);

    if (isUserPlaylist && userDataController != null) {
      final List<Track> tracks = items.whereType<Track>().toList(
        growable: false,
      );
      if (tracks.isEmpty) {
        return;
      }

      await userDataController!.addPlaylistTracks(playlistId, tracks);
      return;
    }

    await coreBridge.addPlaylistTracks(playlistId, uris);
  }

  Playlist? _resolvePlaylistFromUserData(
    String targetItemId,
    String targetProvider,
  ) {
    final List<Playlist> playlists =
        userDataController?.playlists ?? const <Playlist>[];
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
