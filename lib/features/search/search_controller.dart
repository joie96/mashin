import 'package:flutter/foundation.dart';

import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';

/// Holds search state and loading logic for tracks, albums, playlists and artists.
class SearchController extends ChangeNotifier {
  final MusicAssistantClient client;

  SearchController({required this.client});

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

  Future<void> load() async {
    try {
      await client.getProviderManifests();
      _notifyListenersIfAlive();
    } catch (_) {
      // Keep UI functional even if manifest loading fails.
    }
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
      _searchForType(requestId, query, MediaType.track),
      _searchForType(requestId, query, MediaType.album),
      _searchForType(requestId, query, MediaType.playlist),
      _searchForType(requestId, query, MediaType.artist),
    ]);
  }

  void resetResults() {
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

  Future<void> _searchForType(int requestId, String query, MediaType mediaType) async {
    try {
      final SearchResults? result = await client.search(
        searchQuery: query,
        mediaTypes: <MediaType>[mediaType],
        limit: 50,
        libraryOnly: false,
      );

      if (requestId != _requestId) {
        // Ignore stale responses when a newer query already started.
        return;
      }

      switch (mediaType) {
        case MediaType.track:
          _tracks = result?.tracks ?? const <Track>[];
          break;
        case MediaType.album:
          _albums = result?.albums ?? const <Album>[];
          break;
        case MediaType.playlist:
          _playlists = result?.playlists ?? const <Playlist>[];
          break;
        case MediaType.artist:
          _artists = result?.artists ?? const <Artist>[];
          break;
        default:
          break;
      }
    } catch (_) {
      if (requestId != _requestId) {
        return;
      }

      switch (mediaType) {
        case MediaType.track:
          _tracks = const <Track>[];
          break;
        case MediaType.album:
          _albums = const <Album>[];
          break;
        case MediaType.playlist:
          _playlists = const <Playlist>[];
          break;
        case MediaType.artist:
          _artists = const <Artist>[];
          break;
        default:
          break;
      }
    } finally {
      if (requestId == _requestId) {
        switch (mediaType) {
          case MediaType.track:
            _isLoadingTracks = false;
            break;
          case MediaType.album:
            _isLoadingAlbums = false;
            break;
          case MediaType.playlist:
            _isLoadingPlaylists = false;
            break;
          case MediaType.artist:
            _isLoadingArtists = false;
            break;
          default:
            break;
        }
      }
      _notifyListenersIfAlive();
    }
  }

  void _notifyListenersIfAlive() {
    if (_isDisposed) {
      return;
    }
    notifyListeners();
  }

  @override
  void dispose() {
    _isDisposed = true;
    _requestId++;
    super.dispose();
  }
}