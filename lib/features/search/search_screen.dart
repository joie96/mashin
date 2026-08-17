import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/config/app_config.dart';
import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_models.dart';
import '../../data/music_assistant/ma_client.dart';
import '../playback/playback_coordinator.dart';
import '../playback/playback_models.dart';
import '../shared/widgets/media_row_section_desktop.dart';
import '../shared/widgets/media_row_section_mobile.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import '../shared/widgets/media_table_section_mobile.dart';

class SearchScreen extends StatefulWidget {
  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;

  const SearchScreen({
    super.key,
    required this.client,
    required this.playbackCoordinator,
  });

  @override
  State<SearchScreen> createState() => _SearchScreenState();
}

class _SearchScreenState extends State<SearchScreen> {
  // #region Fields

  final TextEditingController _queryController = TextEditingController();
  Timer? _debounce;

  int _requestId = 0;
  bool _hasSearchRequest = false;

  bool _isLoadingTracks = false;
  bool _isLoadingAlbums = false;
  bool _isLoadingPlaylists = false;
  bool _isLoadingArtists = false;

  List<Track> _tracks = const <Track>[];
  List<Album> _albums = const <Album>[];
  List<Playlist> _playlists = const <Playlist>[];
  List<Artist> _artists = const <Artist>[];

  // #endregion

  // #region Lifecycle

  @override
  void initState() {
    super.initState();
    _primeProviderManifests();
  }

  Future<void> _primeProviderManifests() async {
    try {
      await widget.client.getProviderManifests();
      if (!mounted) {
        return;
      }
      setState(() {});
    } catch (_) {
      // Keep UI functional even if manifest loading fails.
    }
  }

  @override
  void dispose() {
    _debounce?.cancel();
    _queryController.dispose();
    super.dispose();
  }

  // #endregion

  // #region UI

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final isMobile = AppConfig.isMobile();

    return ListView(
      padding: const EdgeInsets.fromLTRB(20, 18, 20, 28),
      children: <Widget>[
        Text(
          'Suche',
          style: Theme.of(context).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w700),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _queryController,
          textInputAction: TextInputAction.search,
          decoration: InputDecoration(
            hintText: 'Tracks, Alben, Playlists, Artists',
            prefixIcon: const Icon(Icons.search_outlined),
            suffixIcon: _queryController.text.isEmpty
                ? null
                : Row(
                    mainAxisSize: MainAxisSize.min,
                    children: <Widget>[
                      IconButton(
                        tooltip: 'Suchen',
                        onPressed: _submitSearch,
                        icon: const Icon(Icons.search_outlined),
                      ),
                      IconButton(
                        tooltip: 'Leeren',
                        onPressed: _clearSearch,
                        icon: const Icon(Icons.close_outlined),
                      ),
                    ],
                  ),
          ),
          onChanged: _onQueryChanged,
          onSubmitted: (_) => _submitSearch(),
        ),
        const SizedBox(height: 20),
        if (!_hasSearchRequest)
          Container(
            padding: const EdgeInsets.all(16),
            decoration: BoxDecoration(
              color: colors.bgSecondary,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: colors.border),
            ),
            child: Text(
              'Gib einen Suchbegriff ein, um Ergebnisse anzuzeigen.',
              style: TextStyle(color: colors.textSecondary),
            ),
          )
        else ...<Widget>[
          isMobile
              ? MediaTableSectionMobile<Track>(
                  title: 'Tracks',
                  items: _tracks,
                  isLoading: _isLoadingTracks,
                  emptyText: 'Keine Tracks gefunden.',
                  rowMapper: mapTrackRowData,
                  maxItemsPerPage: 3,
                )
              : MediaTableSectionDesktop<Track>(
                  title: 'Tracks',
                  items: _tracks,
                  isLoading: _isLoadingTracks,
                  emptyText: 'Keine Tracks gefunden.',
                  rowMapper: mapTrackRowData,
                  desktopColumns: buildTrackDesktopColumns(),
                  maxItemsPerPage: 10,
                  onRowDoubleTap: _onTrackRowDoubleTap,
                ),
          const SizedBox(height: 18),
          isMobile
              ? MediaRowSectionMobile<Album>(
                  title: 'Alben',
                  isLoading: _isLoadingAlbums,
                  items: _albums,
                  emptyText: 'Keine Alben gefunden.',
                  cardMapper: albumRowCardData,
                )
              : MediaRowSectionDesktop<Album>(
                  title: 'Alben',
                  isLoading: _isLoadingAlbums,
                  items: _albums,
                  emptyText: 'Keine Alben gefunden.',
                  cardMapper: albumRowCardData,
                ),
          const SizedBox(height: 18),
          isMobile
              ? MediaRowSectionMobile<Playlist>(
                  title: 'Playlists',
                  isLoading: _isLoadingPlaylists,
                  items: _playlists,
                  emptyText: 'Keine Playlists gefunden.',
                  cardMapper: playlistRowCardData,
                )
              : MediaRowSectionDesktop<Playlist>(
                  title: 'Playlists',
                  isLoading: _isLoadingPlaylists,
                  items: _playlists,
                  emptyText: 'Keine Playlists gefunden.',
                  cardMapper: playlistRowCardData,
                ),
          const SizedBox(height: 18),
          isMobile
              ? MediaRowSectionMobile<Artist>(
                  title: 'Artists',
                  isLoading: _isLoadingArtists,
                  items: _artists,
                  emptyText: 'Keine Artists gefunden.',
                  cardMapper: artistRowCardData,
                )
              : MediaRowSectionDesktop<Artist>(
                  title: 'Artists',
                  isLoading: _isLoadingArtists,
                  items: _artists,
                  emptyText: 'Keine Artists gefunden.',
                  cardMapper: artistRowCardData,
                ),
        ],
      ],
    );
  }

  // #endregion

  // #region Input Handling

  void _onQueryChanged(String value) {
    // Rebuild needed for suffix-icon toggle while typing.
    setState(() {});

    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 320), () {
      _runSearch(value.trim());
    });
  }

  void _clearSearch() {
    _debounce?.cancel();
    _queryController.clear();

    setState(() {
      _hasSearchRequest = false;
      _isLoadingTracks = false;
      _isLoadingAlbums = false;
      _isLoadingPlaylists = false;
      _isLoadingArtists = false;
      _tracks = const <Track>[];
      _albums = const <Album>[];
      _playlists = const <Playlist>[];
      _artists = const <Artist>[];
    });
  }

  void _submitSearch() {
    // Explicit submit should run immediately and not wait for debounce.
    _debounce?.cancel();
    _runSearch(_queryController.text.trim());
  }

  // #endregion

  // #region Data Loading

  Future<void> _runSearch(String query) async {
    if (query.isEmpty) {
      _clearSearch();
      return;
    }

    final requestId = ++_requestId;

    setState(() {
      _hasSearchRequest = true;
      _isLoadingTracks = true;
      _isLoadingAlbums = true;
      _isLoadingPlaylists = true;
      _isLoadingArtists = true;
      _tracks = const <Track>[];
      _albums = const <Album>[];
      _playlists = const <Playlist>[];
      _artists = const <Artist>[];
    });

    await Future.wait(<Future<void>>[
      _searchForType(requestId, query, MediaType.track),
      _searchForType(requestId, query, MediaType.album),
      _searchForType(requestId, query, MediaType.playlist),
      _searchForType(requestId, query, MediaType.artist),
    ]);
  }

  Future<void> _searchForType(int requestId, String query, MediaType mediaType) async {
    try {
      // One request per media type keeps section loading/error states independent.
      final result = await widget.client.search(
        searchQuery: query,
        mediaTypes: <MediaType>[mediaType],
        limit: 500,
        libraryOnly: false,
      );

      if (!mounted || requestId != _requestId) {
        // Ignore stale responses when a newer query already started.
        return;
      }

      setState(() {
        switch (mediaType) {
          case MediaType.track:
            _tracks = result?.tracks ?? const <Track>[];
            break;
          case MediaType.album:
            _albums = result?.albums ?? const <Album>[];
            break;
          case MediaType.playlist:
            _playlists = result?.playlists ?? const <Playlist>[];
            break;
          case MediaType.artist:
            _artists = result?.artists ?? const <Artist>[];
            break;
          default:
            break;
        }
      });
    } catch (_) {
      if (!mounted || requestId != _requestId) {
        return;
      }

      setState(() {
        switch (mediaType) {
          case MediaType.track:
            _tracks = const <Track>[];
            break;
          case MediaType.album:
            _albums = const <Album>[];
            break;
          case MediaType.playlist:
            _playlists = const <Playlist>[];
            break;
          case MediaType.artist:
            _artists = const <Artist>[];
            break;
          default:
            break;
        }
      });
    } finally {
      if (mounted && requestId == _requestId) {
        setState(() {
          switch (mediaType) {
            case MediaType.track:
              _isLoadingTracks = false;
              break;
            case MediaType.album:
              _isLoadingAlbums = false;
              break;
            case MediaType.playlist:
              _isLoadingPlaylists = false;
              break;
            case MediaType.artist:
              _isLoadingArtists = false;
              break;
            default:
              break;
          }
        });
      }
    }
  }

  Future<void> _onTrackRowDoubleTap(Track track, int index) async {
    if (track.itemId.trim().isEmpty || track.provider.trim().isEmpty) {
      return;
    }

    try {
      await widget.playbackCoordinator.playMedia(<MediaItem>[track]);
    } catch (_) {
      // Keep the search UI responsive even if playback routing fails.
    }
  }

  // #endregion
}
