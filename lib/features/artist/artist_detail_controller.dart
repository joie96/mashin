import 'dart:convert';
import 'dart:math';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:http/http.dart' as http;

import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';

/// Loads and holds all data required by the artist detail page (metadata, albums,
/// similar artists, top tracks) and exposes the playback/favorite/description actions
/// used by the desktop and mobile artist detail pages.
class ArtistDetailController extends ChangeNotifier {
  static const int descriptionCollapsedMaxLines = 4;
  static final http.Client _deezerHttpClient = http.Client();
  final Random _shuffleRandom = Random();

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
      _artist = await client.getArtist(itemId, providerInstanceIdOrDomain);
      _isArtistFavorite = _artist?.favorite ?? false;
      await _loadDeezerBiographyIfNeeded();
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
      final List<Album> albums = await client.getArtistAlbums(
        itemId,
        providerInstanceIdOrDomain,
      );
      _albums = List<Album>.unmodifiable(
        <Album>[...albums]..sort((Album left, Album right) {
          return (right.year ?? 0).compareTo(left.year ?? 0);
        }),
      );
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
      List<Artist> artists = await client.getSimilarArtists(
        itemId,
        providerInstanceIdOrDomain,
        limit: 25,
      );
      if (artists.isEmpty) {
        artists = await _loadSimilarArtistsFromTracks();
      }
      _similarArtists = List<Artist>.unmodifiable(artists);
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
      final List<Track> tracks = await client.getArtistTopTracks(
        itemId,
        providerInstanceIdOrDomain,
      );
      _topTracks = List<Track>.unmodifiable(
        tracks.map(_withAlbumYear).toList(growable: false),
      );
    } catch (_) {
      _topTracks = const <Track>[];
    } finally {
      _isLoadingTopTracks = false;
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
      sortName: currentArtist.sortName,
      uri: currentArtist.uri,
      providerMappings: currentArtist.providerMappings,
      favorite: currentArtist.favorite,
      metadata: MediaItemMetadata(
        description: biography,
        images: previousMetadata.images,
        genres: previousMetadata.genres,
        label: previousMetadata.label,
        popularity: previousMetadata.popularity,
        cacheChecksum: previousMetadata.cacheChecksum,
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
      artists: trackAlbum.artists,
      sortName: trackAlbum.sortName,
      uri: trackAlbum.uri,
      year: matchingAlbum!.year,
      albumType: trackAlbum.albumType,
      providerMappings: trackAlbum.providerMappings,
      metadata: trackAlbum.metadata,
      favorite: trackAlbum.favorite,
    );
    return Track(
      itemId: track.itemId,
      provider: track.provider,
      name: track.name,
      duration: track.duration,
      artists: track.artists,
      sortName: track.sortName,
      uri: track.uri,
      album: albumWithYear,
      providerMappings: track.providerMappings,
      metadata: track.metadata,
      favorite: track.favorite,
    );
  }

  Future<List<Artist>> _loadSimilarArtistsFromTracks() async {
    final Track? topTrack = _topTracks.isEmpty ? null : _topTracks.first;
    if (topTrack == null) {
      return const <Artist>[];
    }

    List<Track> similarTracks = await client.getSimilarTracks(
      topTrack.itemId,
      topTrack.provider,
      limit: 50,
      allowLookup: true,
    );
    if (similarTracks.isEmpty) {
      final List<Track> versions = await client.getTrackVersions(
        topTrack.itemId,
        topTrack.provider,
      );
      if (versions.isNotEmpty) {
        final Track fallbackVersion = versions.firstWhere(
          (Track version) => version.provider != topTrack.provider,
          orElse: () => versions.first,
        );
        similarTracks = await client.getSimilarTracks(
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
          return await client.getArtist(candidate.itemId, candidate.provider) ??
              candidate;
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
    notifyListeners();

    try {
      if (nextFavorite) {
        final String uri = item.uri ?? '';
        if (uri.isNotEmpty) {
          await client.addFavorite(uri);
        }
      } else {
        await client.removeFavorite(
          mediaType: item.mediaType,
          libraryItemId: item.itemId,
        );
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
        await client.removeFavorite(
          mediaType: MediaType.artist,
          libraryItemId: current.itemId,
        );
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

  Future<void> startArtistRadio() async {
    final Artist? currentArtist = _artist;
    final List<MediaItem> items = _playableTracks(_topTracks);
    if (currentArtist == null || items.isEmpty) {
      return;
    }

    try {
      final MediaItem startItem = items[_shuffleRandom.nextInt(items.length)];
      await playbackCoordinator.playMedia(<MediaItem>[startItem]);
      await playbackCoordinator.playMediaRadioNext(<MediaItem>[currentArtist]);
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }

  /// Starts radio for an arbitrary media item (similar artist, album, ...).
  /// Mirrors MAUI's artist context menu: play the item, then queue radio next.
  Future<void> startItemRadio(MediaItem item) async {
    if (item.itemId.trim().isEmpty || item.provider.trim().isEmpty) {
      return;
    }

    try {
      await playbackCoordinator.playMedia(<MediaItem>[item]);
      await playbackCoordinator.playMediaRadioNext(<MediaItem>[item]);
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }

  /// Starts radio from a track. Mirrors MAUI's track context menu: play the
  /// track, queue radio next, then remove the duplicate queue entry that the
  /// radio command creates right after the played track.
  Future<void> startTrackRadio(Track track) async {
    if (track.itemId.trim().isEmpty || track.provider.trim().isEmpty) {
      return;
    }

    try {
      await playbackCoordinator.playMedia(<MediaItem>[track]);
      await playbackCoordinator.playMediaRadioNext(<MediaItem>[track]);

      // The radio command inserts a duplicate of the seed track directly after
      // the played one; remove it once it shows up in the queue.
      const int duplicateIndex = 1;
      for (int attempt = 0; attempt < 10; attempt++) {
        final List<QueueItem> queueItems = playbackCoordinator.queueItems;
        if (queueItems.length > duplicateIndex) {
          final String queueItemId = queueItems[duplicateIndex].queueItemId;
          if (queueItemId.trim().isNotEmpty) {
            await playbackCoordinator.deleteQueueItem(queueItemId);
            return;
          }
        }
        await Future<void>.delayed(const Duration(milliseconds: 500));
      }
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }
}
