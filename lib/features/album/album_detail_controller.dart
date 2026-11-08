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
    _catalogSubscription = coreBridge.catalogUpdates.listen(_onCatalogUpdate);
    _readSubscription = coreBridge.catalogReadChanges.listen(
      _onCatalogReadChange,
    );
  }

  final DetailProviderSelection sectionProvider;
  StreamSubscription<CatalogUpdate>? _catalogSubscription;
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
  final Map<String, bool> _favoriteOverrides = <String, bool>{};

  bool _isLoadingMetadata = true;
  bool _isLoadingTracks = true;
  bool _isLoadingOtherAlbums = true;
  bool _isLoadingSimilarArtists = true;
  bool _isAlbumFavorite = false;
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
      _isAlbumFavorite = isItemFavorite(_album!);
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
    } else if (_album?.artists.any(
          (artist) =>
              artist.itemId == update.itemId &&
                  artist.provider == update.provider ||
              artist.providerMappings.any(
                (mapping) =>
                    mapping.itemId == update.itemId &&
                    mapping.providerInstance == update.provider,
              ),
        ) ??
        false) {
      switch (update.kind) {
        case CatalogQueryKind.artistAlbums:
          _otherAlbums = update.albums
              .where(
                (album) =>
                    album.itemId != _album?.itemId ||
                    album.provider != _album?.provider,
              )
              .map(
                (album) =>
                    userDataController?.withFavoriteStatus(album) ?? album,
              )
              .toList(growable: false);
        case CatalogQueryKind.similarArtists:
          _similarArtists = update.artists
              .map(
                (artist) =>
                    userDataController?.withFavoriteStatus(artist) ?? artist,
              )
              .toList(growable: false);
        default:
          return;
      }
      _relatedNotifier.notifyListeners();
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
    _favoriteOverrides.clear();
    _sectionContext = null;
    _hasObservedTracks = false;
    _isAlbumFavorite = false;
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
  bool get isAlbumFavorite => _isAlbumFavorite;
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
      await Future.wait(<Future<void>>[_loadTracks(), _loadOtherAlbums()]);
      if (_isLoadCanceled) return;
      await _loadSimilarArtists();
    });
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
      final Album? album = _album;
      _isAlbumFavorite = album != null && isItemFavorite(album);
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
    final timer = Stopwatch()..start();
    _logSection('other_albums', 'started');
    Artist? artist = _album?.artists.firstOrNull;
    if (artist == null) {
      _otherAlbums = const <Album>[];
      _isLoadingOtherAlbums = false;
      _logSection('other_albums', 'skipped reason=no_artist');
      _relatedNotifier.notifyListeners();
      notifyListeners();
      return;
    }

    try {
      if (sectionProvider.selectedProvider != 'library' &&
          artist.provider != sectionProvider.selectedProvider) {
        final Artist fullArtist = await coreBridge.getArtist(
          artist.itemId,
          artist.provider,
        );
        if (_isLoadCanceled) return;
        final ProviderMapping? mapping = fullArtist.providerMappings
            .where(
              (mapping) =>
                  mapping.providerInstance ==
                      sectionProvider.selectedProvider &&
                  mapping.available,
            )
            .firstOrNull;
        if (mapping == null) {
          _otherAlbums = const <Album>[];
          _logSection('other_albums', 'skipped reason=no_provider_mapping');
          return;
        }
        artist = Artist(
          itemId: mapping.itemId,
          provider: mapping.providerInstance,
          name: fullArtist.name,
        );
      }
      final List<Album> loaded = await coreBridge.getArtistAlbums(
        artist.itemId,
        artist.provider,
      );
      if (_isLoadCanceled) return;
      _otherAlbums = List<Album>.unmodifiable(
        loaded
            .where(
              (Album item) =>
                  !(item.itemId == itemId &&
                      item.provider == providerInstanceIdOrDomain) &&
                  !(item.itemId == sectionProvider.selectedItemId &&
                      item.provider == sectionProvider.selectedProvider),
            )
            .map(
              (Album item) =>
                  userDataController?.withFavoriteStatus(item) ?? item,
            )
            .toList(growable: false)
          ..sort(
            (Album left, Album right) =>
                (right.year ?? 0).compareTo(left.year ?? 0),
          ),
      );
      _logSection(
        'other_albums',
        'loaded count=${_otherAlbums.length} elapsed_ms=${timer.elapsedMilliseconds}',
      );
    } catch (error) {
      if (_isLoadCanceled) return;
      _logSection(
        'other_albums',
        'failed elapsed_ms=${timer.elapsedMilliseconds} error=$error',
        level: LogLevelPreference.warn,
      );
      _otherAlbums = const <Album>[];
    } finally {
      if (!_isLoadCanceled) {
        _isLoadingOtherAlbums = false;
        _relatedNotifier.notifyListeners();
        notifyListeners();
      }
    }
  }

  Future<void> _loadSimilarArtists() async {
    final timer = Stopwatch()..start();
    _logSection('similar_artists', 'started');
    final Track? seed = _tracks.firstOrNull;
    if (seed == null) {
      _similarArtists = const <Artist>[];
      _isLoadingSimilarArtists = false;
      _logSection('similar_artists', 'skipped reason=no_tracks');
      _relatedNotifier.notifyListeners();
      notifyListeners();
      return;
    }

    try {
      List<Track> similarTracks = await coreBridge.getSimilarTracks(
        seed.itemId,
        seed.provider,
        limit: 50,
        allowLookup: true,
      );
      if (_isLoadCanceled) return;
      if (similarTracks.isEmpty) {
        final List<Track> versions = await coreBridge.getTrackVersions(
          seed.itemId,
          seed.provider,
        );
        if (_isLoadCanceled) return;
        if (versions.isNotEmpty) {
          final Track fallback = versions.firstWhere(
            (Track version) => version.provider != seed.provider,
            orElse: () => versions.first,
          );
          similarTracks = await coreBridge.getSimilarTracks(
            fallback.itemId,
            fallback.provider,
            limit: 50,
            allowLookup: true,
          );
          if (_isLoadCanceled) return;
        }
      }

      final String? albumArtistId = _album?.artists.firstOrNull?.itemId;
      final Map<String, Artist> unique = <String, Artist>{};
      for (final Track track in similarTracks) {
        for (final Artist artist in track.artists) {
          if (artist.itemId != albumArtistId) {
            unique.putIfAbsent(artist.itemId, () => artist);
          }
        }
      }

      final List<Artist> enriched = <Artist>[];
      for (final Artist artist in unique.values.take(15)) {
        if (_isLoadCanceled) return;
        try {
          enriched.add(
            await coreBridge.getArtist(artist.itemId, artist.provider),
          );
        } catch (_) {
          enriched.add(artist);
        }
      }
      if (_isLoadCanceled) return;
      _similarArtists = List<Artist>.unmodifiable(
        enriched
            .map(
              (Artist artist) =>
                  userDataController?.withFavoriteStatus(artist) ?? artist,
            )
            .toList(growable: false),
      );
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
      _similarArtists = const <Artist>[];
    } finally {
      if (!_isLoadCanceled) {
        _isLoadingSimilarArtists = false;
        _relatedNotifier.notifyListeners();
        notifyListeners();
      }
    }
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

  bool isItemFavorite(MediaItem item) =>
      _favoriteOverrides[_favoriteKey(item)] ?? item.favorite;

  Future<void> toggleItemFavorite(MediaItem item) async {
    final String key = _favoriteKey(item);
    final bool next = !isItemFavorite(item);
    _favoriteOverrides[key] = next;
    _tracksNotifier.notifyListeners();
    _relatedNotifier.notifyListeners();
    try {
      final UserData? controller = userDataController;
      if (controller == null) {
        _favoriteOverrides.remove(key);
        _tracksNotifier.notifyListeners();
        _relatedNotifier.notifyListeners();
        return;
      }
      await controller.setFavorite(<MediaItem>[item], next);
    } catch (_) {
      _favoriteOverrides[key] = !next;
      _tracksNotifier.notifyListeners();
      _relatedNotifier.notifyListeners();
    }
  }

  Future<void> toggleAlbumFavorite() async {
    final Album? current = _album;
    if (current == null) {
      return;
    }
    final bool next = !_isAlbumFavorite;
    _isAlbumFavorite = next;
    _metadataNotifier.notifyListeners();
    try {
      final UserData? controller = userDataController;
      if (controller == null) {
        _isAlbumFavorite = !next;
        _metadataNotifier.notifyListeners();
        return;
      }
      await controller.setFavorite(<MediaItem>[current], next);
    } catch (_) {
      _isAlbumFavorite = !next;
      _metadataNotifier.notifyListeners();
    }
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

  String _favoriteKey(MediaItem item) =>
      '${item.mediaType.name}|${item.provider}|${item.itemId}';
}
