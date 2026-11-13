import 'dart:async';

import 'package:flutter/foundation.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_query_scope.dart';
import '../core/core_userdata.dart';
import '../shared/detail_provider_selection.dart';

class AlbumDetailController extends ChangeNotifier {
  AlbumDetailController({
    required this.coreBridge,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
  }) : sectionProvider = DetailProviderSelection(
         itemId: itemId,
         provider: providerInstanceIdOrDomain,
       ) {
    userDataController?.favoritesChanges.addListener(_onFavoritesChanged);
    _catalogSubscription = coreBridge.catalogUpdates.listen(_onCatalogUpdate);
    _cacheSubscription = coreBridge.mediaReadInvalidations.listen(
      _onMediaReadInvalidated,
    );
    _readSubscription = coreBridge.catalogReadChanges.listen(
      _onCatalogReadChange,
    );
  }

  final DetailProviderSelection sectionProvider;
  StreamSubscription<CatalogUpdate>? _catalogSubscription;
  StreamSubscription<void>? _cacheSubscription;
  StreamSubscription<CatalogReadChange>? _readSubscription;
  CatalogReadChange? _metadataReadState;
  CatalogReadChange? _tracksReadState;
  Completer<Album?>? _firstMetadata;
  (String, String)? _sectionContext;
  bool _hasObservedTracks = false;
  bool _isSelectingProvider = false;

  final MashinCoreBridgeApi coreBridge;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;
  final ChangeNotifier _metadataNotifier = ChangeNotifier();
  final ChangeNotifier _readStatusNotifier = ChangeNotifier();
  final ChangeNotifier _tracksNotifier = ChangeNotifier();
  final ChangeNotifier _relatedNotifier = ChangeNotifier();

  Album? _album;
  List<Track> _tracks = const <Track>[];
  List<Album> _otherAlbums = const <Album>[];
  List<Artist> _similarArtists = const <Artist>[];

  bool _isLoadingMetadata = true;
  bool _isLoadingTracks = true;
  bool _isLoadingOtherAlbums = true;
  bool _isLoadingSimilarArtists = true;
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
      if (_firstMetadata case final pending? when !pending.isCompleted) {
        pending.complete(null);
      }
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
    final requiresReload = _requiresReload;
    _requiresReload = false;
    _logSection('page', 'visible reload=$requiresReload');
    if (requiresReload) await loadAlbum();
  }

  void _onMediaReadInvalidated(void _) {
    if (_isControllerDisposed) return;
    _requiresReload = true;
    _queryScope.cancel();
    _sectionsQueryScope.cancel();
    if (_firstMetadata case final pending? when !pending.isCompleted) {
      pending.complete(null);
    }
    _loadingFuture = null;
    if (!_isPageVisible) return;
    _queryScope = CoreQueryScope();
    _requiresReload = false;
    unawaited(loadAlbum());
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
    userDataController?.favoritesChanges.removeListener(_onFavoritesChanged);
    unawaited(_catalogSubscription?.cancel());
    unawaited(_cacheSubscription?.cancel());
    unawaited(_readSubscription?.cancel());
    _queryScope.cancel();
    _sectionsQueryScope.cancel();
    if (_firstMetadata case final pending? when !pending.isCompleted) {
      pending.complete(null);
    }
    _logSection('page', 'disposed; pending results will be ignored');
    _metadataNotifier.dispose();
    _readStatusNotifier.dispose();
    _tracksNotifier.dispose();
    _relatedNotifier.dispose();
    super.dispose();
  }

  // Catalog observations keep the original album separate from section selections.
  void _onCatalogUpdate(CatalogUpdate update) {
    if (_isLoadCanceled || !_isPageVisible) return;
    if (update.matches(
          CatalogQueryKind.album,
          itemId,
          providerInstanceIdOrDomain,
        ) &&
        update.item is Album) {
      final album = update.item! as Album;
      final incoming = userDataController?.withFavoriteStatus(album) ?? album;
      if (hasSameMediaContent(_album, incoming) && !_isLoadingMetadata) return;
      _album = incoming;
      _isLoadingMetadata = false;
      if (_firstMetadata case final pending? when !pending.isCompleted) {
        pending.complete(_album);
      }
      _metadataNotifier.notifyListeners();
    } else if (update.matches(
      CatalogQueryKind.albumTracks,
      sectionProvider.selectedItemId,
      sectionProvider.selectedProvider,
    )) {
      final incoming = List<Track>.unmodifiable(
        update.tracks.map(
          (track) => userDataController?.withFavoriteStatus(track) ?? track,
        ),
      );
      if (hasSameMediaListContent(_tracks, incoming) && !_isLoadingTracks)
        return;
      _tracks = incoming;
      _hasObservedTracks = true;
      _isLoadingTracks = false;
      _tracksNotifier.notifyListeners();
    } else {
      return;
    }
    notifyListeners();
  }

  void _onCatalogReadChange(CatalogReadChange change) {
    if (_isLoadCanceled || !_isPageVisible) return;
    if (change.matches(
      CatalogQueryKind.album,
      itemId,
      providerInstanceIdOrDomain,
    )) {
      _metadataReadState = change;
    } else if (change.matches(
      CatalogQueryKind.albumTracks,
      sectionProvider.selectedItemId,
      sectionProvider.selectedProvider,
    )) {
      _tracksReadState = change;
    } else {
      return;
    }
    _readStatusNotifier.notifyListeners();
    if (change.phase == CatalogLoadPhase.invalidated) {
      _invalidateServerContext();
      return;
    }
  }

  // Retire old reads before clearing server-scoped view data and notifying the visible page.
  void _invalidateServerContext() {
    _queryScope.cancel();
    _sectionsQueryScope.cancel();
    if (_firstMetadata case final pending? when !pending.isCompleted) {
      pending.complete(null);
    }
    _requiresReload = true;
    _album = null;
    _tracks = const [];
    _otherAlbums = const [];
    _similarArtists = const [];
    _sectionContext = null;
    _hasObservedTracks = false;
    _isLoadingMetadata = false;
    _isLoadingTracks = false;
    _isLoadingOtherAlbums = false;
    _isLoadingSimilarArtists = false;
    _isSelectingProvider = false;
    _metadataNotifier.notifyListeners();
    _tracksNotifier.notifyListeners();
    _relatedNotifier.notifyListeners();
    super.notifyListeners();
  }

  Album? get album => _album;
  List<Track> get tracks => _tracks;
  List<Album> get otherAlbums => _otherAlbums;
  List<Artist> get similarArtists => _similarArtists;

  bool get isLoadingMetadata => _isLoadingMetadata;
  CatalogReadChange? get metadataReadState => _metadataReadState;
  CatalogReadChange? get tracksReadState => _tracksReadState;
  bool get isLoadingTracks => _isLoadingTracks;
  bool get isLoadingOtherAlbums => _isLoadingOtherAlbums;
  bool get isLoadingSimilarArtists => _isLoadingSimilarArtists;
  bool get isLoadingSections =>
      _isSelectingProvider ||
      _isLoadingTracks ||
      _isLoadingOtherAlbums ||
      _isLoadingSimilarArtists;
  bool get isAlbumFavorite => _album != null && isItemFavorite(_album!);
  bool get isDescriptionExpanded => _isDescriptionExpanded;
  Listenable get metadataListenable => _metadataNotifier;
  Listenable get readStatusListenable => _readStatusNotifier;
  Listenable get tracksListenable => _tracksNotifier;
  Listenable get relatedListenable => _relatedNotifier;
  bool get hasDescription => description != null;
  int get descriptionMaxLines => _isDescriptionExpanded ? 1000 : 4;

  String get albumName {
    final String name = _album?.name.trim() ?? '';
    return name.isEmpty ? 'Unbekanntes Album' : name;
  }

  String get artistName {
    final String albumArtist = _album?.artists.firstOrNull?.name.trim() ?? '';
    if (albumArtist.isNotEmpty) {
      return albumArtist;
    }
    final String trackArtist =
        _tracks.firstOrNull?.artists.firstOrNull?.name.trim() ?? '';
    return trackArtist.isEmpty ? 'Unbekannte:r Interpret:in' : trackArtist;
  }

  String get yearText {
    final int? year = _album?.year;
    return year == null || year <= 0 ? '' : '$year';
  }

  String? get description {
    final String value = _album?.metadata?.description?.trim() ?? '';
    return value.isEmpty ? null : value.replaceAll(RegExp(r'\n\s*\n+'), '\n');
  }

  Future<void> loadAlbum() => _runLoad(_loadAlbum);

  Future<void> _loadAlbum() async {
    if (_isLoadCanceled) return;
    _logSection('page', 'load started');
    _isLoadingMetadata = _album == null;
    _isLoadingTracks = !_hasObservedTracks;
    _isLoadingOtherAlbums = true;
    _isLoadingSimilarArtists = true;
    notifyListeners();

    final firstMetadata = Completer<Album?>();
    _firstMetadata = firstMetadata;
    if (_album != null) firstMetadata.complete(_album);
    final metadataLoading = _loadMetadata();
    try {
      final originalAlbum = await Future.any<Album?>([
        firstMetadata.future,
        metadataLoading.then((_) => _album),
      ]);
      if (_isLoadCanceled) return;
      if (originalAlbum != null)
        await sectionProvider.initialize(originalAlbum);
      if (_isLoadCanceled) return;
      await _loadSections();
      await metadataLoading;
      if (_isLoadCanceled) return;
      final refreshedAlbum = _album;
      if (refreshedAlbum != null && !identical(originalAlbum, refreshedAlbum)) {
        final previousContext = (
          sectionProvider.selectedItemId,
          sectionProvider.selectedProvider,
        );
        await sectionProvider.initialize(refreshedAlbum);
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

  // Provider changes never replace the original album or its metadata.
  Future<void> selectProvider(ProviderMapping mapping) =>
      _queryScope.run(() => _selectProvider(mapping));

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

  // Independent album reads must not gate track-dependent artist queries.
  Future<void> _loadSections() async {
    _sectionsQueryScope.cancel();
    final sectionScope = CoreQueryScope();
    _sectionsQueryScope = sectionScope;
    final context = (
      sectionProvider.selectedItemId,
      sectionProvider.selectedProvider,
    );
    if (context != _sectionContext) {
      _tracks = const <Track>[];
      _otherAlbums = const <Album>[];
      _similarArtists = const <Artist>[];
      _tracksReadState = null;
      _hasObservedTracks = false;
      _sectionContext = context;
    }
    _isLoadingTracks = !_hasObservedTracks;
    _isLoadingOtherAlbums = true;
    _isLoadingSimilarArtists = true;
    _tracksNotifier.notifyListeners();
    _relatedNotifier.notifyListeners();
    notifyListeners();
    await sectionScope.run(() async {
      await Future.wait(<Future<void>>[
        _loadTracksAndSimilarArtists(),
        _loadOtherAlbums(),
      ]);
    });
  }

  // Each observed seed replaces its own branch without waiting for retired reads.
  Future<void> _loadTracksAndSimilarArtists() async {
    final scope = CoreQueryScope.current ?? _sectionsQueryScope;
    CoreQueryScope? branch;
    (String?, String?)? previousSeed;
    final reads = <Future<void>>[];
    var isObserving = true;
    void observeTracks() {
      if (!isObserving || scope.isCanceled || !_hasObservedTracks) return;
      final seed = _tracks.firstOrNull;
      final identity = (seed?.provider, seed?.itemId);
      if (identity == previousSeed) return;
      previousSeed = identity;
      branch?.cancel();
      final nextBranch = scope.createChild();
      branch = nextBranch;
      reads.add(
        nextBranch.run(_loadSimilarArtists).whenComplete(nextBranch.cancel),
      );
    }

    void onTracksChanged() => scheduleMicrotask(observeTracks);
    _tracksNotifier.addListener(onTracksChanged);
    try {
      observeTracks();
      await _loadTracks();
      if (scope.isCanceled) return;
      if (_hasObservedTracks) {
        observeTracks();
      } else {
        await _loadSimilarArtists();
      }
      await Future.wait(reads);
    } finally {
      isObserving = false;
      _tracksNotifier.removeListener(onTracksChanged);
      branch?.cancel();
    }
  }

  Future<void> _loadMetadata() async {
    final timer = Stopwatch()..start();
    var hasNewMetadata = false;
    _logSection('metadata', 'started');
    try {
      final Album loaded = await coreBridge.getAlbum(
        itemId,
        providerInstanceIdOrDomain,
      );
      if (_isLoadCanceled) return;
      final incoming = userDataController?.withFavoriteStatus(loaded) ?? loaded;
      hasNewMetadata = !hasSameMediaContent(_album, incoming);
      if (hasNewMetadata) _album = incoming;
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
        if (shouldNotify) _metadataNotifier.notifyListeners();
        notifyListeners();
      }
    }
  }

  Future<void> _loadTracks() async {
    final timer = Stopwatch()..start();
    var hasNewTracks = false;
    _logSection('tracks', 'started');
    try {
      final List<Track> loaded = await coreBridge.getAlbumTracks(
        sectionProvider.selectedItemId,
        sectionProvider.selectedProvider,
      );
      if (_isLoadCanceled) return;
      final incoming = List<Track>.unmodifiable(
        loaded
            .map(
              (track) => userDataController?.withFavoriteStatus(track) ?? track,
            )
            .toList(growable: false),
      );
      hasNewTracks = !hasSameMediaListContent(_tracks, incoming);
      if (hasNewTracks) _tracks = incoming;
      _hasObservedTracks = true;
      _logSection(
        'tracks',
        'loaded count=${_tracks.length} elapsed_ms=${timer.elapsedMilliseconds}',
      );
    } catch (error) {
      if (_isLoadCanceled) return;
      _logSection(
        'tracks',
        'failed elapsed_ms=${timer.elapsedMilliseconds} error=$error',
        level: LogLevelPreference.warn,
      );
    } finally {
      if (!_isLoadCanceled) {
        final shouldNotify = _isLoadingTracks || hasNewTracks;
        _isLoadingTracks = false;
        if (shouldNotify) {
          _tracksNotifier.notifyListeners();
        }
        notifyListeners();
      }
    }
  }

  Future<void> _loadOtherAlbums() async {
    final scope = CoreQueryScope.current ?? _sectionsQueryScope;
    bool isCurrent() =>
        !_isControllerDisposed && _isPageVisible && !scope.isCanceled;
    final artist = _album?.artists.firstOrNull;
    (String, String)? resolve(Artist value) {
      final selected = sectionProvider.selectedProvider;
      if (selected == 'library' || value.provider == selected) {
        return (value.provider, value.itemId);
      }
      final mapping = value.providerMappings
          .where(
            (mapping) =>
                mapping.available && mapping.providerInstance == selected,
          )
          .firstOrNull;
      return mapping == null
          ? null
          : (mapping.providerInstance, mapping.itemId);
    }

    Future<void> loadAlbums(
      Artist value,
      bool Function() isValueCurrent,
    ) async {
      final key = resolve(value);
      if (key == null) return;
      await _observeCatalogValues<List<Album>>(
        CatalogQueryKind.artistAlbums,
        Artist(provider: key.$1, itemId: key.$2, name: value.name),
        isCurrent: isValueCurrent,
        select: (update) => update.albums,
        hasSameValue: hasSameMediaListContent,
        load: () => coreBridge.getArtistAlbums(key.$2, key.$1),
        onValue: (albums, isAlbumCurrent) async {
          if (!isAlbumCurrent()) return;
          final incoming = _projectOtherAlbums(albums);
          if (hasSameMediaListContent(_otherAlbums, incoming) &&
              !_isLoadingOtherAlbums) {
            return;
          }
          _otherAlbums = incoming;
          _isLoadingOtherAlbums = false;
          _relatedNotifier.notifyListeners();
          notifyListeners();
        },
      );
    }

    try {
      if (artist == null) return;
      if (sectionProvider.selectedProvider == 'library' ||
          artist.provider == sectionProvider.selectedProvider) {
        await loadAlbums(artist, isCurrent);
      } else {
        await _observeCatalogValues<Artist>(
          CatalogQueryKind.artist,
          artist,
          initialValue: resolve(artist) == null ? null : artist,
          isCurrent: isCurrent,
          select: (update) =>
              update.item is Artist ? update.item! as Artist : null,
          hasSameValue: (left, right) => resolve(left) == resolve(right),
          load: () => coreBridge.getArtist(artist.itemId, artist.provider),
          onValue: loadAlbums,
        );
      }
    } finally {
      if (isCurrent() && _isLoadingOtherAlbums) {
        _isLoadingOtherAlbums = false;
        _relatedNotifier.notifyListeners();
        notifyListeners();
      }
    }
  }

  Future<void> _loadSimilarArtists() async {
    final timer = Stopwatch()..start();
    final scope = CoreQueryScope.current ?? _sectionsQueryScope;
    final Track? seed = _tracks.firstOrNull;
    bool isCurrent() =>
        !_isControllerDisposed &&
        _isPageVisible &&
        !scope.isCanceled &&
        (seed?.provider, seed?.itemId) ==
            (_tracks.firstOrNull?.provider, _tracks.firstOrNull?.itemId);
    _logSection('similar_artists', 'started');
    if (seed == null) {
      _similarArtists = const <Artist>[];
      _isLoadingSimilarArtists = false;
      _logSection('similar_artists', 'skipped reason=no_tracks');
      _relatedNotifier.notifyListeners();
      notifyListeners();
      return;
    }

    try {
      await _observeSimilarArtistTracks(
        seed,
        isCurrent: isCurrent,
        shouldLookupVersions: true,
      );
      if (!isCurrent()) return;
      _logSection(
        'similar_artists',
        'loaded count=${_similarArtists.length} elapsed_ms=${timer.elapsedMilliseconds}',
      );
    } catch (error) {
      if (!isCurrent()) return;
      _logSection(
        'similar_artists',
        'failed elapsed_ms=${timer.elapsedMilliseconds} error=$error',
        level: LogLevelPreference.warn,
      );
    } finally {
      if (isCurrent() && _isLoadingSimilarArtists) {
        _isLoadingSimilarArtists = false;
        _relatedNotifier.notifyListeners();
        notifyListeners();
      }
    }
  }

  // Every observed value owns its follow-ups; newer values retire older projections.
  Future<void> _observeSimilarArtistTracks(
    Track seed, {
    required bool Function() isCurrent,
    required bool shouldLookupVersions,
  }) => _observeRelatedTracks(
    CatalogQueryKind.similarTracks,
    seed,
    isCurrent: isCurrent,
    load: () => coreBridge.getSimilarTracks(
      seed.itemId,
      seed.provider,
      limit: 50,
      allowLookup: true,
    ),
    onTracks: (tracks, isValueCurrent) async {
      if (tracks.isNotEmpty || !shouldLookupVersions) {
        await _projectSimilarArtists(tracks, isValueCurrent);
        return;
      }
      await _observeRelatedTracks(
        CatalogQueryKind.trackVersions,
        seed,
        isCurrent: isValueCurrent,
        load: () => coreBridge.getTrackVersions(seed.itemId, seed.provider),
        onTracks: (versions, isVersionCurrent) async {
          if (versions.isEmpty) {
            if (isVersionCurrent()) _publishSimilarArtists(const []);
            return;
          }
          final fallback = versions.firstWhere(
            (version) => version.provider != seed.provider,
            orElse: () => versions.first,
          );
          await _observeSimilarArtistTracks(
            fallback,
            isCurrent: isVersionCurrent,
            shouldLookupVersions: false,
          );
        },
      );
    },
  );

  Future<void> _observeRelatedTracks(
    CatalogQueryKind kind,
    Track seed, {
    required bool Function() isCurrent,
    required Future<List<Track>> Function() load,
    required Future<void> Function(List<Track>, bool Function()) onTracks,
  }) => _observeCatalogValues<List<Track>>(
    kind,
    seed,
    isCurrent: isCurrent,
    load: load,
    hasSameValue: hasSameMediaListContent,
    select: (update) =>
        kind == CatalogQueryKind.similarTracks &&
            (update.limit != 50 || update.allowLookup != true)
        ? null
        : update.tracks,
    onValue: onTracks,
  );

  Future<void> _observeCatalogValues<T>(
    CatalogQueryKind kind,
    MediaItem seed, {
    T? initialValue,
    required bool Function() isCurrent,
    required T? Function(CatalogUpdate) select,
    required bool Function(T, T) hasSameValue,
    required Future<T> Function() load,
    required Future<void> Function(T, bool Function()) onValue,
  }) async {
    final scope = CoreQueryScope.current ?? _sectionsQueryScope;
    CoreQueryScope? valueScope;
    T? previous;
    var revision = 0;
    final projections = <Future<void>>[];
    void accept(T value) {
      if (!isCurrent() ||
          previous != null && hasSameValue(previous as T, value)) {
        return;
      }
      previous = value;
      final valueRevision = ++revision;
      valueScope?.cancel();
      final branch = scope.createChild();
      valueScope = branch;
      bool isValueCurrent() =>
          isCurrent() && !branch.isCanceled && valueRevision == revision;
      projections.add(
        branch
            .run(() async {
              try {
                if (isValueCurrent()) await onValue(value, isValueCurrent);
              } catch (error) {
                if (isValueCurrent()) {
                  _logSection(
                    'similar_artists',
                    'projection failed error=$error',
                    level: LogLevelPreference.warn,
                  );
                }
              }
            })
            .whenComplete(branch.cancel),
      );
    }

    final subscription = coreBridge.catalogUpdates.listen((update) {
      if (update.matches(kind, seed.itemId, seed.provider)) {
        final value = select(update);
        if (value != null) accept(value);
      }
    });
    final readSubscription = coreBridge.catalogReadChanges.listen((change) {
      if (isCurrent() &&
          change.matches(kind, seed.itemId, seed.provider) &&
          change.phase == CatalogLoadPhase.invalidated) {
        _invalidateServerContext();
      }
    });
    try {
      if (initialValue != null) accept(initialValue);
      accept(await load());
    } catch (error) {
      if (isCurrent()) {
        _logSection(
          'similar_artists',
          'read=${kind.name} failed error=$error',
          level: LogLevelPreference.warn,
        );
      }
    } finally {
      await subscription.cancel();
      await readSubscription.cancel();
      await Future.wait(projections);
      valueScope?.cancel();
    }
  }

  // Embedded artists are immediately usable; enrichment never gates their first publication.
  Future<void> _projectSimilarArtists(
    List<Track> tracks,
    bool Function() isCurrent,
  ) async {
    final albumArtistId = _album?.artists.firstOrNull?.itemId;
    final unique = <String, Artist>{};
    for (final track in tracks) {
      for (final artist in track.artists) {
        if (artist.itemId != albumArtistId) {
          unique.putIfAbsent(artist.itemId, () => artist);
        }
      }
    }
    final artists = unique.values.take(15).toList(growable: false);
    if (!isCurrent()) return;
    _publishSimilarArtists(artists);
    await Future.wait(
      List.generate(artists.length, (index) async {
        if (!isCurrent()) return;
        final artist = artists[index];
        await _observeCatalogValues<Artist>(
          CatalogQueryKind.artist,
          artist,
          isCurrent: isCurrent,
          select: (update) =>
              update.item is Artist ? update.item! as Artist : null,
          hasSameValue: hasSameMediaContent,
          load: () => coreBridge.getArtist(artist.itemId, artist.provider),
          onValue: (enriched, isArtistCurrent) async {
            if (!isArtistCurrent()) return;
            artists[index] = enriched;
            _publishSimilarArtists(artists);
          },
        );
      }),
    );
  }

  void _publishSimilarArtists(List<Artist> artists) {
    final incoming = List<Artist>.unmodifiable(
      artists.map(
        (artist) => userDataController?.withFavoriteStatus(artist) ?? artist,
      ),
    );
    if (hasSameMediaListContent(_similarArtists, incoming) &&
        !_isLoadingSimilarArtists) {
      return;
    }
    _similarArtists = incoming;
    _isLoadingSimilarArtists = false;
    _relatedNotifier.notifyListeners();
    notifyListeners();
  }

  // A single load generation owns all sequential and parallel section requests.
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
      'detail.album',
      'page=${identityHashCode(this)} item=$itemId provider=$providerInstanceIdOrDomain selected_provider=${sectionProvider.selectedProvider} section=$section $message',
    );
  }

  // Favorite changes share one session-owned status across all open views.
  void _onFavoritesChanged() {
    if (_isControllerDisposed) return;
    _metadataNotifier.notifyListeners();
    _tracksNotifier.notifyListeners();
    _relatedNotifier.notifyListeners();
    notifyListeners();
  }

  bool isItemFavorite(MediaItem item) =>
      userDataController?.isFavorite(item) ?? item.favorite;

  Future<void> toggleItemFavorite(MediaItem item) async {
    final bool next = !isItemFavorite(item);
    try {
      await userDataController?.setFavorite(<MediaItem>[item], next);
    } catch (_) {}
  }

  Future<void> toggleAlbumFavorite() async {
    final Album? current = _album;
    if (current == null) {
      return;
    }
    await toggleItemFavorite(current);
  }

  void toggleDescription() {
    _isDescriptionExpanded = !_isDescriptionExpanded;
    _metadataNotifier.notifyListeners();
  }

  Future<void> playAlbum() async {
    if (_tracks.isEmpty) {
      return;
    }
    await coreBridge.queuePlayMedia(
      _tracks,
      providerInstance: sectionProvider.selectedProvider,
    );
  }

  Future<void> shuffleAlbum() async {
    if (_tracks.isEmpty) {
      return;
    }
    await coreBridge.queuePlayMedia(
      _tracks,
      shuffle: true,
      providerInstance: sectionProvider.selectedProvider,
    );
  }

  Future<void> startTrackRadio(Track track) async {
    await coreBridge.startRadio(<MediaItem>[track]);
  }

  Future<void> startItemRadio(MediaItem item) async {
    await coreBridge.startRadio(<MediaItem>[item]);
  }

  // Related-album projection is identical for early observations and final responses.
  List<Album> _projectOtherAlbums(List<Album> albums) =>
      List<Album>.unmodifiable(
        albums
            .where(
              (album) =>
                  !(album.itemId == itemId &&
                      album.provider == providerInstanceIdOrDomain) &&
                  !(album.itemId == sectionProvider.selectedItemId &&
                      album.provider == sectionProvider.selectedProvider),
            )
            .map(
              (album) => userDataController?.withFavoriteStatus(album) ?? album,
            )
            .toList(growable: false)
          ..sort((left, right) => (right.year ?? 0).compareTo(left.year ?? 0)),
      );
}
