import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../shared/widgets/media_row_section_mobile.dart';
import '../shared/widgets/media_table_section_mobile.dart';
import 'album_detail_controller.dart';

class AlbumDetailPageMobile extends StatefulWidget {
  const AlbumDetailPageMobile({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.onOpenArtist,
    this.onOpenAlbum,
  });

  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

  @override
  State<AlbumDetailPageMobile> createState() => _AlbumDetailPageMobileState();
}

class _AlbumDetailPageMobileState extends State<AlbumDetailPageMobile> {
  late final AlbumDetailController _controller;

  @override
  void initState() {
    super.initState();
    _controller = AlbumDetailController(
      client: widget.client,
      playbackCoordinator: widget.playbackCoordinator,
      itemId: widget.itemId,
      providerInstanceIdOrDomain: widget.providerInstanceIdOrDomain,
    )..addListener(_onChanged);
    unawaited(_controller.loadAlbum());
  }

  @override
  void dispose() {
    _controller
      ..removeListener(_onChanged)
      ..dispose();
    super.dispose();
  }

  void _onChanged() {
    if (mounted) setState(() {});
  }

  List<ContextMenuAction> _buildHeaderMenu(Album album) => <ContextMenuAction>[
    ContextMenuAction(
      label: 'Als Nächstes spielen',
      icon: Icons.keyboard_return,
      iconQuarterTurns: 2,
      onSelected: () =>
          widget.playbackCoordinator.playMediaNext(<MediaItem>[album]),
    ),
    ContextMenuAction(
      label: 'Als Letztes spielen',
      icon: Icons.last_page,
      onSelected: () =>
          widget.playbackCoordinator.playMediaLast(<MediaItem>[album]),
    ),
    ContextMenuAction(
      label: _controller.isAlbumFavorite
          ? 'Aus Favoriten entfernen'
          : 'Zu Favoriten hinzufügen',
      icon: _controller.isAlbumFavorite
          ? Icons.favorite_outlined
          : Icons.favorite_border_outlined,
      onSelected: _controller.toggleAlbumFavorite,
    ),
  ];

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final Album? album = _controller.album;
    final String? imageUri = album?.imageUri;
    return Container(
      color: colors.bgPrimary,
      child: SingleChildScrollView(
        padding: const EdgeInsets.only(bottom: 12),
        child: Column(
          children: <Widget>[
            SizedBox(
              height: 220,
              width: double.infinity,
              child: ClipRRect(
                borderRadius: const BorderRadius.vertical(
                  bottom: Radius.circular(16),
                ),
                child: Container(
                  color: colors.bgTertiary,
                  child: imageUri == null || imageUri.trim().isEmpty
                      ? Icon(
                          Icons.album_outlined,
                          color: colors.accent,
                          size: 72,
                        )
                      : Image.network(
                          imageUri,
                          fit: BoxFit.cover,
                          errorBuilder: (_, _, _) => Icon(
                            Icons.album_outlined,
                            color: colors.accent,
                            size: 72,
                          ),
                        ),
                ),
              ),
            ),
            const SizedBox(height: 8),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Text(
                _controller.albumName,
                textAlign: TextAlign.center,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.headlineSmall?.copyWith(
                  fontWeight: FontWeight.bold,
                ),
              ),
            ),
            const SizedBox(height: 4),
            Text(
              '${_controller.artistName}${_controller.yearText.isEmpty ? '' : ' • ${_controller.yearText}'}',
              style: TextStyle(color: colors.textSecondary),
            ),
            const SizedBox(height: 16),
            Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: <Widget>[
                IconButton(
                  onPressed: _controller.playAlbum,
                  tooltip: 'Abspielen',
                  icon: const Icon(Icons.play_arrow),
                ),
                IconButton(
                  onPressed: _controller.shuffleAlbum,
                  tooltip: 'Zufällig',
                  icon: const Icon(Icons.shuffle),
                ),
                IconButton(
                  onPressed: _controller.toggleAlbumFavorite,
                  tooltip: 'Favorit',
                  icon: Icon(
                    _controller.isAlbumFavorite
                        ? Icons.favorite
                        : Icons.favorite_border_outlined,
                  ),
                ),
                IconButton(
                  onPressed: album == null
                      ? null
                      : () => unawaited(
                          const AppContextMenuController().show(
                            context: context,
                            actions: _buildHeaderMenu(album),
                          ),
                        ),
                  tooltip: 'Weitere Optionen',
                  icon: const Icon(Icons.more_vert_outlined),
                ),
              ],
            ),
            if (_controller.hasDescription)
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
                child: GestureDetector(
                  onTap: _controller.toggleDescription,
                  child: Text(
                    _controller.description ?? '',
                    maxLines: _controller.descriptionMaxLines,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: colors.textSecondary),
                  ),
                ),
              ),
            const SizedBox(height: 16),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: MediaTableSectionMobile<Track>(
                title: 'Titel',
                isLoading: _controller.isLoadingTracks,
                items: _controller.tracks,
                emptyText: 'Keine Titel gefunden.',
                maxItemsPerPage: 0,
                playbackCoordinator: widget.playbackCoordinator,
              ),
            ),
            const SizedBox(height: 16),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: MediaRowSectionMobile<Album>(
                title: 'Weitere Alben',
                isLoading: _controller.isLoadingOtherAlbums,
                items: _controller.otherAlbums,
                emptyText: 'Keine weiteren Alben gefunden.',
                playbackCoordinator: widget.playbackCoordinator,
              ),
            ),
            const SizedBox(height: 16),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: MediaRowSectionMobile<Artist>(
                title: 'Ähnliche Interpreten',
                isLoading: _controller.isLoadingSimilarArtists,
                items: _controller.similarArtists,
                emptyText: 'Keine ähnlichen Interpreten gefunden.',
                playbackCoordinator: widget.playbackCoordinator,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
