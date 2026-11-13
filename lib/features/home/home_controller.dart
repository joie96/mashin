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
    DateTime Function()? now,
  }) : _random = random ?? math.Random(),
       _now = now ?? DateTime.now {
    _playerStateSubscription = coreBridge.playerStateChanges.listen(
      _handlePlaybackChanged,
    );
    _eventSubscription = coreBridge.events.listen((event) {
      if (!_isDisposed &&
          _hasStartedLoading &&
          event['type'] == 'provider_instances_changed') {
        unawaited(_loadRecentListens());
      }
      if (!_isDisposed &&
          _hasStartedLoading &&
          event['type'] == 'cache_cleared' &&
          event['cache'] == 'database') {
        unawaited(_refreshRecommendationFolders(requiresRestart: true));
        unawaited(_loadRecentListens());
      }
      if (!_isDisposed &&
          _hasStartedLoading &&
          event['type'] == 'recommendations_updated' &&
          event['view'] == 'home') {
        _applyRecommendationSnapshot(
          RecommendationSnapshot.fromJson(
            event['snapshot'] as Map<String, dynamic>,
          ),
        );
      }
      if (!_isDisposed &&
          _hasStartedLoading &&
          event['type'] == 'recommendations_changed') {
        unawaited(_refreshRecommendationFolders());
      }
      if (!_isDisposed &&
          _hasStartedLoading &&
          event['type'] == 'user_data_changed' &&
          _hasRecentListensLoadError &&
          !_isReadingRecentListens &&
          _recentListensRecoveryAttempts < 3) {
        _recentListensRecoveryAttempts++;
        unawaited(_loadRecentListens());
      }
    });
    _catalogSubscription = coreBridge.catalogUpdates.listen((update) {
      if (_isDisposed ||
          !_hasStartedLoading ||
          update.kind != CatalogQueryKind.recentlyPlayed) {
        return;
      }
      _recentListens = _withFavoriteStatuses(update.tracks);
      _isLoadingRecentListens = false;
      _recentListensNotifier.notifyListeners();
      notifyListeners();
    });
  }

  final MashinCoreBridgeApi coreBridge;
  final UserData? userDataController;
  final math.Random _random;
  final DateTime Function() _now;
  StreamSubscription<PlayerState>? _playerStateSubscription;
  StreamSubscription<Map<String, dynamic>>? _eventSubscription;
  StreamSubscription<CatalogUpdate>? _catalogSubscription;
  bool _hasStartedLoading = false;
  Future<void>? _recommendationRefresh;
  bool _needsRecommendationRefresh = false;
  bool _shouldRefreshRecommendations = false;
  final Map<String, List<(String, String)>> _selectedArtistFolders = {};
  final Map<(String, String), HomeArtistSection> _loadedArtistSections = {};
  RecommendationSnapshot _snapshot = const RecommendationSnapshot(
    isLoading: true,
  );
  int _recommendationSequence = 0;

  bool _isDisposed = false;
  final ChangeNotifier _recommendationsNotifier = ChangeNotifier();
  final ChangeNotifier _genrePlaylistsNotifier = ChangeNotifier();
  final ChangeNotifier _artistPlaylistsNotifier = ChangeNotifier();
  final ChangeNotifier _topTracksNotifier = ChangeNotifier();
  final ChangeNotifier _topArtistsNotifier = ChangeNotifier();
  final ChangeNotifier _artistSectionsNotifier = ChangeNotifier();
  final ChangeNotifier _recentListensNotifier = ChangeNotifier();

  bool _isLoadingRecommendations = true;
  bool _hasLoadedRecommendations = false;
  bool _hasLoadError = false;
  bool _isLoadingGenrePlaylists = true;
  bool _isLoadingPlaylists = true;
  bool _isLoadingTopTracks = true;
  bool _isLoadingTopArtists = true;
  bool _isLoadingArtistSections = true;
  bool _isLoadingRecentListens = true;
  bool _isReadingRecentListens = false;
  bool _hasRecentListensLoadError = false;
  int _recentListensRecoveryAttempts = 0;
  static const _recentListensRefreshInterval = Duration(seconds: 30);
  PlayerState? _previousPlaybackState;
  DateTime? _previousPlaybackObservedAt;
  DateTime? _lastRecentListensReadAt;
  Duration _currentTrackPlayedDuration = Duration.zero;
  bool _shouldRefreshRecentListens = false;

  List<Track> _recommendationItems = const <Track>[];
  List<Playlist> _genrePlaylists = const <Playlist>[];
  List<Playlist> _artistPlaylists = const <Playlist>[];
  List<Track> _topTracks = const <Track>[];
  List<Artist> _topArtists = const <Artist>[];
  List<HomeArtistSection> _artistSections = const <HomeArtistSection>[];
  List<Track> _recentListens = const <Track>[];

  bool get isLoadingRecommendations => _isLoadingRecommendations;
  bool get hasLoadError => _hasLoadError;
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

  Future<void> load({bool refresh = false}) async {
    if (_isDisposed) return;
    _hasStartedLoading = true;
    unawaited(_refreshRecommendationFolders(refresh: refresh));
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

  Future<void> _refreshRecommendationFolders({
    bool refresh = false,
    bool requiresRestart = false,
  }) {
    final pending = _recommendationRefresh;
    if (pending != null && !refresh && !requiresRestart) return pending;
    _shouldRefreshRecommendations |= refresh;
    _needsRecommendationRefresh = true;
    return _recommendationRefresh ??= Future<void>.microtask(() async {
      try {
        while (_needsRecommendationRefresh && !_isDisposed) {
          _needsRecommendationRefresh = false;
          final shouldRefresh = _shouldRefreshRecommendations;
          _shouldRefreshRecommendations = false;
          await _loadRecommendationFolders(refresh: shouldRefresh);
        }
      } finally {
        _recommendationRefresh = null;
      }
    });
  }

  Future<void> _loadRecommendationFolders({required bool refresh}) async {
    try {
      final snapshot = await coreBridge.getRecommendations(
        view: RecommendationView.home,
        refresh: refresh,
      );
      if (_isDisposed) return;
      _applyRecommendationSnapshot(snapshot);
    } catch (_) {
      if (_isDisposed) return;
      _hasLoadError = true;
      _isLoadingRecommendations = false;
      _isLoadingGenrePlaylists = false;
      _isLoadingPlaylists = false;
      _isLoadingTopTracks = false;
      _isLoadingTopArtists = false;
      _isLoadingArtistSections = false;
      _notifyRecommendationSections();
      notifyListeners();
    }
  }

  void _applyRecommendationSnapshot(RecommendationSnapshot snapshot) {
    if (_isDisposed) return;
    Set<String>? changedFolderIds;
    final delta = snapshot.delta;
    if (snapshot.revision != 0 && snapshot.revision < _snapshot.revision) {
      return;
    }
    if (delta != null) {
      if (snapshot.revision == _snapshot.revision &&
          delta.sequence <= _recommendationSequence) {
        return;
      }
      final isNewRead = snapshot.revision > _snapshot.revision;
      if (delta.sequence != (isNewRead ? 1 : _recommendationSequence + 1)) {
        unawaited(_refreshRecommendationFolders(requiresRestart: true));
        return;
      }
      final update = snapshot;
      try {
        snapshot = snapshot.applyTo(
          isNewRead ? const RecommendationSnapshot() : _snapshot,
        );
      } on StateError {
        unawaited(_refreshRecommendationFolders(requiresRestart: true));
        return;
      }
      final retained = snapshot.folders
          .map((folder) => (folder.provider, folder.itemId))
          .toSet();
      changedFolderIds = {
        for (final folder in update.folders) folder.itemId,
        for (final folder in _snapshot.folders)
          if (!retained.contains((folder.provider, folder.itemId)))
            folder.itemId,
      };
      _recommendationSequence = delta.sequence;
    } else {
      if (snapshot.revision != 0 &&
          snapshot.revision == _snapshot.revision &&
          _recommendationSequence > 0) {
        return;
      }
      _recommendationSequence = 0;
    }
    _snapshot = snapshot;
    bool hasChanged(String identity) =>
        changedFolderIds == null || changedFolderIds.contains(identity);
    final previousRecommendations = _recommendationItems;
    final previousGenrePlaylists = _genrePlaylists;
    final previousArtistPlaylists = _artistPlaylists;
    final previousTopTracks = _topTracks;
    final previousTopArtists = _topArtists;
    final previousArtistSections = _artistSections;
    final previousLoading = [
      _isLoadingRecommendations,
      _isLoadingGenrePlaylists,
      _isLoadingPlaylists,
      _isLoadingTopTracks,
      _isLoadingTopArtists,
      _isLoadingArtistSections,
    ];
    final hadLoadError = _hasLoadError;
    _hasLoadError = snapshot.hasLoadError;
    final List<RecommendationFolder> recommendationFolders = snapshot.folders
        .where((folder) => folder.itemId.trim().isNotEmpty)
        .toList(growable: false);

    if (hasChanged('user_recommendations') &&
        !snapshot.isFolderLoading('user_recommendations') &&
        !snapshot.hasFolderLoadError('user_recommendations')) {
      final recommendations = List<Track>.of(
        _tracksFromFolder(recommendationFolders, 'user_recommendations'),
      );
      if (!_hasLoadedRecommendations) {
        _shuffle(recommendations);
      } else {
        _preserveSelectedOrder(_recommendationItems, recommendations);
      }
      _recommendationItems = _withFavoriteStatuses(recommendations);
      _hasLoadedRecommendations = true;
    }
    if (hasChanged('user_genre_playlists_monthly') &&
        _canApplyFolder(snapshot, 'user_genre_playlists_monthly')) {
      _genrePlaylists = _withFavoriteStatuses(
        _playlistsFromFolder(
          recommendationFolders,
          'user_genre_playlists_monthly',
        ).map(_withNormalizedPlaylistName).take(9).toList(growable: false),
      );
    }
    if (hasChanged('user_artist_playlists_monthly') &&
        _canApplyFolder(snapshot, 'user_artist_playlists_monthly')) {
      _artistPlaylists = _withFavoriteStatuses(
        _playlistsFromFolder(
          recommendationFolders,
          'user_artist_playlists_monthly',
        ).map(_withNormalizedPlaylistName).toList(growable: false),
      );
    }
    if (hasChanged('user_top_tracks_monthly') &&
        _canApplyFolder(snapshot, 'user_top_tracks_monthly')) {
      _topTracks = _withFavoriteStatuses(
        _tracksFromFolder(recommendationFolders, 'user_top_tracks_monthly'),
      );
    }
    if (hasChanged('user_top_artists_monthly') &&
        _canApplyFolder(snapshot, 'user_top_artists_monthly')) {
      _topArtists = _withFavoriteStatuses(
        _artistsFromFolder(recommendationFolders, 'user_top_artists_monthly'),
      );
    }
    if (changedFolderIds == null ||
        changedFolderIds.any(
          (identity) =>
              identity.startsWith('user_genre_artists_') ||
              identity.startsWith('user_similar_artists_'),
        )) {
      final List<HomeArtistSection> genreSections = _artistSectionsFromFolders(
        recommendationFolders,
        'user_genre_artists_',
      );
      final List<HomeArtistSection> similarSections =
          _artistSectionsFromFolders(
            recommendationFolders,
            'user_similar_artists_',
          );
      final artistSections = <HomeArtistSection>[
        for (int index = 0; index < 2; index++) ...<HomeArtistSection>[
          if (index < genreSections.length) genreSections[index],
          if (index < similarSections.length) similarSections[index],
        ],
      ];
      if (artistSections.isNotEmpty ||
          (!snapshot.isLoading && !snapshot.hasLoadError)) {
        _artistSections = artistSections;
      }
    }
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
    _recommendationItems = _retainMediaList(
      previousRecommendations,
      _recommendationItems,
    );
    _genrePlaylists = _retainMediaList(previousGenrePlaylists, _genrePlaylists);
    _artistPlaylists = _retainMediaList(
      previousArtistPlaylists,
      _artistPlaylists,
    );
    _topTracks = _retainMediaList(previousTopTracks, _topTracks);
    _topArtists = _retainMediaList(previousTopArtists, _topArtists);
    if (previousArtistSections.length == _artistSections.length &&
        List.generate(
          _artistSections.length,
          (index) =>
              previousArtistSections[index].title ==
                  _artistSections[index].title &&
              hasSameMediaListContent(
                previousArtistSections[index].artists,
                _artistSections[index].artists,
              ),
        ).every((isEqual) => isEqual)) {
      _artistSections = previousArtistSections;
    }
    final changes = [
      !identical(previousRecommendations, _recommendationItems) ||
          previousLoading[0] != _isLoadingRecommendations,
      !identical(previousGenrePlaylists, _genrePlaylists) ||
          previousLoading[1] != _isLoadingGenrePlaylists,
      !identical(previousArtistPlaylists, _artistPlaylists) ||
          previousLoading[2] != _isLoadingPlaylists,
      !identical(previousTopTracks, _topTracks) ||
          previousLoading[3] != _isLoadingTopTracks,
      !identical(previousTopArtists, _topArtists) ||
          previousLoading[4] != _isLoadingTopArtists,
      !identical(previousArtistSections, _artistSections) ||
          previousLoading[5] != _isLoadingArtistSections,
    ];
    final notifiers = [
      _recommendationsNotifier,
      _genrePlaylistsNotifier,
      _artistPlaylistsNotifier,
      _topTracksNotifier,
      _topArtistsNotifier,
      _artistSectionsNotifier,
    ];
    for (var index = 0; index < changes.length; index++) {
      if (changes[index]) _notifySection(notifiers[index]);
    }
    if (changes.any((hasChanged) => hasChanged) ||
        hadLoadError != _hasLoadError) {
      notifyListeners();
    }
  }

  // Retain view selections and unchanged content across cache/server publications.
  bool _canApplyFolder(RecommendationSnapshot snapshot, String itemId) =>
      !snapshot.isFolderLoading(itemId) && !snapshot.hasFolderLoadError(itemId);

  List<T> _retainMediaList<T extends MediaItem>(
    List<T> previous,
    List<T> incoming,
  ) => hasSameMediaListContent(previous, incoming) ? previous : incoming;

  void _preserveSelectedOrder<T extends MediaItem>(
    List<T> previous,
    List<T> incoming,
  ) {
    final order = {
      for (var index = 0; index < previous.length; index++)
        (previous[index].provider, previous[index].itemId): index,
    };
    incoming.sort(
      (first, second) =>
          (order[(first.provider, first.itemId)] ?? previous.length).compareTo(
            order[(second.provider, second.itemId)] ?? previous.length,
          ),
    );
  }

  // Playback observation and history refresh.
  void _handlePlaybackChanged(PlayerState state) {
    if (_isDisposed || !_hasStartedLoading) return;
    final now = _now();
    final previous = _previousPlaybackState;
    final observedAt = _previousPlaybackObservedAt;
    if (state.connectionState != CoreConnectionState.online) {
      _previousPlaybackState = null;
      _previousPlaybackObservedAt = null;
      _currentTrackPlayedDuration = Duration.zero;
      return;
    }
    if (previous?.playbackState == PlayState.playing &&
        previous?.currentQueueItemId != null &&
        observedAt != null &&
        !now.isBefore(observedAt)) {
      _currentTrackPlayedDuration += now.difference(observedAt);
    }
    final hasChangedTrack =
        previous?.currentQueueItemId != null &&
        previous!.currentQueueItemId != state.currentQueueItemId;
    final hasEndedPlayback =
        previous?.currentQueueItemId != null &&
        previous!.playbackState != PlayState.idle &&
        state.playbackState == PlayState.idle;
    final hasRepeatedTrack =
        previous?.currentQueueItemId != null &&
        previous!.currentQueueItemId == state.currentQueueItemId &&
        previous.playbackState == PlayState.playing &&
        state.playbackState == PlayState.playing &&
        (previous.durationMillis ?? 0) > 0 &&
        (previous.positionMillis ?? 0) >= previous.durationMillis! - 1500 &&
        (state.positionMillis ?? 1501) <= 1500;
    final hasFinishedTrack =
        hasChangedTrack || hasEndedPlayback || hasRepeatedTrack;
    final hasPlayedLongEnough =
        _currentTrackPlayedDuration >= _recentListensRefreshInterval;
    _previousPlaybackState = state;
    _previousPlaybackObservedAt = now;
    if (hasFinishedTrack) _currentTrackPlayedDuration = Duration.zero;
    if (!hasFinishedTrack || !hasPlayedLongEnough) return;
    final lastReadAt = _lastRecentListensReadAt;
    if (lastReadAt != null &&
        now.difference(lastReadAt) < _recentListensRefreshInterval) {
      return;
    }
    if (_isReadingRecentListens) {
      _shouldRefreshRecentListens = true;
    } else {
      unawaited(_loadRecentListens());
    }
  }

  Future<void> _loadRecentListens() async {
    if (_isDisposed || _isReadingRecentListens) return;
    _isReadingRecentListens = true;
    _shouldRefreshRecentListens = false;
    _lastRecentListensReadAt = _now();
    _hasRecentListensLoadError = false;
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
      _hasRecentListensLoadError = true;
      if (_isLoadingRecentListens) _recentListens = const <Track>[];
    } finally {
      _isReadingRecentListens = false;
      _isLoadingRecentListens = false;
      _notifySection(_recentListensNotifier);
      notifyListeners();
      if (_shouldRefreshRecentListens) {
        unawaited(_loadRecentListens());
      }
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
      final matches = candidates.where(
        (folder) => (folder.provider, folder.itemId) == identity,
      );
      if (matches.isEmpty) continue;
      final folder = matches.first;
      if (folder.isLoading || folder.hasLoadError) continue;
      final previous = _loadedArtistSections[identity];
      final artists = folder.items
          .whereType<Artist>()
          .where(_isValidPlayable)
          .toList();
      if (previous == null) {
        _shuffle(artists);
      } else {
        _preserveSelectedOrder(previous.artists, artists);
      }
      final candidate = HomeArtistSection(
        title: prefix == 'user_similar_artists_'
            ? _normalizeSimilarArtistSectionTitle(folder.title)
            : folder.title,
        artists: artists,
      );
      final section =
          previous != null &&
              previous.title == candidate.title &&
              hasSameMediaListContent(previous.artists, candidate.artists)
          ? previous
          : candidate;
      _loadedArtistSections[identity] = section;
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
    unawaited(_playerStateSubscription?.cancel());
    unawaited(_eventSubscription?.cancel());
    unawaited(_catalogSubscription?.cancel());
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
