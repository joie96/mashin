import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';

import '../core/core_models.dart';
import '../core/core_bridge.dart';
import '../core/core_userdata.dart';

class HomeSimilarArtistSection {
  const HomeSimilarArtistSection({required this.title, required this.artists});

  final String title;
  final List<Artist> artists;
}

class HomeController extends ChangeNotifier {
  HomeController({required this.coreBridge, this.userDataController});

  final MashinCoreBridgeApi coreBridge;
  final UserData? userDataController;

  bool _isDisposed = false;
  final ChangeNotifier _recommendationsNotifier = ChangeNotifier();
  final ChangeNotifier _genrePlaylistsNotifier = ChangeNotifier();
  final ChangeNotifier _artistPlaylistsNotifier = ChangeNotifier();
  final ChangeNotifier _topTracksNotifier = ChangeNotifier();
  final ChangeNotifier _topArtistsNotifier = ChangeNotifier();
  final ChangeNotifier _similarArtistsNotifier = ChangeNotifier();
  final ChangeNotifier _recentListensNotifier = ChangeNotifier();

  bool _isLoadingRecommendations = true;
  bool _isLoadingGenrePlaylists = true;
  bool _isLoadingPlaylists = true;
  bool _isLoadingTopTracks = true;
  bool _isLoadingTopArtists = true;
  bool _isLoadingSimilarArtists = true;
  bool _isLoadingRecentListens = true;

  List<Track> _recommendationItems = const <Track>[];
  List<Playlist> _genrePlaylists = const <Playlist>[];
  List<Playlist> _artistPlaylists = const <Playlist>[];
  List<Track> _topTracks = const <Track>[];
  List<Artist> _topArtists = const <Artist>[];
  List<HomeSimilarArtistSection> _similarArtistSections =
      const <HomeSimilarArtistSection>[];
  List<Track> _recentListens = const <Track>[];

  bool get isLoadingRecommendations => _isLoadingRecommendations;
  bool get isLoadingGenrePlaylists => _isLoadingGenrePlaylists;
  bool get isLoadingPlaylists => _isLoadingPlaylists;
  bool get isLoadingTopTracks => _isLoadingTopTracks;
  bool get isLoadingTopArtists => _isLoadingTopArtists;
  bool get isLoadingSimilarArtists => _isLoadingSimilarArtists;
  bool get isLoadingRecentListens => _isLoadingRecentListens;

  List<Track> get recommendationItems => _recommendationItems;
  List<Playlist> get genrePlaylists => _genrePlaylists;
  List<Playlist> get artistPlaylists => _artistPlaylists;
  List<Track> get topTracks => _topTracks;
  List<Artist> get topArtists => _topArtists;
  List<HomeSimilarArtistSection> get similarArtistSections =>
      _similarArtistSections;
  List<Track> get recentListens => _recentListens;

  Listenable get recommendationsListenable => _recommendationsNotifier;
  Listenable get genrePlaylistsListenable => _genrePlaylistsNotifier;
  Listenable get artistPlaylistsListenable => _artistPlaylistsNotifier;
  Listenable get topTracksListenable => _topTracksNotifier;
  Listenable get topArtistsListenable => _topArtistsNotifier;
  Listenable get similarArtistsListenable => _similarArtistsNotifier;
  Listenable get recentListensListenable => _recentListensNotifier;

  bool get showNoRecommendations =>
      !_isLoadingRecommendations && _recommendationItems.isEmpty;
  bool get showNoGenrePlaylists =>
      !_isLoadingGenrePlaylists && _genrePlaylists.isEmpty;
  bool get showNoArtistPlaylists =>
      !_isLoadingPlaylists && _artistPlaylists.isEmpty;
  bool get showNoTopTracks => !_isLoadingTopTracks && _topTracks.isEmpty;
  bool get showNoTopArtists => !_isLoadingTopArtists && _topArtists.isEmpty;
  bool get showNoSimilarArtists =>
      !_isLoadingSimilarArtists && _similarArtistSections.isEmpty;
  bool get showNoRecentListens =>
      !_isLoadingRecentListens && _recentListens.isEmpty;

  @override
  void notifyListeners() {
    if (!_isDisposed) {
      super.notifyListeners();
    }
  }

  void _notifySection(ChangeNotifier notifier) {
    if (!_isDisposed) {
      notifier.notifyListeners();
    }
  }

  void _notifyRecommendationSections() {
    _notifySection(_recommendationsNotifier);
    _notifySection(_genrePlaylistsNotifier);
    _notifySection(_artistPlaylistsNotifier);
    _notifySection(_topTracksNotifier);
    _notifySection(_topArtistsNotifier);
    _notifySection(_similarArtistsNotifier);
  }

  Future<void> load() async {
    unawaited(_loadRecommendationFolders());
    unawaited(_loadRecentListens());
  }

  void refreshFavoriteStatuses() {
    _recommendationItems = _withFavoriteStatuses(_recommendationItems);
    _genrePlaylists = _withFavoriteStatuses(_genrePlaylists);
    _artistPlaylists = _withFavoriteStatuses(_artistPlaylists);
    _topTracks = _withFavoriteStatuses(_topTracks);
    _topArtists = _withFavoriteStatuses(_topArtists);
    _similarArtistSections = _similarArtistSections
        .map(
          (HomeSimilarArtistSection section) => HomeSimilarArtistSection(
            title: section.title,
            artists: _withFavoriteStatuses(section.artists),
          ),
        )
        .toList(growable: false);
    _recentListens = _withFavoriteStatuses(_recentListens);
    _notifyRecommendationSections();
    _notifySection(_recentListensNotifier);
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
      await coreBridge.queuePlayMedia(playable);
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
      await coreBridge.queuePlayMedia(playable, shuffle: true);
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
    _isLoadingSimilarArtists = true;
    _notifyRecommendationSections();
    notifyListeners();

    try {
      final List<RecommendationFolder> folders = await coreBridge
          .getRecommendations();
      final List<RecommendationFolder> listenBrainzFolders = folders
          .where(_isListenBrainzFolder)
          .toList(growable: false);

      final List<Track> recommendations = _tracksFromFolder(
        listenBrainzFolders,
        'recommendations',
      );
      _shuffle(recommendations);
      _recommendationItems = _withFavoriteStatuses(recommendations);
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
      _similarArtistSections = _similarArtistSectionsFromFolders(
        listenBrainzFolders,
      );
    } catch (_) {
      _recommendationItems = const <Track>[];
      _genrePlaylists = const <Playlist>[];
      _artistPlaylists = const <Playlist>[];
      _topTracks = const <Track>[];
      _topArtists = const <Artist>[];
      _similarArtistSections = const <HomeSimilarArtistSection>[];
    } finally {
      _isLoadingRecommendations = false;
      _isLoadingGenrePlaylists = false;
      _isLoadingPlaylists = false;
      _isLoadingTopTracks = false;
      _isLoadingTopArtists = false;
      _isLoadingSimilarArtists = false;
      _notifyRecommendationSections();
      notifyListeners();
    }
  }

  Future<void> _loadRecentListens() async {
    _isLoadingRecentListens = true;
    _notifySection(_recentListensNotifier);
    notifyListeners();
    try {
      final List<Track> recent = (await coreBridge.getRecentlyPlayedTracks())
          .whereType<Track>()
          .toList(growable: false);
      _recentListens = _withFavoriteStatuses(recent);
    } catch (_) {
      _recentListens = const <Track>[];
    } finally {
      _isLoadingRecentListens = false;
      _notifySection(_recentListensNotifier);
      notifyListeners();
    }
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

  List<HomeSimilarArtistSection> _similarArtistSectionsFromFolders(
    List<RecommendationFolder> folders,
  ) {
    return folders
        .where(
          (RecommendationFolder folder) =>
              folder.itemId.toLowerCase().startsWith('similar_artists_'),
        )
        .map(
          (RecommendationFolder folder) => HomeSimilarArtistSection(
            title: _normalizeSimilarArtistSectionTitle(folder.title),
            artists: _withFavoriteStatuses(
              folder.items
                  .whereType<Artist>()
                  .where(_isValidPlayable)
                  .toList(growable: false),
            ),
          ),
        )
        .where((HomeSimilarArtistSection section) => section.artists.isNotEmpty)
        .take(5)
        .toList(growable: false);
  }

  Playlist _withNormalizedPlaylistName(Playlist playlist) {
    final String normalizedName = _normalizePlaylistDisplayName(playlist.name);
    return Playlist(
      itemId: playlist.itemId,
      provider: playlist.provider,
      name: normalizedName,
      isEditable: playlist.isEditable,
      isPlayable: playlist.isPlayable,
      available: playlist.available,
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

  String _normalizeSimilarArtistSectionTitle(String title) {
    return title.replaceFirst(
      RegExp(r'^Similar Artists for\s*', caseSensitive: false),
      'Ähnlich zu ',
    );
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
    _recommendationsNotifier.dispose();
    _genrePlaylistsNotifier.dispose();
    _artistPlaylistsNotifier.dispose();
    _topTracksNotifier.dispose();
    _topArtistsNotifier.dispose();
    _similarArtistsNotifier.dispose();
    _recentListensNotifier.dispose();
    super.dispose();
  }
}
