import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';

import '../core/core_models.dart';
import '../core/core_bridge.dart';
import '../core/core_userdata.dart';

class HomeArtistSection {
  const HomeArtistSection({required this.title, required this.artists});

  final String title;
  final List<Artist> artists;
}

class HomeController extends ChangeNotifier {
  HomeController({
    required this.coreBridge,
    this.userDataController,
    math.Random? random,
  }) : _random = random ?? math.Random() {
    _eventSubscription = coreBridge.events.listen((event) {
      if (_hasStartedLoading && event['type'] == 'recommendations_changed') {
        unawaited(_refreshRecommendationFolders());
      }
    });
  }

  final MashinCoreBridgeApi coreBridge;
  final UserData? userDataController;
  final math.Random _random;
  StreamSubscription<Map<String, dynamic>>? _eventSubscription;
  bool _hasStartedLoading = false;
  Future<void>? _recommendationRefresh;
  bool _needsRecommendationRefresh = false;
  final Map<String, List<(String, String)>> _selectedArtistFolders = {};
  final Map<(String, String), HomeArtistSection> _loadedArtistSections = {};

  bool _isDisposed = false;
  final ChangeNotifier _recommendationsNotifier = ChangeNotifier();
  final ChangeNotifier _genrePlaylistsNotifier = ChangeNotifier();
  final ChangeNotifier _artistPlaylistsNotifier = ChangeNotifier();
  final ChangeNotifier _topTracksNotifier = ChangeNotifier();
  final ChangeNotifier _topArtistsNotifier = ChangeNotifier();
  final ChangeNotifier _artistSectionsNotifier = ChangeNotifier();
  final ChangeNotifier _recentListensNotifier = ChangeNotifier();

  bool _isLoadingRecommendations = true;
  bool _isLoadingGenrePlaylists = true;
  bool _isLoadingPlaylists = true;
  bool _isLoadingTopTracks = true;
  bool _isLoadingTopArtists = true;
  bool _isLoadingArtistSections = true;
  bool _isLoadingRecentListens = true;

  List<Track> _recommendationItems = const <Track>[];
  List<Playlist> _genrePlaylists = const <Playlist>[];
  List<Playlist> _artistPlaylists = const <Playlist>[];
  List<Track> _topTracks = const <Track>[];
  List<Artist> _topArtists = const <Artist>[];
  List<HomeArtistSection> _artistSections = const <HomeArtistSection>[];
  List<Track> _recentListens = const <Track>[];

  bool get isLoadingRecommendations => _isLoadingRecommendations;
  bool get isLoadingGenrePlaylists => _isLoadingGenrePlaylists;
  bool get isLoadingPlaylists => _isLoadingPlaylists;
  bool get isLoadingTopTracks => _isLoadingTopTracks;
  bool get isLoadingTopArtists => _isLoadingTopArtists;
  bool get isLoadingArtistSections => _isLoadingArtistSections;
  bool get isLoadingRecentListens => _isLoadingRecentListens;

  List<Track> get recommendationItems => _recommendationItems;
  List<Playlist> get genrePlaylists => _genrePlaylists;
  List<Playlist> get artistPlaylists => _artistPlaylists;
  List<Track> get topTracks => _topTracks;
  List<Artist> get topArtists => _topArtists;
  List<HomeArtistSection> get artistSections => _artistSections;
  List<Track> get recentListens => _recentListens;

  Listenable get recommendationsListenable => _recommendationsNotifier;
  Listenable get genrePlaylistsListenable => _genrePlaylistsNotifier;
  Listenable get artistPlaylistsListenable => _artistPlaylistsNotifier;
  Listenable get topTracksListenable => _topTracksNotifier;
  Listenable get topArtistsListenable => _topArtistsNotifier;
  Listenable get artistSectionsListenable => _artistSectionsNotifier;
  Listenable get recentListensListenable => _recentListensNotifier;

  bool get showNoRecommendations =>
      !_isLoadingRecommendations && _recommendationItems.isEmpty;
  bool get showNoGenrePlaylists =>
      !_isLoadingGenrePlaylists && _genrePlaylists.isEmpty;
  bool get showNoArtistPlaylists =>
      !_isLoadingPlaylists && _artistPlaylists.isEmpty;
  bool get showNoTopTracks => !_isLoadingTopTracks && _topTracks.isEmpty;
  bool get showNoTopArtists => !_isLoadingTopArtists && _topArtists.isEmpty;
  bool get showNoArtistSections =>
      !_isLoadingArtistSections && _artistSections.isEmpty;
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
    _notifySection(_artistSectionsNotifier);
  }

  Future<void> load() async {
    if (_isDisposed) return;
    _hasStartedLoading = true;
    unawaited(_refreshRecommendationFolders());
    unawaited(_loadRecentListens());
  }

  void refreshFavoriteStatuses() {
    _recommendationItems = _withFavoriteStatuses(_recommendationItems);
    _genrePlaylists = _withFavoriteStatuses(_genrePlaylists);
    _artistPlaylists = _withFavoriteStatuses(_artistPlaylists);
    _topTracks = _withFavoriteStatuses(_topTracks);
    _topArtists = _withFavoriteStatuses(_topArtists);
    _artistSections = _artistSections
        .map(
          (HomeArtistSection section) => HomeArtistSection(
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

  Future<void> _refreshRecommendationFolders() {
    _needsRecommendationRefresh = true;
    return _recommendationRefresh ??= Future<void>.microtask(() async {
      try {
        while (_needsRecommendationRefresh && !_isDisposed) {
          _needsRecommendationRefresh = false;
          await _loadRecommendationFolders();
        }
      } finally {
        _recommendationRefresh = null;
      }
    });
  }

  Future<void> _loadRecommendationFolders() async {
    try {
      final snapshot = await coreBridge.getRecommendations(
        view: RecommendationView.home,
      );
      if (_isDisposed) return;
      _applyRecommendationSnapshot(snapshot);
    } catch (_) {
      _isLoadingRecommendations = false;
      _isLoadingGenrePlaylists = false;
      _isLoadingPlaylists = false;
      _isLoadingTopTracks = false;
      _isLoadingTopArtists = false;
      _isLoadingArtistSections = false;
    } finally {
      _notifyRecommendationSections();
      notifyListeners();
    }
  }

  void _applyRecommendationSnapshot(RecommendationSnapshot snapshot) {
    final List<RecommendationFolder> recommendationFolders = snapshot.folders
        .where((folder) => folder.itemId.trim().isNotEmpty)
        .toList(growable: false);

    final List<Track> recommendations = List<Track>.of(
      _tracksFromFolder(recommendationFolders, 'user_recommendations'),
    );
    if (_isLoadingRecommendations) {
      _shuffle(recommendations);
      _recommendationItems = _withFavoriteStatuses(recommendations);
    }
    _genrePlaylists = _withFavoriteStatuses(
      _playlistsFromFolder(
        recommendationFolders,
        'user_genre_playlists_monthly',
      ).map(_withNormalizedPlaylistName).take(9).toList(growable: false),
    );
    _artistPlaylists = _withFavoriteStatuses(
      _playlistsFromFolder(
        recommendationFolders,
        'user_artist_playlists_monthly',
      ).map(_withNormalizedPlaylistName).toList(growable: false),
    );
    _topTracks = _withFavoriteStatuses(
      _tracksFromFolder(recommendationFolders, 'user_top_tracks_monthly'),
    );
    _topArtists = _withFavoriteStatuses(
      _artistsFromFolder(recommendationFolders, 'user_top_artists_monthly'),
    );
    final List<HomeArtistSection> genreSections = _artistSectionsFromFolders(
      recommendationFolders,
      'user_genre_artists_',
    );
    final List<HomeArtistSection> similarSections = _artistSectionsFromFolders(
      recommendationFolders,
      'user_similar_artists_',
    );
    _artistSections = <HomeArtistSection>[
      for (int index = 0; index < 2; index++) ...<HomeArtistSection>[
        if (index < genreSections.length) genreSections[index],
        if (index < similarSections.length) similarSections[index],
      ],
    ];
    _isLoadingRecommendations = snapshot.isFolderLoading(
      'user_recommendations',
    );
    _isLoadingGenrePlaylists = snapshot.isFolderLoading(
      'user_genre_playlists_monthly',
    );
    _isLoadingPlaylists = snapshot.isFolderLoading(
      'user_artist_playlists_monthly',
    );
    _isLoadingTopTracks = snapshot.isFolderLoading('user_top_tracks_monthly');
    _isLoadingTopArtists = snapshot.isFolderLoading('user_top_artists_monthly');
    _isLoadingArtistSections = snapshot.isLoading && _artistSections.isEmpty;
  }

  Future<void> _loadRecentListens() async {
    _isLoadingRecentListens = _recentListens.isEmpty;
    _notifySection(_recentListensNotifier);
    notifyListeners();
    try {
      final List<Track> recent = (await coreBridge.getRecentlyPlayedTracks())
          .whereType<Track>()
          .toList(growable: false);
      if (_isDisposed) return;
      _recentListens = _withFavoriteStatuses(recent);
    } catch (_) {
      if (_isLoadingRecentListens) _recentListens = const <Track>[];
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

  List<HomeArtistSection> _artistSectionsFromFolders(
    List<RecommendationFolder> folders,
    String prefix,
  ) {
    final candidates = folders
        .where((folder) => folder.itemId.toLowerCase().startsWith(prefix))
        .toList();
    if (candidates.isEmpty) return const [];
    final selected = _selectedArtistFolders.putIfAbsent(prefix, () {
      final identities = candidates
          .map((folder) => (folder.provider, folder.itemId))
          .toSet()
          .toList();
      _shuffle(identities);
      return identities.take(2).toList();
    });
    final sections = <HomeArtistSection>[];
    for (final identity in selected) {
      final folder = candidates.firstWhere(
        (folder) => (folder.provider, folder.itemId) == identity,
      );
      if (folder.isLoading) continue;
      final section = _loadedArtistSections.putIfAbsent(identity, () {
        final artists = folder.items
            .whereType<Artist>()
            .where(_isValidPlayable)
            .toList();
        _shuffle(artists);
        return HomeArtistSection(
          title: prefix == 'user_similar_artists_'
              ? _normalizeSimilarArtistSectionTitle(folder.title)
              : folder.title,
          artists: artists,
        );
      });
      if (section.artists.isNotEmpty) {
        sections.add(
          HomeArtistSection(
            title: section.title,
            artists: _withFavoriteStatuses(section.artists),
          ),
        );
      }
    }
    return sections;
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
    items.shuffle(_random);
  }

  @override
  void dispose() {
    _isDisposed = true;
    unawaited(_eventSubscription?.cancel());
    _recommendationsNotifier.dispose();
    _genrePlaylistsNotifier.dispose();
    _artistPlaylistsNotifier.dispose();
    _topTracksNotifier.dispose();
    _topArtistsNotifier.dispose();
    _artistSectionsNotifier.dispose();
    _recentListensNotifier.dispose();
    super.dispose();
  }
}
