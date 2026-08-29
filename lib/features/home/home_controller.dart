import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';

import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/userdata_controller.dart';

class HomeController extends ChangeNotifier {
  HomeController({
    required this.client,
    required this.playbackCoordinator,
    this.userDataController,
  });

  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final UserDataController? userDataController;

  bool _isDisposed = false;
  bool _isLoadingRecommendations = true;
  bool _isLoadingGenrePlaylists = true;
  bool _isLoadingPlaylists = true;
  bool _isLoadingTopTracks = true;
  bool _isLoadingTopArtists = true;
  bool _isLoadingRecentListens = true;

  List<MediaItem> _recommendationItems = const <MediaItem>[];
  List<Playlist> _genrePlaylists = const <Playlist>[];
  List<Playlist> _artistPlaylists = const <Playlist>[];
  List<Track> _topTracks = const <Track>[];
  List<Artist> _topArtists = const <Artist>[];
  List<Track> _recentListens = const <Track>[];

  bool get isLoadingRecommendations => _isLoadingRecommendations;
  bool get isLoadingGenrePlaylists => _isLoadingGenrePlaylists;
  bool get isLoadingPlaylists => _isLoadingPlaylists;
  bool get isLoadingTopTracks => _isLoadingTopTracks;
  bool get isLoadingTopArtists => _isLoadingTopArtists;
  bool get isLoadingRecentListens => _isLoadingRecentListens;

  List<MediaItem> get recommendationItems => _recommendationItems;
  List<Playlist> get genrePlaylists => _genrePlaylists;
  List<Playlist> get artistPlaylists => _artistPlaylists;
  List<Track> get topTracks => _topTracks;
  List<Artist> get topArtists => _topArtists;
  List<Track> get recentListens => _recentListens;

  bool get showNoRecommendations =>
      !_isLoadingRecommendations && _recommendationItems.isEmpty;
  bool get showNoGenrePlaylists =>
      !_isLoadingGenrePlaylists && _genrePlaylists.isEmpty;
  bool get showNoArtistPlaylists =>
      !_isLoadingPlaylists && _artistPlaylists.isEmpty;
  bool get showNoTopTracks => !_isLoadingTopTracks && _topTracks.isEmpty;
  bool get showNoTopArtists => !_isLoadingTopArtists && _topArtists.isEmpty;
  bool get showNoRecentListens =>
      !_isLoadingRecentListens && _recentListens.isEmpty;

  @override
  void notifyListeners() {
    if (!_isDisposed) {
      super.notifyListeners();
    }
  }

  Future<void> load() async {
    try {
      await client.getProviderManifests();
      notifyListeners();
    } catch (_) {
      // Provider icons fall back when manifests are unavailable.
    }

    unawaited(_loadRecommendationFolders());
    unawaited(_loadRecentListens());
  }

  void refreshFavoriteStatuses() {
    _recommendationItems = _withFavoriteStatuses(_recommendationItems);
    _genrePlaylists = _withFavoriteStatuses(_genrePlaylists);
    _artistPlaylists = _withFavoriteStatuses(_artistPlaylists);
    _topTracks = _withFavoriteStatuses(_topTracks);
    _topArtists = _withFavoriteStatuses(_topArtists);
    _recentListens = _withFavoriteStatuses(_recentListens);
    notifyListeners();
  }

  bool isItemFavorite(MediaItem item) => item.favorite;

  Future<void> setFavorite(MediaItem item, bool favorite) async {
    await userDataController?.setFavorite(<MediaItem>[item], favorite);
  }

  Future<void> playAll(Iterable<MediaItem> items) async {
    final List<MediaItem> playable = items.where(_isValidPlayable).toList();
    if (playable.isEmpty) {
      return;
    }

    try {
      await playbackCoordinator.playMedia(playable);
    } catch (_) {
      // Playback failures should not break the home feed.
    }
  }

  Future<void> shuffleAll(Iterable<MediaItem> items) async {
    final List<MediaItem> playable = items.where(_isValidPlayable).toList();
    if (playable.isEmpty) {
      return;
    }

    try {
      await playbackCoordinator.shufflePlayMedia(playable);
    } catch (_) {
      // Playback failures should not break the home feed.
    }
  }

  Future<void> _loadRecommendationFolders() async {
    _isLoadingRecommendations = true;
    _isLoadingGenrePlaylists = true;
    _isLoadingPlaylists = true;
    _isLoadingTopTracks = true;
    _isLoadingTopArtists = true;
    notifyListeners();

    try {
      final List<RecommendationFolder> folders = await client
          .getRecommendations();
      final List<RecommendationFolder> listenBrainzFolders = folders
          .where(_isListenBrainzFolder)
          .toList(growable: false);

      final List<Track> recommendations = _tracksFromFolder(
        listenBrainzFolders,
        'recommendations',
      );
      _shuffle(recommendations);
      _recommendationItems = _withFavoriteStatuses(
        recommendations.cast<MediaItem>(),
      );
      _genrePlaylists = _withFavoriteStatuses(
        _playlistsFromFolder(
          listenBrainzFolders,
          'genre_playlists',
        ).map(_withNormalizedPlaylistName).take(9).toList(growable: false),
      );
      _artistPlaylists = _withFavoriteStatuses(
        _playlistsFromFolder(
          listenBrainzFolders,
          'artist_playlists',
        ).map(_withNormalizedPlaylistName).toList(growable: false),
      );
      _topTracks = _withFavoriteStatuses(
        _tracksFromFolder(listenBrainzFolders, 'top_tracks'),
      );
      _topArtists = _withFavoriteStatuses(
        _artistsFromFolder(listenBrainzFolders, 'top_artists'),
      );
    } catch (_) {
      _recommendationItems = const <MediaItem>[];
      _genrePlaylists = const <Playlist>[];
      _artistPlaylists = const <Playlist>[];
      _topTracks = const <Track>[];
      _topArtists = const <Artist>[];
    } finally {
      _isLoadingRecommendations = false;
      _isLoadingGenrePlaylists = false;
      _isLoadingPlaylists = false;
      _isLoadingTopTracks = false;
      _isLoadingTopArtists = false;
      notifyListeners();
    }
  }

  Future<void> _loadRecentListens() async {
    _isLoadingRecentListens = true;
    notifyListeners();
    try {
      final List<MediaItem> recent = await client.getRecentlyPlayedItems(
        limit: 40,
        mediaTypes: <MediaType>[MediaType.track],
      );
      _recentListens = _withFavoriteStatuses(
        recent.whereType<Track>().toList(),
      );
    } catch (_) {
      _recentListens = const <Track>[];
    } finally {
      _isLoadingRecentListens = false;
      notifyListeners();
    }
  }

  List<T> _withFavoriteStatuses<T extends MediaItem>(List<T> items) {
    final UserDataController? controller = userDataController;
    if (controller == null) {
      return items;
    }
    return items
        .map((T item) => controller.withFavoriteStatus(item))
        .toList(growable: false);
  }

  bool _isValidPlayable(MediaItem item) {
    return item.itemId.trim().isNotEmpty && item.provider.trim().isNotEmpty;
  }

  bool _isListenBrainzFolder(RecommendationFolder folder) {
    return folder.provider.startsWith('listenbrainz_recommendations--') &&
        folder.itemId.trim().isNotEmpty;
  }

  RecommendationFolder? _findFolderById(
    List<RecommendationFolder> folders,
    String itemId,
  ) {
    for (final RecommendationFolder folder in folders) {
      if (folder.itemId.toLowerCase() == itemId.toLowerCase()) {
        return folder;
      }
    }
    return null;
  }

  List<Track> _tracksFromFolder(
    List<RecommendationFolder> folders,
    String itemId,
  ) {
    return _findFolderById(folders, itemId)?.items
            .whereType<Track>()
            .where(_isValidPlayable)
            .toList(growable: false) ??
        const <Track>[];
  }

  List<Artist> _artistsFromFolder(
    List<RecommendationFolder> folders,
    String itemId,
  ) {
    return _findFolderById(folders, itemId)?.items
            .whereType<Artist>()
            .where(_isValidPlayable)
            .toList(growable: false) ??
        const <Artist>[];
  }

  List<Playlist> _playlistsFromFolder(
    List<RecommendationFolder> folders,
    String itemId,
  ) {
    return _findFolderById(folders, itemId)?.items
            .whereType<Playlist>()
            .where(_isValidPlayable)
            .toList(growable: false) ??
        const <Playlist>[];
  }

  Playlist _withNormalizedPlaylistName(Playlist playlist) {
    final String normalizedName = _normalizePlaylistDisplayName(playlist.name);
    return Playlist(
      itemId: playlist.itemId,
      provider: playlist.provider,
      name: normalizedName,
      isEditable: playlist.isEditable,
      sortName: playlist.sortName,
      uri: playlist.uri,
      owner: null,
      providerMappings: playlist.providerMappings,
      metadata: playlist.metadata,
      favorite: playlist.favorite,
    );
  }

  String _normalizePlaylistDisplayName(String playlistName) {
    const String radioPrefix = 'Radio: ';
    if (playlistName.toLowerCase().startsWith(radioPrefix.toLowerCase())) {
      return playlistName.substring(radioPrefix.length).trimLeft();
    }
    return playlistName;
  }

  void _shuffle<T>(List<T> items) {
    final math.Random random = math.Random();
    for (int index = items.length - 1; index > 0; index--) {
      final int swapIndex = random.nextInt(index + 1);
      final T item = items[index];
      items[index] = items[swapIndex];
      items[swapIndex] = item;
    }
  }

  @override
  void dispose() {
    _isDisposed = true;
    super.dispose();
  }
}
