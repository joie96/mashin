import 'dart:async';

import 'package:flutter/foundation.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_query_scope.dart';
import '../core/core_userdata.dart';
import '../shared/detail_provider_selection.dart';

/// Loads and holds all data required by the artist detail page (metadata, albums,
/// similar artists, top tracks) and exposes the playback/favorite/description actions
/// used by the desktop and mobile artist detail pages.
class ArtistDetailController extends ChangeNotifier {
  static const int descriptionCollapsedMaxLines = 4;
  final MashinCoreBridgeApi coreBridge;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;
  final ChangeNotifier _metadataNotifier = ChangeNotifier();
  final ChangeNotifier _readStatusNotifier = ChangeNotifier();
  final ChangeNotifier _albumsNotifier = ChangeNotifier();
  final ChangeNotifier _similarArtistsNotifier = ChangeNotifier();
  final ChangeNotifier _topTracksNotifier = ChangeNotifier();

  ArtistDetailController({
    required this.coreBridge,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
  }) : sectionProvider = DetailProviderSelection(
         itemId: itemId,
         provider: providerInstanceIdOrDomain,
       ) {
    _catalogSubscription = coreBridge.catalogUpdates.listen(_onCatalogUpdate);
    _readSubscription = coreBridge.catalogReadChanges.listen(
      _onCatalogReadChange,
    );
  }

  final DetailProviderSelection sectionProvider;
  StreamSubscription<CatalogUpdate>? _catalogSubscription;
  StreamSubscription<CatalogReadChange>? _readSubscription;
  Completer<Artist?>? _firstMetadata;
  (String, String)? _sectionContext;
  CatalogReadChange? _metadataReadState;
  final Map<CatalogQueryKind, CatalogReadChange> _sectionReadStates = {};
  bool _isSelectingProvider = false;

  Artist? _artist;
  List<Album> _albums = const <Album>[];
  List<Artist> _similarArtists = const <Artist>[];
  List<Track> _topTracks = const <Track>[];

  bool _isLoadingMetadata = true;
  bool _isLoadingAlbums = true;
  bool _isLoadingSimilarArtists = true;
  bool _isLoadingTopTracks = true;
  bool _hasObservedAlbums = false;
  bool _hasObservedTopTracks = false;
  bool _hasObservedSimilarArtists = false;

  bool _isArtistFavorite = false;
  final Map<String, bool> _favoriteOverrides = <String, bool>{};
  bool _isDescriptionExpanded = false;
  bool _isControllerDisposed = false;
  bool _isPageVisible = true;
  bool _requiresReload = false;
  int _visibilityGeneration = 0;
  CoreQueryScope _queryScope = CoreQueryScope();
  CoreQueryScope _sectionsQueryScope = CoreQueryScope();
  Future<void>? _loadingFuture;

  bool get _isLoadCanceled =>
      _isControllerDisposed ||
      (CoreQueryScope.current ?? _queryScope).isCanceled;

  // Hidden generations release their subscriptions without delaying a returning page.
  Future<void> setPageVisible(bool isVisible) async {
    if (_isControllerDisposed || _isPageVisible == isVisible) return;
    _isPageVisible = isVisible;
    final generation = ++_visibilityGeneration;
    if (!isVisible) {
      _requiresReload =
          _loadingFuture != null || _isLoadingMetadata || isLoadingSections;
      _queryScope.cancel();
      _sectionsQueryScope.cancel();
      _loadingFuture = null;
      _logSection('page', 'hidden; read requests canceled');
      return;
    }
    if (_isControllerDisposed ||
        !_isPageVisible ||
        generation != _visibilityGeneration) {
      return;
    }
    _queryScope = CoreQueryScope();
    _sectionsQueryScope = CoreQueryScope();
    _isSelectingProvider = false;
    final requiresReload = _requiresReload;
    _requiresReload = false;
    _logSection('page', 'visible reload=$requiresReload');
    if (requiresReload) await loadArtist();
  }

  @override
  void notifyListeners() {
    if (!_isLoadCanceled) {
      super.notifyListeners();
    }
  }

  @override
  void dispose() {
    _isControllerDisposed = true;
    unawaited(_catalogSubscription?.cancel());
    unawaited(_readSubscription?.cancel());
    _queryScope.cancel();
    _sectionsQueryScope.cancel();
    _logSection('page', 'disposed; pending results will be ignored');
    _metadataNotifier.dispose();
    _readStatusNotifier.dispose();
    _albumsNotifier.dispose();
    _similarArtistsNotifier.dispose();
    _topTracksNotifier.dispose();
    super.dispose();
  }

  Artist? get artist => _artist;
  List<Album> get albums => _albums;
  List<Artist> get similarArtists => _similarArtists;
  List<Track> get topTracks => _topTracks;

  bool get isLoadingMetadata => _isLoadingMetadata;
  CatalogReadChange? get metadataReadState => _metadataReadState;
  CatalogReadChange? sectionReadState(CatalogQueryKind kind) =>
      _sectionReadStates[kind];
  bool get isLoadingAlbums => _isLoadingAlbums;
  bool get hasObservedAlbums => _hasObservedAlbums;
  bool get isLoadingSimilarArtists => _isLoadingSimilarArtists;
  bool get hasObservedSimilarArtists => _hasObservedSimilarArtists;
  bool get isLoadingTopTracks => _isLoadingTopTracks;
  bool get hasObservedTopTracks => _hasObservedTopTracks;
  bool get isLoadingSections =>
      _isSelectingProvider ||
      _isLoadingAlbums ||
      _isLoadingTopTracks ||
      _isLoadingSimilarArtists;

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
  int get descriptionMaxLines =>
      _isDescriptionExpanded ? 1000 : descriptionCollapsedMaxLines;
  Listenable get metadataListenable => _metadataNotifier;
  Listenable get readStatusListenable => _readStatusNotifier;
  Listenable get albumsListenable => _albumsNotifier;
  Listenable get similarArtistsListenable => _similarArtistsNotifier;
  Listenable get topTracksListenable => _topTracksNotifier;

  // Page and provider commands.
  Future<void> loadArtist() => _runLoad(_loadArtist);

  Future<void> selectProvider(ProviderMapping mapping) =>
      _queryScope.run(() => _selectProvider(mapping));

  // Catalog observations for the original header and selected section context.
  void _onCatalogUpdate(CatalogUpdate update) {
    if (_isLoadCanceled || !_isPageVisible) return;
    if (update.matches(
          CatalogQueryKind.artist,
          itemId,
          providerInstanceIdOrDomain,
        ) &&
        update.item is Artist) {
      final artist = update.item! as Artist;
      final incoming = userDataController?.withFavoriteStatus(artist) ?? artist;
      if (hasSameMediaContent(_artist, incoming) && !_isLoadingMetadata) return;
      _artist = incoming;
      _isArtistFavorite = isItemFavorite(_artist!);
      _isLoadingMetadata = false;
      if (_firstMetadata case final pending? when !pending.isCompleted) {
        pending.complete(_artist);
      }
      _metadataNotifier.notifyListeners();
    } else if (update.itemId == sectionProvider.selectedItemId &&
        update.provider == sectionProvider.selectedProvider) {
      switch (update.kind) {
        case CatalogQueryKind.artistAlbums:
          final incoming = _mapAlbums(update.albums);
          if (hasSameMediaListContent(_albums, incoming) && !_isLoadingAlbums)
            return;
          _albums = incoming;
          _hasObservedAlbums = true;
          _isLoadingAlbums = false;
          _albumsNotifier.notifyListeners();
        case CatalogQueryKind.artistTopTracks:
          final incoming = update.tracks
              .map(_withAlbumYear)
              .map(
                (track) =>
                    userDataController?.withFavoriteStatus(track) ?? track,
              )
              .toList(growable: false);
          if (hasSameMediaListContent(_topTracks, incoming) &&
              !_isLoadingTopTracks)
            return;
          _topTracks = incoming;
          _hasObservedTopTracks = true;
          _isLoadingTopTracks = false;
          _topTracksNotifier.notifyListeners();
        case CatalogQueryKind.similarArtists:
          if (update.limit != 25) return;
          final incoming = update.artists
              .map(
                (artist) =>
                    userDataController?.withFavoriteStatus(artist) ?? artist,
              )
              .toList(growable: false);
          if (hasSameMediaListContent(_similarArtists, incoming) &&
              !_isLoadingSimilarArtists)
            return;
          _similarArtists = incoming;
          _hasObservedSimilarArtists = true;
          _isLoadingSimilarArtists = false;
          _similarArtistsNotifier.notifyListeners();
        default:
          return;
      }
    } else {
      return;
    }
    notifyListeners();
  }

  void _onCatalogReadChange(CatalogReadChange change) {
    if (_isLoadCanceled || !_isPageVisible) return;
    if (change.matches(
      CatalogQueryKind.artist,
      itemId,
      providerInstanceIdOrDomain,
    )) {
      _metadataReadState = change;
    } else if (change.itemId == sectionProvider.selectedItemId &&
        change.provider == sectionProvider.selectedProvider &&
        (change.kind == CatalogQueryKind.artistAlbums ||
            change.kind == CatalogQueryKind.artistTopTracks ||
            (change.kind == CatalogQueryKind.similarArtists &&
                change.limit == 25))) {
      _sectionReadStates[change.kind] = change;
    } else {
      return;
    }
    _readStatusNotifier.notifyListeners();
    if (change.phase == CatalogLoadPhase.invalidated) {
      _invalidateServerContext();
      notifyListeners();
    }
  }

  // Metadata and section load coordination.
  void _invalidateServerContext() {
    _queryScope.cancel();
    _sectionsQueryScope.cancel();
    _requiresReload = true;
    _artist = null;
    _isArtistFavorite = false;
    _albums = const [];
    _topTracks = const [];
    _similarArtists = const [];
    _sectionContext = null;
    _hasObservedAlbums = false;
    _hasObservedTopTracks = false;
    _hasObservedSimilarArtists = false;
    _isLoadingMetadata = false;
    _isLoadingAlbums = false;
    _isLoadingTopTracks = false;
    _isLoadingSimilarArtists = false;
    _isSelectingProvider = false;
    _metadataNotifier.notifyListeners();
    _albumsNotifier.notifyListeners();
    _topTracksNotifier.notifyListeners();
    _similarArtistsNotifier.notifyListeners();
  }

  Future<void> _loadArtist() async {
    if (_isLoadCanceled) return;
    _logSection('page', 'load started');
    _isLoadingMetadata = _artist == null;
    _isLoadingAlbums = !_hasObservedAlbums;
    _isLoadingTopTracks = !_hasObservedTopTracks;
    _isLoadingSimilarArtists = !_hasObservedSimilarArtists;
    notifyListeners();

    final firstMetadata = Completer<Artist?>();
    _firstMetadata = firstMetadata;
    final metadataLoading = _loadArtistMetadata();
    try {
      final originalArtist = await Future.any<Artist?>([
        firstMetadata.future,
        metadataLoading.then((_) => _artist),
      ]);
      if (_isLoadCanceled) return;
      if (originalArtist != null)
        await sectionProvider.initialize(originalArtist);
      if (_isLoadCanceled) return;
      await _loadSections();
      await metadataLoading;
      if (_isLoadCanceled) return;
      final refreshedArtist = _artist;
      if (refreshedArtist != null &&
          !identical(originalArtist, refreshedArtist)) {
        final previousContext = (
          sectionProvider.selectedItemId,
          sectionProvider.selectedProvider,
        );
        await sectionProvider.initialize(refreshedArtist);
        if (_isLoadCanceled) return;
        _metadataNotifier.notifyListeners();
        notifyListeners();
        if (previousContext !=
            (
              sectionProvider.selectedItemId,
              sectionProvider.selectedProvider,
            )) {
          await _loadSections();
        }
      }
    } finally {
      if (identical(_firstMetadata, firstMetadata)) _firstMetadata = null;
    }
  }

  Future<void> _selectProvider(ProviderMapping mapping) async {
    if (_isLoadCanceled || isLoadingSections) return;
    _isSelectingProvider = true;
    notifyListeners();
    try {
      await sectionProvider.select(mapping);
      if (_isLoadCanceled) return;
      await _loadSections();
    } finally {
      if (!_isLoadCanceled) {
        _isSelectingProvider = false;
        notifyListeners();
      }
    }
  }

  Future<void> _loadSections() async {
    _sectionsQueryScope.cancel();
    final sectionScope = CoreQueryScope();
    _sectionsQueryScope = sectionScope;
    final context = (
      sectionProvider.selectedItemId,
      sectionProvider.selectedProvider,
    );
    final hasSameContext = context == _sectionContext;
    if (!hasSameContext) {
      _albums = const <Album>[];
      _topTracks = const <Track>[];
      _similarArtists = const <Artist>[];
      _sectionReadStates.clear();
      _hasObservedAlbums = false;
      _hasObservedTopTracks = false;
      _hasObservedSimilarArtists = false;
      _sectionContext = context;
    }
    _isLoadingAlbums = !_hasObservedAlbums;
    _isLoadingTopTracks = !_hasObservedTopTracks;
    _isLoadingSimilarArtists = !_hasObservedSimilarArtists;
    _albumsNotifier.notifyListeners();
    _topTracksNotifier.notifyListeners();
    _similarArtistsNotifier.notifyListeners();
    notifyListeners();
    await sectionScope.run(
      () =>
          Future.wait([_loadAlbums(), _loadTopTracks(), _loadSimilarArtists()]),
    );
  }

  Future<void> _loadArtistMetadata() async {
    final timer = Stopwatch()..start();
    var hasNewMetadata = false;
    _logSection('metadata', 'started');
    _isLoadingMetadata = _artist == null;
    notifyListeners();
    try {
      final Artist loaded = await coreBridge.getArtist(
        itemId,
        providerInstanceIdOrDomain,
      );
      if (_isLoadCanceled) return;
      final incoming = userDataController?.withFavoriteStatus(loaded) ?? loaded;
      hasNewMetadata = !hasSameMediaContent(_artist, incoming);
      if (hasNewMetadata) _artist = incoming;
      final Artist? artist = _artist;
      _isArtistFavorite = artist != null && isItemFavorite(artist);
      if (_isLoadCanceled) return;
      _logSection('metadata', 'loaded elapsed_ms=${timer.elapsedMilliseconds}');
    } catch (error) {
      if (_isLoadCanceled) return;
      _logSection(
        'metadata',
        'failed elapsed_ms=${timer.elapsedMilliseconds} error=$error',
        level: LogLevelPreference.warn,
      );
    } finally {
      if (!_isLoadCanceled) {
        final shouldNotify = _isLoadingMetadata || hasNewMetadata;
        _isLoadingMetadata = false;
        if (shouldNotify) {
          _metadataNotifier.notifyListeners();
        }
        notifyListeners();
      }
    }
  }

  Future<void> _loadAlbums() async {
    final timer = Stopwatch()..start();
    var hasNewAlbums = false;
    _logSection('albums', 'started');
    _isLoadingAlbums = !_hasObservedAlbums;
    notifyListeners();
    try {
      final List<Album> albums = await coreBridge.getArtistAlbums(
        sectionProvider.selectedItemId,
        sectionProvider.selectedProvider,
      );
      if (_isLoadCanceled) return;
      final incoming = _mapAlbums(albums);
      hasNewAlbums = !hasSameMediaListContent(_albums, incoming);
      if (hasNewAlbums) _albums = incoming;
      _hasObservedAlbums = true;
      _logSection(
        'albums',
        'loaded count=${_albums.length} elapsed_ms=${timer.elapsedMilliseconds}',
      );
    } catch (error) {
      if (_isLoadCanceled) return;
      _logSection(
        'albums',
        'failed elapsed_ms=${timer.elapsedMilliseconds} error=$error',
        level: LogLevelPreference.warn,
      );
    } finally {
      if (!_isLoadCanceled) {
        final shouldNotify = _isLoadingAlbums || hasNewAlbums;
        _isLoadingAlbums = false;
        if (shouldNotify) _albumsNotifier.notifyListeners();
        notifyListeners();
      }
    }
  }

  Future<void> _loadSimilarArtists() async {
    final timer = Stopwatch()..start();
    var hasNewArtists = false;
    _logSection('similar_artists', 'started');
    _isLoadingSimilarArtists = !_hasObservedSimilarArtists;
    notifyListeners();
    try {
      final List<Artist> artists = await coreBridge.getSimilarArtists(
        sectionProvider.selectedItemId,
        sectionProvider.selectedProvider,
        limit: 25,
      );
      if (_isLoadCanceled) return;
      final incoming = List<Artist>.unmodifiable(
        artists
            .map(
              (Artist artist) =>
                  userDataController?.withFavoriteStatus(artist) ?? artist,
            )
            .toList(growable: false),
      );
      hasNewArtists = !hasSameMediaListContent(_similarArtists, incoming);
      if (hasNewArtists) _similarArtists = incoming;
      _hasObservedSimilarArtists = true;
      _logSection(
        'similar_artists',
        'loaded count=${_similarArtists.length} elapsed_ms=${timer.elapsedMilliseconds}',
      );
    } catch (error) {
      if (_isLoadCanceled) return;
      _logSection(
        'similar_artists',
        'failed elapsed_ms=${timer.elapsedMilliseconds} error=$error',
        level: LogLevelPreference.warn,
      );
    } finally {
      if (!_isLoadCanceled) {
        final shouldNotify = _isLoadingSimilarArtists || hasNewArtists;
        _isLoadingSimilarArtists = false;
        if (shouldNotify) _similarArtistsNotifier.notifyListeners();
        notifyListeners();
      }
    }
  }

  Future<void> _loadTopTracks() async {
    final timer = Stopwatch()..start();
    var hasNewTracks = false;
    _logSection('top_tracks', 'started');
    _isLoadingTopTracks = !_hasObservedTopTracks;
    notifyListeners();
    try {
      final List<Track> tracks = await coreBridge.getArtistTopTracks(
        sectionProvider.selectedItemId,
        sectionProvider.selectedProvider,
      );
      if (_isLoadCanceled) return;
      final incoming = List<Track>.unmodifiable(
        tracks
            .map(_withAlbumYear)
            .map(
              (Track track) =>
                  userDataController?.withFavoriteStatus(track) ?? track,
            )
            .toList(growable: false),
      );
      hasNewTracks = !hasSameMediaListContent(_topTracks, incoming);
      if (hasNewTracks) _topTracks = incoming;
      _hasObservedTopTracks = true;
      _logSection(
        'top_tracks',
        'loaded count=${_topTracks.length} elapsed_ms=${timer.elapsedMilliseconds}',
      );
    } catch (error) {
      if (_isLoadCanceled) return;
      _logSection(
        'top_tracks',
        'failed elapsed_ms=${timer.elapsedMilliseconds} error=$error',
        level: LogLevelPreference.warn,
      );
    } finally {
      if (!_isLoadCanceled) {
        final shouldNotify = _isLoadingTopTracks || hasNewTracks;
        _isLoadingTopTracks = false;
        if (shouldNotify) _topTracksNotifier.notifyListeners();
        notifyListeners();
      }
    }
  }

  // Deduplicates page metadata loads without blocking an independent provider selection.
  Future<void> _runLoad(Future<void> Function() load) {
    if (_isLoadCanceled) return Future<void>.value();
    if (_loadingFuture case final pending?) return pending;
    late final Future<void> loading;
    loading = _queryScope.run(load).whenComplete(() {
      if (identical(_loadingFuture, loading)) _loadingFuture = null;
    });
    return _loadingFuture = loading;
  }

  // Diagnostics identify concurrent page instances without logging response payloads.
  void _logSection(
    String section,
    String message, {
    LogLevelPreference level = LogLevelPreference.debug,
  }) {
    coreBridge.log(
      level,
      'detail.artist',
      'page=${identityHashCode(this)} item=$itemId provider=$providerInstanceIdOrDomain selected_provider=${sectionProvider.selectedProvider} section=$section $message',
    );
  }

  // Bounded display projections never change the persisted server result order.
  List<Album> _mapAlbums(List<Album> albums) {
    final visible = albums
        .map((album) => userDataController?.withFavoriteStatus(album) ?? album)
        .toList();
    visible.sort((left, right) => (right.year ?? 0).compareTo(left.year ?? 0));
    return List<Album>.unmodifiable(visible);
  }

  Track _withAlbumYear(Track track) {
    final Album? trackAlbum = track.album;
    if (trackAlbum == null || trackAlbum.year != null || _albums.isEmpty) {
      return track;
    }

    final Album? matchingAlbum = _albums.cast<Album?>().firstWhere(
      (Album? album) =>
          album?.provider == trackAlbum.provider &&
          album?.itemId == trackAlbum.itemId,
      orElse: () => null,
    );
    if (matchingAlbum?.year == null) {
      return track;
    }

    final Album albumWithYear = Album(
      itemId: trackAlbum.itemId,
      provider: trackAlbum.provider,
      name: trackAlbum.name,
      version: trackAlbum.version,
      artists: trackAlbum.artists,
      sortName: trackAlbum.sortName,
      uri: trackAlbum.uri,
      externalIds: trackAlbum.externalIds,
      isPlayable: trackAlbum.isPlayable,
      available: trackAlbum.available,
      year: matchingAlbum!.year,
      albumType: trackAlbum.albumType,
      providerMappings: trackAlbum.providerMappings,
      metadata: trackAlbum.metadata,
      favorite: trackAlbum.favorite,
      position: trackAlbum.position,
      dateAdded: trackAlbum.dateAdded,
    );
    return Track(
      itemId: track.itemId,
      provider: track.provider,
      name: track.name,
      version: track.version,
      duration: track.duration,
      artists: track.artists,
      sortName: track.sortName,
      uri: track.uri,
      externalIds: track.externalIds,
      isPlayable: track.isPlayable,
      available: track.available,
      album: albumWithYear,
      image: track.image,
      providerMappings: track.providerMappings,
      metadata: track.metadata,
      favorite: track.favorite,
      position: track.position,
      dateAdded: track.dateAdded,
      discNumber: track.discNumber,
      trackNumber: track.trackNumber,
    );
  }

  // Favorites
  String _favoriteKey(MediaItem item) =>
      '${item.mediaType.name}:${item.provider}:${item.itemId}';

  bool isItemFavorite(MediaItem item) {
    return _favoriteOverrides[_favoriteKey(item)] ?? item.favorite;
  }

  Future<void> toggleItemFavorite(MediaItem item) async {
    final String key = _favoriteKey(item);
    final bool nextFavorite = !isItemFavorite(item);
    _favoriteOverrides[key] = nextFavorite;
    _notifyContentSections();

    try {
      final UserData? controller = userDataController;
      if (controller == null) {
        _favoriteOverrides.remove(key);
        _notifyContentSections();
        return;
      }
      await controller.setFavorite(<MediaItem>[item], nextFavorite);
    } catch (_) {
      // Revert the optimistic update on failure.
      _favoriteOverrides[key] = !nextFavorite;
      _notifyContentSections();
    }
  }

  Future<void> toggleArtistFavorite() async {
    final Artist? current = _artist;
    if (current == null) {
      return;
    }

    final bool nextFavorite = !_isArtistFavorite;
    _isArtistFavorite = nextFavorite;
    _metadataNotifier.notifyListeners();

    try {
      final UserData? controller = userDataController;
      if (controller == null) {
        _isArtistFavorite = !nextFavorite;
        _metadataNotifier.notifyListeners();
        return;
      }
      await controller.setFavorite(<MediaItem>[current], nextFavorite);
    } catch (_) {
      _isArtistFavorite = !nextFavorite;
      _metadataNotifier.notifyListeners();
    }
  }

  // Description
  void toggleDescription() {
    _isDescriptionExpanded = !_isDescriptionExpanded;
    _metadataNotifier.notifyListeners();
    notifyListeners();
  }

  void _notifyContentSections() {
    _albumsNotifier.notifyListeners();
    _similarArtistsNotifier.notifyListeners();
    _topTracksNotifier.notifyListeners();
  }

  // Playback
  List<MediaItem> _playableTracks(List<Track> tracks) {
    return tracks
        .where(
          (Track track) =>
              track.itemId.trim().isNotEmpty &&
              track.provider.trim().isNotEmpty,
        )
        .toList(growable: false);
  }

  Future<void> playArtist() async {
    final List<MediaItem> items = _playableTracks(_topTracks);
    if (items.isEmpty) {
      return;
    }
    try {
      await coreBridge.queuePlayMedia(
        items,
        providerInstance: sectionProvider.selectedProvider,
      );
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
      await coreBridge.queuePlayMedia(
        items,
        shuffle: true,
        providerInstance: sectionProvider.selectedProvider,
      );
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }

  Future<void> startArtistRadio() async {
    final Artist? currentArtist = _artist;
    if (currentArtist == null) {
      return;
    }

    try {
      await coreBridge.startRadio(<MediaItem>[currentArtist]);
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }

  /// Starts radio for an arbitrary media item (similar artist, album, ...).
  Future<void> startItemRadio(MediaItem item) async {
    if (item.itemId.trim().isEmpty || item.provider.trim().isEmpty) {
      return;
    }

    try {
      await coreBridge.startRadio(<MediaItem>[item]);
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }

  /// Starts radio from a track.
  Future<void> startTrackRadio(Track track) async {
    if (track.itemId.trim().isEmpty || track.provider.trim().isEmpty) {
      return;
    }

    try {
      await coreBridge.startRadio(<MediaItem>[track]);
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }
}
