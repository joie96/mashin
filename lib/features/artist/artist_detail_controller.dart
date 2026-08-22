import 'package:flutter/foundation.dart';

import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';

/// Loads and holds all data required by the artist detail page (metadata, albums,
/// similar artists, top tracks) and exposes the playback/favorite/description actions
/// used by the desktop and mobile artist detail pages.
class ArtistDetailController extends ChangeNotifier {
  static const int descriptionCollapsedMaxLines = 4;

  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final String itemId;
  final String providerInstanceIdOrDomain;

  ArtistDetailController({
    required this.client,
    required this.playbackCoordinator,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
  });

  Artist? _artist;
  List<Album> _albums = const <Album>[];
  List<Artist> _similarArtists = const <Artist>[];
  List<Track> _topTracks = const <Track>[];

  bool _isLoadingMetadata = true;
  bool _isLoadingAlbums = true;
  bool _isLoadingSimilarArtists = true;
  bool _isLoadingTopTracks = true;

  bool _isArtistFavorite = false;
  final Map<String, bool> _favoriteOverrides = <String, bool>{};
  bool _isDescriptionExpanded = false;

  Artist? get artist => _artist;
  List<Album> get albums => _albums;
  List<Artist> get similarArtists => _similarArtists;
  List<Track> get topTracks => _topTracks;

  bool get isLoadingMetadata => _isLoadingMetadata;
  bool get isLoadingAlbums => _isLoadingAlbums;
  bool get isLoadingSimilarArtists => _isLoadingSimilarArtists;
  bool get isLoadingTopTracks => _isLoadingTopTracks;

  String get artistName {
    final String trimmed = _artist?.name.trim() ?? '';
    return trimmed.isNotEmpty ? trimmed : 'Unbekannter Artist';
  }

  String? get description {
    final String raw = _artist?.metadata?.description?.trim() ?? '';
    if (raw.isEmpty) {
      return null;
    }
    // Mirrors MAUI's RemoveBlankLinesConverter that collapses repeated blank lines.
    return raw.replaceAll(RegExp(r'\n\s*\n+'), '\n');
  }

  bool get hasDescription => description != null;
  bool get isArtistFavorite => _isArtistFavorite;
  bool get isDescriptionExpanded => _isDescriptionExpanded;
  int get descriptionMaxLines => _isDescriptionExpanded ? 1000 : descriptionCollapsedMaxLines;

  // Data Loading
  Future<void> load() async {
    await Future.wait<void>(<Future<void>>[
      _loadMetadata(),
      _loadAlbums(),
      _loadSimilarArtists(),
      _loadTopTracks(),
    ]);
  }

  Future<void> _loadMetadata() async {
    _isLoadingMetadata = true;
    notifyListeners();
    try {
      _artist = await client.getArtist(itemId, providerInstanceIdOrDomain);
      _isArtistFavorite = _artist?.favorite ?? false;
    } catch (_) {
      _artist = null;
    } finally {
      _isLoadingMetadata = false;
      notifyListeners();
    }
  }

  Future<void> _loadAlbums() async {
    _isLoadingAlbums = true;
    notifyListeners();
    try {
      _albums = await client.getArtistAlbums(itemId, providerInstanceIdOrDomain);
    } catch (_) {
      _albums = const <Album>[];
    } finally {
      _isLoadingAlbums = false;
      notifyListeners();
    }
  }

  Future<void> _loadSimilarArtists() async {
    _isLoadingSimilarArtists = true;
    notifyListeners();
    try {
      _similarArtists = await client.getSimilarArtists(itemId, providerInstanceIdOrDomain);
    } catch (_) {
      _similarArtists = const <Artist>[];
    } finally {
      _isLoadingSimilarArtists = false;
      notifyListeners();
    }
  }

  Future<void> _loadTopTracks() async {
    _isLoadingTopTracks = true;
    notifyListeners();
    try {
      _topTracks = await client.getArtistTopTracks(itemId, providerInstanceIdOrDomain);
    } catch (_) {
      _topTracks = const <Track>[];
    } finally {
      _isLoadingTopTracks = false;
      notifyListeners();
    }
  }

  // Favorites
  String _favoriteKey(MediaItem item) => '${item.mediaType.name}:${item.provider}:${item.itemId}';

  bool isItemFavorite(MediaItem item) {
    return _favoriteOverrides[_favoriteKey(item)] ?? item.favorite;
  }

  Future<void> toggleItemFavorite(MediaItem item) async {
    final String key = _favoriteKey(item);
    final bool nextFavorite = !isItemFavorite(item);
    _favoriteOverrides[key] = nextFavorite;
    notifyListeners();

    try {
      if (nextFavorite) {
        final String uri = item.uri ?? '';
        if (uri.isNotEmpty) {
          await client.addFavorite(uri);
        }
      } else {
        await client.removeFavorite(mediaType: item.mediaType, libraryItemId: item.itemId);
      }
    } catch (_) {
      // Revert the optimistic update on failure.
      _favoriteOverrides[key] = !nextFavorite;
      notifyListeners();
    }
  }

  Future<void> toggleArtistFavorite() async {
    final Artist? current = _artist;
    if (current == null) {
      return;
    }

    final bool nextFavorite = !_isArtistFavorite;
    _isArtistFavorite = nextFavorite;
    notifyListeners();

    try {
      if (nextFavorite) {
        final String uri = current.uri ?? '';
        if (uri.isNotEmpty) {
          await client.addFavorite(uri);
        }
      } else {
        await client.removeFavorite(mediaType: MediaType.artist, libraryItemId: current.itemId);
      }
    } catch (_) {
      _isArtistFavorite = !nextFavorite;
      notifyListeners();
    }
  }

  // Description
  void toggleDescription() {
    _isDescriptionExpanded = !_isDescriptionExpanded;
    notifyListeners();
  }

  // Playback
  List<MediaItem> _playableTracks(List<Track> tracks) {
    return tracks
        .where((Track track) => track.itemId.trim().isNotEmpty && track.provider.trim().isNotEmpty)
        .toList(growable: false);
  }

  Future<void> playArtist() async {
    final List<MediaItem> items = _playableTracks(_topTracks);
    if (items.isEmpty) {
      return;
    }
    try {
      await playbackCoordinator.playMedia(items);
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }

  Future<void> shuffleArtist() async {
    final List<MediaItem> items = _playableTracks(_topTracks);
    if (items.isEmpty) {
      return;
    }
    try {
      await playbackCoordinator.shufflePlayMedia(items);
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }
}
