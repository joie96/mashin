import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/widgets/media_row_section_mobile.dart';
import '../shared/widgets/media_table_section_mobile.dart';
import 'search_controller.dart' as feature_search;

/// Mobile search page with local search input and stacked result sections.
class SearchScreenMobile extends StatefulWidget {
  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;
  final ValueChanged<Playlist>? onOpenPlaylist;

  const SearchScreenMobile({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    this.onOpenArtist,
    this.onOpenAlbum,
    this.onOpenPlaylist,
  });

  @override
  State<SearchScreenMobile> createState() => _SearchScreenMobileState();
}

class _SearchScreenMobileState extends State<SearchScreenMobile> {
  final TextEditingController _queryController = TextEditingController();
  Timer? _debounce;

  late final feature_search.SearchController _controller;

  @override
  void initState() {
    super.initState();
    _controller = feature_search.SearchController(client: widget.client)
      ..addListener(_onControllerChanged);
    unawaited(_controller.load());
  }

  void _onControllerChanged() {
    if (mounted) {
      setState(() {});
    }
  }

  @override
  void dispose() {
    _debounce?.cancel();
    _queryController.dispose();
    _controller.removeListener(_onControllerChanged);
    _controller.dispose();
    super.dispose();
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
        if (!_controller.hasSearchRequest)
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
          MediaTableSectionMobile<Track>(
            title: 'Tracks',
            items: _controller.tracks,
            isLoading: _controller.isLoadingTracks,
            emptyText: 'Keine Tracks gefunden.',
            maxItemsPerPage: 3,
            playbackCoordinator: widget.playbackCoordinator,
          ),
          const SizedBox(height: 18),
          MediaRowSectionMobile<Album>(
            title: 'Alben',
            isLoading: _controller.isLoadingAlbums,
            items: _controller.albums,
            emptyText: 'Keine Alben gefunden.',
            playbackCoordinator: widget.playbackCoordinator,
            onItemTap: widget.onOpenAlbum,
          ),
          const SizedBox(height: 18),
          MediaRowSectionMobile<Playlist>(
            title: 'Playlists',
            isLoading: _controller.isLoadingPlaylists,
            items: _controller.playlists,
            emptyText: 'Keine Playlists gefunden.',
            playbackCoordinator: widget.playbackCoordinator,
            onItemTap: widget.onOpenPlaylist,
          ),
          const SizedBox(height: 18),
          MediaRowSectionMobile<Artist>(
            title: 'Artists',
            isLoading: _controller.isLoadingArtists,
            items: _controller.artists,
            emptyText: 'Keine Artists gefunden.',
            playbackCoordinator: widget.playbackCoordinator,
            onItemTap: widget.onOpenArtist,
          ),
        ],
      ],
    );
  }

  void _onQueryChanged(String value) {
    setState(() {});
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 320), () {
      unawaited(_controller.runSearch(value.trim()));
    });
  }

  void _submitSearch() {
    _debounce?.cancel();
    unawaited(_controller.runSearch(_queryController.text.trim()));
  }

  void _clearSearch() {
    _debounce?.cancel();
    _queryController.clear();
    _controller.resetResults();
    setState(() {});
  }
}
