import 'dart:async';

import 'package:flutter/foundation.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_userdata.dart';

/// Holds search state and loading logic for tracks, albums, playlists and artists.
class SearchController extends ChangeNotifier {
  final MashinCoreBridgeApi coreBridge;
  final UserData? userDataController;

  SearchController({required this.coreBridge, this.userDataController});

  int _requestId = 0;
  bool _isDisposed = false;

  bool _hasSearchRequest = false;
  bool _isLoadingTracks = false;
  bool _isLoadingAlbums = false;
  bool _isLoadingPlaylists = false;
  bool _isLoadingArtists = false;

  List<Track> _tracks = const <Track>[];
  List<Album> _albums = const <Album>[];
  List<Playlist> _playlists = const <Playlist>[];
  List<Artist> _artists = const <Artist>[];

  bool get hasSearchRequest => _hasSearchRequest;
  bool get isLoadingTracks => _isLoadingTracks;
  bool get isLoadingAlbums => _isLoadingAlbums;
  bool get isLoadingPlaylists => _isLoadingPlaylists;
  bool get isLoadingArtists => _isLoadingArtists;

  List<Track> get tracks => _tracks;
  List<Album> get albums => _albums;
  List<Playlist> get playlists => _playlists;
  List<Artist> get artists => _artists;

  void refreshFavoriteStatuses() {
    _tracks = _withFavoriteStatuses(_tracks);
    _albums = _withFavoriteStatuses(_albums);
    _playlists = _withFavoriteStatuses(_playlists);
    _artists = _withFavoriteStatuses(_artists);
    _notifyListenersIfAlive();
  }

  Future<void> runSearch(String query) async {
    if (query.isEmpty) {
      resetResults();
      return;
    }

    final int requestId = ++_requestId;

    _hasSearchRequest = true;
    _isLoadingTracks = true;
    _isLoadingAlbums = true;
    _isLoadingPlaylists = true;
    _isLoadingArtists = true;
    _tracks = const <Track>[];
    _albums = const <Album>[];
    _playlists = const <Playlist>[];
    _artists = const <Artist>[];
    _notifyListenersIfAlive();

    await Future.wait<void>(<Future<void>>[
      _searchMedia(query, MediaType.track, requestId),
      _searchMedia(query, MediaType.album, requestId),
      _searchMedia(query, MediaType.playlist, requestId),
      _searchMedia(query, MediaType.artist, requestId),
    ]);
  }

  // Completes each result group independently, including failed requests.
  Future<void> _searchMedia(
    String query,
    MediaType mediaType,
    int requestId,
  ) async {
    try {
      final SearchResults results = await coreBridge.searchMedia(
        query,
        mediaType,
      );
      if (requestId != _requestId || _isDisposed) {
        return;
      }
      switch (mediaType) {
        case MediaType.track:
          _tracks = _withFavoriteStatuses(results.tracks);
          unawaited(_loadTrackAlbums(requestId, _tracks));
        case MediaType.album:
          _albums = _withFavoriteStatuses(results.albums);
        case MediaType.playlist:
          _playlists = _withFavoriteStatuses(results.playlists);
        case MediaType.artist:
          _artists = _withFavoriteStatuses(results.artists);
        default:
          throw ArgumentError.value(mediaType, 'mediaType');
      }
    } catch (_) {
    } finally {
      if (requestId == _requestId && !_isDisposed) {
        switch (mediaType) {
          case MediaType.track:
            _isLoadingTracks = false;
          case MediaType.album:
            _isLoadingAlbums = false;
          case MediaType.playlist:
            _isLoadingPlaylists = false;
          case MediaType.artist:
            _isLoadingArtists = false;
          default:
            break;
        }
        _notifyListenersIfAlive();
      }
    }
  }

  // Album details arrive after the initial track results are visible.
  Future<void> _loadTrackAlbums(
    int requestId,
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
    if (_isDisposed || requestId != _requestId) {
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
      _tracks = enriched;
      _notifyListenersIfAlive();
    }
  }

  void resetResults() {
    _requestId++;
    _hasSearchRequest = false;
    _isLoadingTracks = false;
    _isLoadingAlbums = false;
    _isLoadingPlaylists = false;
    _isLoadingArtists = false;
    _tracks = const <Track>[];
    _albums = const <Album>[];
    _playlists = const <Playlist>[];
    _artists = const <Artist>[];
    _notifyListenersIfAlive();
  }

  void _notifyListenersIfAlive() {
    if (_isDisposed) {
      return;
    }
    notifyListeners();
  }

  List<T> _withFavoriteStatuses<T extends MediaItem>(List<T> items) {
    final UserData? controller = userDataController;
    if (controller == null) {
      return items;
    }
    return items
        .map((T item) => controller.withFavoriteStatus(item))
        .toList(growable: false);
  }

  @override
  void dispose() {
    _isDisposed = true;
    _requestId++;
    super.dispose();
  }
}
