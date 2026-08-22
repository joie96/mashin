import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/widgets/media_row_section_desktop.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import 'artist_detail_controller.dart';
import 'artist_detail_page.dart';

/// Desktop artist detail page: header with cover/name/actions, description and
/// paginated album/similar-artist rows plus a top-tracks table, mirroring the
/// MAUI Views/Desktop/ArtistDetailPage layout.
class ArtistDetailPageDesktop extends StatefulWidget {
  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final String itemId;
  final String providerInstanceIdOrDomain;

  const ArtistDetailPageDesktop({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
  });

  @override
  State<ArtistDetailPageDesktop> createState() => _ArtistDetailPageDesktopState();
}

class _ArtistDetailPageDesktopState extends State<ArtistDetailPageDesktop> {
  static const double _coverSize = 200;

  late final ArtistDetailController _controller;

  @override
  void initState() {
    super.initState();
    _controller = ArtistDetailController(
      client: widget.client,
      playbackCoordinator: widget.playbackCoordinator,
      itemId: widget.itemId,
      providerInstanceIdOrDomain: widget.providerInstanceIdOrDomain,
    )..addListener(_onControllerChanged);
    unawaited(_controller.load());
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
    super.dispose();
  }

  void _showItemMenu(MediaItem item) {
    showModalBottomSheet<void>(
      context: context,
      builder: (BuildContext sheetContext) {
        final bool isFavorite = _controller.isItemFavorite(item);
        return SafeArea(
          child: Wrap(
            children: <Widget>[
              ListTile(
                leading: const Icon(Icons.play_arrow_outlined),
                title: const Text('Abspielen'),
                onTap: () {
                  Navigator.of(sheetContext).pop();
                  unawaited(widget.playbackCoordinator.playMedia(<MediaItem>[item]));
                },
              ),
              ListTile(
                leading: Icon(isFavorite ? Icons.favorite_outlined : Icons.favorite_border_outlined),
                title: Text(isFavorite ? 'Favorit entfernen' : 'Als Favorit markieren'),
                onTap: () {
                  Navigator.of(sheetContext).pop();
                  unawaited(_controller.toggleItemFavorite(item));
                },
              ),
            ],
          ),
        );
      },
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Scaffold(
      backgroundColor: colors.bgPrimary,
      appBar: AppBar(title: Text(_controller.artistName)),
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            _buildHeader(context, colors),
            if (_controller.isLoadingMetadata || _controller.hasDescription) ...<Widget>[
              const SizedBox(height: 32),
              _buildDescription(context, colors),
            ],
            const SizedBox(height: 32),
            MediaRowSectionDesktop<Album>(
              title: 'Veröffentlichungen',
              isLoading: _controller.isLoadingAlbums,
              items: _controller.albums,
              emptyText: 'Keine Veröffentlichungen gefunden.',
              playbackCoordinator: widget.playbackCoordinator,
              isRowFavorite: (Album item, int index) => _controller.isItemFavorite(item),
              onRowFavoriteTap: (Album item, int index) => _controller.toggleItemFavorite(item),
              onRowMoreTap: (Album item, int index) => _showItemMenu(item),
              onArtistTap: (Artist artist) => ArtistDetailPage.push(
                context,
                client: widget.client,
                playbackCoordinator: widget.playbackCoordinator,
                artist: artist,
              ),
            ),
            const SizedBox(height: 32),
            MediaRowSectionDesktop<Artist>(
              title: 'Ähnliche Interpreten',
              isLoading: _controller.isLoadingSimilarArtists,
              items: _controller.similarArtists,
              emptyText: 'Keine ähnlichen Interpreten gefunden.',
              playbackCoordinator: widget.playbackCoordinator,
              isRowFavorite: (Artist item, int index) => _controller.isItemFavorite(item),
              onRowFavoriteTap: (Artist item, int index) => _controller.toggleItemFavorite(item),
              onRowMoreTap: (Artist item, int index) => _showItemMenu(item),
            ),
            const SizedBox(height: 32),
            MediaTableSectionDesktop<Track>(
              title: 'Toplieder',
              isLoading: _controller.isLoadingTopTracks,
              items: _controller.topTracks,
              emptyText: 'Keine Toplieder gefunden.',
              maxItemsPerPage: 15,
              playbackCoordinator: widget.playbackCoordinator,
              isRowFavorite: (Track item, int index) => _controller.isItemFavorite(item),
              onRowFavoriteTap: (Track item, int index) => _controller.toggleItemFavorite(item),
              onRowMoreTap: (Track item, int index) => _showItemMenu(item),
              onArtistTap: (Artist artist) => ArtistDetailPage.push(
                context,
                client: widget.client,
                playbackCoordinator: widget.playbackCoordinator,
                artist: artist,
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildHeader(BuildContext context, MashinColors colors) {
    final Artist? artist = _controller.artist;
    final bool isLoading = _controller.isLoadingMetadata;

    return SizedBox(
      height: _coverSize,
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          _buildArtistImage(colors, artist, isLoading),
          const SizedBox(width: 30),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisAlignment: MainAxisAlignment.end,
              children: <Widget>[
                Text(
                  'ARTIST/INTERPRET',
                  style: TextStyle(color: colors.textSecondary, fontWeight: FontWeight.bold, fontSize: 12),
                ),
                const SizedBox(height: 16),
                isLoading
                    ? _buildSkeletonLine(colors, width: 320, height: 32)
                    : Row(
                        crossAxisAlignment: CrossAxisAlignment.center,
                        children: <Widget>[
                          Flexible(
                            child: Text(
                              _controller.artistName,
                              maxLines: 2,
                              overflow: TextOverflow.ellipsis,
                              style: Theme.of(context).textTheme.headlineMedium?.copyWith(fontWeight: FontWeight.bold),
                            ),
                          ),
                          if (artist != null) ...<Widget>[
                            const SizedBox(width: 16),
                            MediaProviderIcon(
                              providerLabel: artist.provider,
                              iconSvg: ProviderManifestRegistry.iconSvgForProvider(artist.provider),
                              size: 28,
                            ),
                          ],
                        ],
                      ),
                const SizedBox(height: 8),
                isLoading
                    ? _buildSkeletonLine(colors, width: 150, height: 14)
                    : Text('${_controller.albums.length} Alben', style: TextStyle(color: colors.textSecondary)),
                const SizedBox(height: 16),
                if (!isLoading)
                  Row(
                    children: <Widget>[
                      _HeaderActionButton(icon: Icons.play_arrow, label: 'Abspielen', onTap: _controller.playArtist),
                      const SizedBox(width: 12),
                      _HeaderActionButton(icon: Icons.shuffle, label: 'Zufällig', onTap: _controller.shuffleArtist),
                      const SizedBox(width: 12),
                      IconButton(
                        tooltip: _controller.isArtistFavorite ? 'Favorit entfernen' : 'Als Favorit markieren',
                        onPressed: _controller.toggleArtistFavorite,
                        icon: Icon(
                          _controller.isArtistFavorite ? Icons.favorite_outlined : Icons.favorite_border_outlined,
                        ),
                      ),
                      IconButton(
                        tooltip: 'Weitere Optionen',
                        onPressed: artist == null ? null : () => _showItemMenu(artist),
                        icon: const Icon(Icons.more_horiz_outlined),
                      ),
                    ],
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildArtistImage(MashinColors colors, Artist? artist, bool isLoading) {
    if (isLoading) {
      return ClipOval(
        child: Container(width: _coverSize, height: _coverSize, color: colors.bgTertiary),
      );
    }

    final String? imageUri = artist?.imageUri;
    return ClipOval(
      child: Container(
        width: _coverSize,
        height: _coverSize,
        color: colors.bgTertiary,
        child: imageUri == null || imageUri.trim().isEmpty
            ? Icon(Icons.person_outlined, color: colors.accent, size: 72)
            : Image.network(
                imageUri,
                fit: BoxFit.cover,
                errorBuilder: (_, _, _) => Icon(Icons.person_outlined, color: colors.accent, size: 72),
              ),
      ),
    );
  }

  Widget _buildDescription(BuildContext context, MashinColors colors) {
    if (_controller.isLoadingMetadata) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          _buildSkeletonLine(colors, width: 200, height: 18),
          const SizedBox(height: 16),
          _buildSkeletonLine(colors, width: double.infinity, height: 14),
          const SizedBox(height: 8),
          _buildSkeletonLine(colors, width: double.infinity, height: 14),
        ],
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text(
          'Über ${_controller.artistName}',
          style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
        ),
        const SizedBox(height: 16),
        GestureDetector(
          onTap: _controller.toggleDescription,
          child: Text(
            _controller.description ?? '',
            maxLines: _controller.descriptionMaxLines,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(color: colors.textSecondary),
          ),
        ),
        const SizedBox(height: 8),
        Center(
          child: IconButton(
            onPressed: _controller.toggleDescription,
            icon: Icon(_controller.isDescriptionExpanded ? Icons.expand_less_outlined : Icons.expand_more_outlined),
          ),
        ),
      ],
    );
  }

  Widget _buildSkeletonLine(MashinColors colors, {required double width, required double height}) {
    return Container(
      width: width,
      height: height,
      decoration: BoxDecoration(color: colors.bgTertiary, borderRadius: BorderRadius.circular(6)),
    );
  }
}

class _HeaderActionButton extends StatelessWidget {
  final IconData icon;
  final String label;
  final VoidCallback onTap;

  const _HeaderActionButton({required this.icon, required this.label, required this.onTap});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Material(
      color: colors.bgSecondary,
      borderRadius: BorderRadius.circular(8),
      child: InkWell(
        borderRadius: BorderRadius.circular(8),
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: <Widget>[
              Icon(icon, size: 18, color: colors.textPrimary),
              const SizedBox(width: 8),
              Text(label, style: TextStyle(color: colors.textPrimary, fontWeight: FontWeight.w600)),
            ],
          ),
        ),
      ),
    );
  }
}
