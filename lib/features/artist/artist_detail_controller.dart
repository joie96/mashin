import 'dart:convert';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:http/http.dart' as http;

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_userdata.dart';

/// Loads and holds all data required by the artist detail page (metadata, albums,
/// similar artists, top tracks) and exposes the playback/favorite/description actions
/// used by the desktop and mobile artist detail pages.
class ArtistDetailController extends ChangeNotifier {
  static const int descriptionCollapsedMaxLines = 4;
  static final http.Client _deezerHttpClient = http.Client();

  final MashinCoreBridgeApi coreBridge;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;
  final ChangeNotifier _metadataNotifier = ChangeNotifier();
  final ChangeNotifier _albumsNotifier = ChangeNotifier();
  final ChangeNotifier _similarArtistsNotifier = ChangeNotifier();
  final ChangeNotifier _topTracksNotifier = ChangeNotifier();

  ArtistDetailController({
    required this.coreBridge,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
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
  bool _isDisposed = false;

  @override
  void notifyListeners() {
    if (!_isDisposed) {
      super.notifyListeners();
    }
  }

  @override
  void dispose() {
    _isDisposed = true;
    super.dispose();
  }

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
  int get descriptionMaxLines =>
      _isDescriptionExpanded ? 1000 : descriptionCollapsedMaxLines;
  Listenable get metadataListenable => _metadataNotifier;
  Listenable get albumsListenable => _albumsNotifier;
  Listenable get similarArtistsListenable => _similarArtistsNotifier;
  Listenable get topTracksListenable => _topTracksNotifier;

  // Data Loading
  Future<void> loadArtist() async {
    _isLoadingMetadata = true;
    _isLoadingAlbums = true;
    _isLoadingTopTracks = true;
    _isLoadingSimilarArtists = true;
    notifyListeners();

    await _loadArtistMetadata();
    await _loadAlbums();
    await _loadTopTracks();
    await _loadSimilarArtists();
  }

  Future<void> _loadArtistMetadata() async {
    _isLoadingMetadata = true;
    notifyListeners();
    try {
      final Artist loaded = await coreBridge.getArtist(
        itemId,
        providerInstanceIdOrDomain,
      );
      _artist = userDataController?.withFavoriteStatus(loaded) ?? loaded;
      final Artist? artist = _artist;
      _isArtistFavorite = artist != null && isItemFavorite(artist);
      await _loadDeezerBiographyIfNeeded();
    } catch (_) {
      _artist = null;
    } finally {
      _isLoadingMetadata = false;
      _metadataNotifier.notifyListeners();
      notifyListeners();
    }
  }

  Future<void> _loadAlbums() async {
    _isLoadingAlbums = true;
    notifyListeners();
    try {
      final List<Album> albums = await coreBridge.getArtistAlbums(
        itemId,
        providerInstanceIdOrDomain,
      );
      _albums = List<Album>.unmodifiable(
        albums
            .map(
              (Album album) =>
                  userDataController?.withFavoriteStatus(album) ?? album,
            )
            .toList(growable: false)
          ..sort((Album left, Album right) {
            return (right.year ?? 0).compareTo(left.year ?? 0);
          }),
      );
    } catch (_) {
      _albums = const <Album>[];
    } finally {
      _isLoadingAlbums = false;
      _albumsNotifier.notifyListeners();
      notifyListeners();
    }
  }

  Future<void> _loadSimilarArtists() async {
    _isLoadingSimilarArtists = true;
    notifyListeners();
    try {
      List<Artist> artists = await coreBridge.getSimilarArtists(
        itemId,
        providerInstanceIdOrDomain,
        limit: 25,
      );
      if (artists.isEmpty) {
        artists = await _loadSimilarArtistsFromTracks();
      }
      _similarArtists = List<Artist>.unmodifiable(
        artists
            .map(
              (Artist artist) =>
                  userDataController?.withFavoriteStatus(artist) ?? artist,
            )
            .toList(growable: false),
      );
    } catch (_) {
      _similarArtists = const <Artist>[];
    } finally {
      _isLoadingSimilarArtists = false;
      _similarArtistsNotifier.notifyListeners();
      notifyListeners();
    }
  }

  Future<void> _loadTopTracks() async {
    _isLoadingTopTracks = true;
    notifyListeners();
    try {
      final List<Track> tracks = await coreBridge.getArtistTopTracks(
        itemId,
        providerInstanceIdOrDomain,
      );
      _topTracks = List<Track>.unmodifiable(
        tracks
            .map(_withAlbumYear)
            .map(
              (Track track) =>
                  userDataController?.withFavoriteStatus(track) ?? track,
            )
            .toList(growable: false),
      );
    } catch (_) {
      _topTracks = const <Track>[];
    } finally {
      _isLoadingTopTracks = false;
      _topTracksNotifier.notifyListeners();
      notifyListeners();
    }
  }

  Future<void> _loadDeezerBiographyIfNeeded() async {
    final Artist? currentArtist = _artist;
    if (currentArtist == null ||
        !providerInstanceIdOrDomain.toLowerCase().startsWith('deezer') ||
        (currentArtist.metadata?.description?.trim().isNotEmpty ?? false)) {
      return;
    }

    final String language = ui.PlatformDispatcher.instance.locale.languageCode
        .trim();
    final String? biography = await _getDeezerArtistBio(
      itemId,
      language.isEmpty ? 'de' : language,
    );
    if (biography == null || biography.isEmpty) {
      return;
    }

    final MediaItemMetadata previousMetadata =
        currentArtist.metadata ??
        const MediaItemMetadata(images: <MediaItemImage>[], genres: <String>[]);
    _artist = Artist(
      itemId: currentArtist.itemId,
      provider: currentArtist.provider,
      name: currentArtist.name,
      version: currentArtist.version,
      sortName: currentArtist.sortName,
      uri: currentArtist.uri,
      externalIds: currentArtist.externalIds,
      isPlayable: currentArtist.isPlayable,
      available: currentArtist.available,
      providerMappings: currentArtist.providerMappings,
      favorite: currentArtist.favorite,
      position: currentArtist.position,
      dateAdded: currentArtist.dateAdded,
      artistType: currentArtist.artistType,
      metadata: MediaItemMetadata(
        description: biography,
        descriptionLanguage: previousMetadata.descriptionLanguage,
        images: previousMetadata.images,
        genres: previousMetadata.genres,
        label: previousMetadata.label,
        popularity: previousMetadata.popularity,
        cacheChecksum: previousMetadata.cacheChecksum,
        explicit: previousMetadata.explicit,
        lyrics: previousMetadata.lyrics,
        transcript: previousMetadata.transcript,
      ),
    );
  }

  Future<String?> _getDeezerArtistBio(String artistId, String language) async {
    try {
      final http.Response loginResponse = await _deezerHttpClient.get(
        Uri.parse('https://auth.deezer.com/login/anonymous?jo=p&rto=c'),
        headers: <String, String>{
          'accept': 'application/json',
          'accept-language': language.toLowerCase(),
          'referer': 'https://www.deezer.com/',
          'user-agent': 'Mozilla/5.0',
        },
      );
      if (loginResponse.statusCode < 200 || loginResponse.statusCode >= 300) {
        return null;
      }

      final Object? loginPayload = jsonDecode(loginResponse.body);
      final String? token = loginPayload is Map<String, dynamic>
          ? loginPayload['jwt'] as String?
          : null;
      if (token == null || token.trim().isEmpty) {
        return null;
      }

      final http.Response biographyResponse = await _deezerHttpClient.post(
        Uri.parse('https://pipe.deezer.com/api'),
        headers: <String, String>{
          'authorization': 'Bearer ${token.trim()}',
          'content-type': 'application/json',
          'origin': 'https://www.deezer.com',
          'referer': 'https://www.deezer.com/',
          'accept-language': language.toLowerCase(),
        },
        body: jsonEncode(<String, Object>{
          'operationName': 'ArtistBio',
          'variables': <String, String>{'artistId': artistId},
          'query':
              'query ArtistBio(\$artistId: String!) { artist(artistId: \$artistId) { id name bio { full } } }',
        }),
      );
      if (biographyResponse.statusCode < 200 ||
          biographyResponse.statusCode >= 300) {
        return null;
      }

      final Object? payload = jsonDecode(biographyResponse.body);
      final Object? data = payload is Map<String, dynamic>
          ? payload['data']
          : null;
      final Object? artist = data is Map<String, dynamic>
          ? data['artist']
          : null;
      final Object? biography = artist is Map<String, dynamic>
          ? artist['bio']
          : null;
      final String? rawText = biography is Map<String, dynamic>
          ? biography['full'] as String?
          : null;
      return _cleanDeezerBiography(rawText);
    } catch (_) {
      return null;
    }
  }

  String? _cleanDeezerBiography(String? rawText) {
    if (rawText == null || rawText.trim().isEmpty) {
      return null;
    }

    String text = rawText
        .replaceAll(RegExp(r'<\s*br\s*/?\s*>', caseSensitive: false), '\n')
        .replaceAll(RegExp(r'<\s*/\s*p\s*>', caseSensitive: false), '\n\n')
        .replaceAll(RegExp(r'<\s*p[^>]*>', caseSensitive: false), '')
        .replaceAll(RegExp(r'<[^>]+>'), '')
        .replaceAll('&amp;', '&')
        .replaceAll('&quot;', '"')
        .replaceAll('&apos;', "'")
        .replaceAll('&lt;', '<')
        .replaceAll('&gt;', '>')
        .replaceAll('\r\n', '\n')
        .replaceAll('\r', '\n');
    text = text
        .replaceAll(RegExp(r'[ \t]+\n'), '\n')
        .replaceAll(RegExp(r'\n{3,}'), '\n\n')
        .trim();
    return text.isEmpty ? null : text;
  }

  Track _withAlbumYear(Track track) {
    final Album? trackAlbum = track.album;
    if (trackAlbum == null || trackAlbum.year != null || _albums.isEmpty) {
      return track;
    }

    final Album? matchingAlbum = _albums.cast<Album?>().firstWhere(
      (Album? album) => album?.itemId == trackAlbum.itemId,
      orElse: () => _albums.cast<Album?>().firstWhere(
        (Album? album) =>
            album?.name.toLowerCase() == trackAlbum.name.toLowerCase(),
        orElse: () => null,
      ),
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

  Future<List<Artist>> _loadSimilarArtistsFromTracks() async {
    final Track? topTrack = _topTracks.isEmpty ? null : _topTracks.first;
    if (topTrack == null) {
      return const <Artist>[];
    }

    List<Track> similarTracks = await coreBridge.getSimilarTracks(
      topTrack.itemId,
      topTrack.provider,
      limit: 50,
      allowLookup: true,
    );
    if (similarTracks.isEmpty) {
      final List<Track> versions = await coreBridge.getTrackVersions(
        topTrack.itemId,
        topTrack.provider,
      );
      if (versions.isNotEmpty) {
        final Track fallbackVersion = versions.firstWhere(
          (Track version) => version.provider != topTrack.provider,
          orElse: () => versions.first,
        );
        similarTracks = await coreBridge.getSimilarTracks(
          fallbackVersion.itemId,
          fallbackVersion.provider,
          limit: 50,
          allowLookup: true,
        );
      }
    }

    final Set<String> seen = <String>{};
    final List<Artist> fallbackArtists = <Artist>[];
    for (final Track track in similarTracks) {
      for (final Artist candidate in track.artists) {
        final String key = '${candidate.provider}|${candidate.itemId}';
        if (candidate.itemId == itemId || !seen.add(key)) {
          continue;
        }
        fallbackArtists.add(candidate);
        if (fallbackArtists.length == 25) {
          break;
        }
      }
      if (fallbackArtists.length == 25) {
        break;
      }
    }

    final List<Artist> enriched = await Future.wait<Artist>(
      fallbackArtists.map((Artist candidate) async {
        try {
          return await coreBridge.getArtist(
            candidate.itemId,
            candidate.provider,
          );
        } catch (_) {
          return candidate;
        }
      }),
    );
    return enriched;
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
      await coreBridge.queuePlayMedia(items);
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
      await coreBridge.queuePlayMedia(items, shuffle: true);
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
      await coreBridge.startSendspinRadio(<MediaItem>[currentArtist]);
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
      await coreBridge.startSendspinRadio(<MediaItem>[item]);
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
      await coreBridge.startSendspinRadio(<MediaItem>[track]);
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }
}
