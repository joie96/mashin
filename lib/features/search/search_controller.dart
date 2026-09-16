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

    try {
      final SearchResults results = await coreBridge.searchMusic(query);
      if (requestId != _requestId) {
        return;
      }
      _tracks = _withFavoriteStatuses(results.tracks);
      _albums = _withFavoriteStatuses(results.albums);
      _playlists = _withFavoriteStatuses(results.playlists);
      _artists = _withFavoriteStatuses(results.artists);
    } catch (_) {
      if (requestId != _requestId) {
        return;
      }
    } finally {
      if (requestId == _requestId) {
        _isLoadingTracks = false;
        _isLoadingAlbums = false;
        _isLoadingPlaylists = false;
        _isLoadingArtists = false;
      }
      _notifyListenersIfAlive();
    }
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
