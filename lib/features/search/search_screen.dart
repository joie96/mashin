import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/config/app_config.dart';
import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_models.dart';
import '../../data/music_assistant/music_assistant_client.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import '../shared/widgets/media_table_section_mobile.dart';

class SearchScreen extends StatefulWidget {
  final MusicAssistantClient client;

  const SearchScreen({
    super.key,
    required this.client,
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

  List<MaTrack> _tracks = const <MaTrack>[];
  List<MaAlbum> _albums = const <MaAlbum>[];
  List<MaPlaylist> _playlists = const <MaPlaylist>[];
  List<MaArtist> _artists = const <MaArtist>[];

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
            prefixIcon: const Icon(Icons.search_rounded),
            suffixIcon: _queryController.text.isEmpty
                ? null
                : Row(
                    mainAxisSize: MainAxisSize.min,
                    children: <Widget>[
                      IconButton(
                        tooltip: 'Suchen',
                        onPressed: _submitSearch,
                        icon: const Icon(Icons.search_rounded),
                      ),
                      IconButton(
                        tooltip: 'Leeren',
                        onPressed: _clearSearch,
                        icon: const Icon(Icons.close_rounded),
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
              ? SizedBox(
                  height: 500,
                  child: MediaTableSectionMobile<MaTrack>(
                    title: 'Tracks',
                    items: _tracks,
                    isLoading: _isLoadingTracks,
                    emptyText: 'Keine Tracks gefunden.',
                    rowMapper: mapTrackRowData,
                    maxItemsPerPage: 0,
                  ),
                )
              : SizedBox(
                  height: 500,
                  child: MediaTableSectionDesktop<MaTrack>(
                    title: 'Tracks',
                    items: _tracks,
                    isLoading: _isLoadingTracks,
                    emptyText: 'Keine Tracks gefunden.',
                    rowMapper: mapTrackRowData,
                    desktopColumns: buildTrackDesktopColumns(),
                    maxItemsPerPage: 0,
                  ),
                ),
          const SizedBox(height: 18),
          isMobile
              ? MediaTableSectionMobile<MaAlbum>(
                  title: 'Alben',
                  items: _albums,
                  isLoading: _isLoadingAlbums,
                  emptyText: 'Keine Alben gefunden.',
                  rowMapper: mapAlbumRowData,
                  maxItemsPerPage: 3,
                )
              : MediaTableSectionDesktop<MaAlbum>(
                  title: 'Alben',
                  items: _albums,
                  isLoading: _isLoadingAlbums,
                  emptyText: 'Keine Alben gefunden.',
                  rowMapper: mapAlbumRowData,
                  desktopColumns: buildAlbumDesktopColumns(),
                  maxItemsPerPage: 3,
                ),
          const SizedBox(height: 18),
          isMobile
              ? MediaTableSectionMobile<MaPlaylist>(
                  title: 'Playlists',
                  items: _playlists,
                  isLoading: _isLoadingPlaylists,
                  emptyText: 'Keine Playlists gefunden.',
                  rowMapper: mapPlaylistRowData,
                  maxItemsPerPage: 3,
                )
              : MediaTableSectionDesktop<MaPlaylist>(
                  title: 'Playlists',
                  items: _playlists,
                  isLoading: _isLoadingPlaylists,
                  emptyText: 'Keine Playlists gefunden.',
                  rowMapper: mapPlaylistRowData,
                  desktopColumns: buildPlaylistDesktopColumns(),
                  maxItemsPerPage: 3,
                ),
          const SizedBox(height: 18),
          isMobile
              ? MediaTableSectionMobile<MaArtist>(
                  title: 'Artists',
                  items: _artists,
                  isLoading: _isLoadingArtists,
                  emptyText: 'Keine Artists gefunden.',
                  rowMapper: mapArtistRowData,
                  maxItemsPerPage: 3,
                )
              : MediaTableSectionDesktop<MaArtist>(
                  title: 'Artists',
                  items: _artists,
                  isLoading: _isLoadingArtists,
                  emptyText: 'Keine Artists gefunden.',
                  rowMapper: mapArtistRowData,
                  desktopColumns: buildArtistDesktopColumns(),
                  maxItemsPerPage: 3,
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
      _tracks = const <MaTrack>[];
      _albums = const <MaAlbum>[];
      _playlists = const <MaPlaylist>[];
      _artists = const <MaArtist>[];
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
      _tracks = const <MaTrack>[];
      _albums = const <MaAlbum>[];
      _playlists = const <MaPlaylist>[];
      _artists = const <MaArtist>[];
    });

    await Future.wait(<Future<void>>[
      _searchForType(requestId, query, MaMediaType.track),
      _searchForType(requestId, query, MaMediaType.album),
      _searchForType(requestId, query, MaMediaType.playlist),
      _searchForType(requestId, query, MaMediaType.artist),
    ]);
  }

  Future<void> _searchForType(int requestId, String query, MaMediaType mediaType) async {
    try {
      // One request per media type keeps section loading/error states independent.
      final result = await widget.client.search(
        searchQuery: query,
        mediaTypes: <MaMediaType>[mediaType],
        limit: 500,
        libraryOnly: false,
      );

      if (!mounted || requestId != _requestId) {
        // Ignore stale responses when a newer query already started.
        return;
      }

      setState(() {
        switch (mediaType) {
          case MaMediaType.track:
            _tracks = result?.tracks ?? const <MaTrack>[];
            break;
          case MaMediaType.album:
            _albums = result?.albums ?? const <MaAlbum>[];
            break;
          case MaMediaType.playlist:
            _playlists = result?.playlists ?? const <MaPlaylist>[];
            break;
          case MaMediaType.artist:
            _artists = result?.artists ?? const <MaArtist>[];
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
          case MaMediaType.track:
            _tracks = const <MaTrack>[];
            break;
          case MaMediaType.album:
            _albums = const <MaAlbum>[];
            break;
          case MaMediaType.playlist:
            _playlists = const <MaPlaylist>[];
            break;
          case MaMediaType.artist:
            _artists = const <MaArtist>[];
            break;
          default:
            break;
        }
      });
    } finally {
      if (mounted && requestId == _requestId) {
        setState(() {
          switch (mediaType) {
            case MaMediaType.track:
              _isLoadingTracks = false;
              break;
            case MaMediaType.album:
              _isLoadingAlbums = false;
              break;
            case MaMediaType.playlist:
              _isLoadingPlaylists = false;
              break;
            case MaMediaType.artist:
              _isLoadingArtists = false;
              break;
            default:
              break;
          }
        });
      }
    }
  }

  // #endregion
}
