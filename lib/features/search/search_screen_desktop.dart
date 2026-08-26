import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/userdata_controller.dart';
import '../shared/widgets/media_row_section_desktop.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import 'search_controller.dart' as feature_search;

/// Desktop search page driven by the shell top-bar query.
class SearchScreenDesktop extends StatefulWidget {
  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final UserDataController? userDataController;
  final bool desktopSearchDriven;
  final String desktopSearchQuery;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const SearchScreenDesktop({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    this.userDataController,
    this.desktopSearchDriven = false,
    this.desktopSearchQuery = '',
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  State<SearchScreenDesktop> createState() => _SearchScreenDesktopState();
}

class _SearchScreenDesktopState extends State<SearchScreenDesktop> {
  late final feature_search.SearchController _controller;

  @override
  void initState() {
    super.initState();
    _controller = feature_search.SearchController(
      client: widget.client,
      userDataController: widget.userDataController,
    )
      ..addListener(_onControllerChanged);
    widget.userDataController?.addListener(_onUserDataChanged);
    unawaited(_controller.load());

    if (widget.desktopSearchDriven) {
      _syncDesktopQuery(widget.desktopSearchQuery);
    }
  }

  @override
  void didUpdateWidget(covariant SearchScreenDesktop oldWidget) {
    super.didUpdateWidget(oldWidget);

    if (!widget.desktopSearchDriven) {
      return;
    }

    if (oldWidget.desktopSearchQuery != widget.desktopSearchQuery) {
      _syncDesktopQuery(widget.desktopSearchQuery);
    }
  }

  void _onControllerChanged() {
    if (mounted) {
      setState(() {});
    }
  }

  @override
  void dispose() {
    _controller.removeListener(_onControllerChanged);
    _controller.dispose();
    widget.userDataController?.removeListener(_onUserDataChanged);
    super.dispose();
  }

  void _onUserDataChanged() {
    _controller.refreshFavoriteStatuses();
  }

  bool _isItemFavorite(MediaItem item) => item.favorite;

  void _toggleFavorite(MediaItem item) {
    final UserDataController? userDataController = widget.userDataController;
    if (userDataController == null) {
      return;
    }
    unawaited(
      userDataController.setFavorite(
        <MediaItem>[item],
        !_isItemFavorite(item),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return ListView(
      padding: const EdgeInsets.fromLTRB(20, 18, 20, 28),
      children: <Widget>[
        Text(
          'Suche',
          style: Theme.of(
            context,
          ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w700),
        ),
        const SizedBox(height: 20),
        if (!_controller.hasSearchRequest)
          Container(
            padding: const EdgeInsets.all(16),
            decoration: BoxDecoration(
              color: colors.bgSecondary,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: colors.border),
            ),
            child: Text(
              'Nutze die Suchleiste oben, um Ergebnisse anzuzeigen.',
              style: TextStyle(color: colors.textSecondary),
            ),
          )
        else ...<Widget>[
          MediaTableSectionDesktop<Track>(
            title: 'Tracks',
            items: _controller.tracks,
            isLoading: _controller.isLoadingTracks,
            emptyText: 'Keine Tracks gefunden.',
            maxItemsPerPage: 10,
            playbackCoordinator: widget.playbackCoordinator,
            isRowFavorite: (Track item, int _) => _isItemFavorite(item),
            onRowFavoriteTap: widget.userDataController == null
              ? null
              : (Track item, int _) => _toggleFavorite(item),
            onArtistTap: widget.onOpenArtist,
            onAlbumTap: widget.onOpenAlbum,
          ),
          const SizedBox(height: 20),
          MediaRowSectionDesktop<Album>(
            title: 'Alben',
            isLoading: _controller.isLoadingAlbums,
            items: _controller.albums,
            emptyText: 'Keine Alben gefunden.',
            playbackCoordinator: widget.playbackCoordinator,
            isRowFavorite: (Album item, int _) => _isItemFavorite(item),
            onRowFavoriteTap: widget.userDataController == null
              ? null
              : (Album item, int _) => _toggleFavorite(item),
            onArtistTap: widget.onOpenArtist,
            onAlbumTap: widget.onOpenAlbum,
          ),
          const SizedBox(height: 20),
          MediaRowSectionDesktop<Playlist>(
            title: 'Playlists',
            isLoading: _controller.isLoadingPlaylists,
            items: _controller.playlists,
            emptyText: 'Keine Playlists gefunden.',
            playbackCoordinator: widget.playbackCoordinator,
            isRowFavorite: (Playlist item, int _) => _isItemFavorite(item),
            onRowFavoriteTap: widget.userDataController == null
              ? null
              : (Playlist item, int _) => _toggleFavorite(item),
            onPlaylistTap: widget.onOpenPlaylist,
          ),
          const SizedBox(height: 20),
          MediaRowSectionDesktop<Artist>(
            title: 'Artists',
            isLoading: _controller.isLoadingArtists,
            items: _controller.artists,
            emptyText: 'Keine Artists gefunden.',
            playbackCoordinator: widget.playbackCoordinator,
            isRowFavorite: (Artist item, int _) => _isItemFavorite(item),
            onRowFavoriteTap: widget.userDataController == null
              ? null
              : (Artist item, int _) => _toggleFavorite(item),
            onArtistTap: widget.onOpenArtist,
          ),
        ],
      ],
    );
  }

  void _syncDesktopQuery(String query) {
    unawaited(_controller.runSearch(query.trim()));
  }
}
