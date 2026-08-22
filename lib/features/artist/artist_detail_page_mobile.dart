import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/widgets/media_row_section_mobile.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import '../shared/widgets/media_table_section_mobile.dart';
import 'artist_detail_controller.dart';

/// Mobile artist detail page: image banner header, centered actions, description and stacked album/similar-artist/top-track sections
class ArtistDetailPageMobile extends StatefulWidget {
  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final ValueChanged<Artist>? onOpenArtist;

  const ArtistDetailPageMobile({
    super.key,
    required this.client,
    required this.playbackCoordinator,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.onOpenArtist,
  });

  @override
  State<ArtistDetailPageMobile> createState() => _ArtistDetailPageMobileState();
}

class _ArtistDetailPageMobileState extends State<ArtistDetailPageMobile> {
  static const double _bannerHeight = 220;

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

    return Container(
      color: colors.bgPrimary,
      child: SingleChildScrollView(
        padding: const EdgeInsets.only(bottom: 12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            _buildBanner(colors),
            const SizedBox(height: 4),
            _buildNameAndMeta(context, colors),
            const SizedBox(height: 16),
            _buildActions(colors),
            if (_controller.isLoadingMetadata || _controller.hasDescription) ...<Widget>[
              const SizedBox(height: 16),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: _buildDescription(context, colors),
              ),
            ],
            const SizedBox(height: 16),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: MediaRowSectionMobile<Album>(
                title: 'Veröffentlichungen',
                isLoading: _controller.isLoadingAlbums,
                items: _controller.albums,
                emptyText: 'Keine Veröffentlichungen gefunden.',
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
            const SizedBox(height: 16),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: MediaTableSectionMobile<Track>(
                title: 'Top Tracks',
                isLoading: _controller.isLoadingTopTracks,
                items: _controller.topTracks,
                emptyText: 'Keine Top Tracks gefunden.',
                maxItemsPerPage: 0,
                playbackCoordinator: widget.playbackCoordinator,
                onRowTap: (Track item, int index) => _showItemMenu(item),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildBanner(MashinColors colors) {
    final Artist? artist = _controller.artist;
    final String? imageUri = artist?.imageUri;
    final bool hasImage = !_controller.isLoadingMetadata && imageUri != null && imageUri.trim().isNotEmpty;

    return SizedBox(
      height: _bannerHeight,
      width: double.infinity,
      child: ClipRRect(
        borderRadius: const BorderRadius.only(bottomLeft: Radius.circular(16), bottomRight: Radius.circular(16)),
        child: Container(
          color: colors.bgTertiary,
          child: hasImage
              ? Image.network(
                  imageUri,
                  fit: BoxFit.cover,
                  errorBuilder: (_, _, _) => Icon(Icons.person_outlined, color: colors.accent, size: 72),
                )
              : _controller.isLoadingMetadata
                  ? null
                  : Icon(Icons.person_outlined, color: colors.accent, size: 72),
        ),
      ),
    );
  }

  Widget _buildNameAndMeta(BuildContext context, MashinColors colors) {
    final Artist? artist = _controller.artist;

    if (_controller.isLoadingMetadata) {
      return Center(
        child: Container(
          width: 200,
          height: 24,
          margin: const EdgeInsets.symmetric(horizontal: 16),
          decoration: BoxDecoration(color: colors.bgTertiary, borderRadius: BorderRadius.circular(6)),
        ),
      );
    }

    return Column(
      children: <Widget>[
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: <Widget>[
              Flexible(
                child: Text(
                  _controller.artistName,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  textAlign: TextAlign.center,
                  style: Theme.of(context).textTheme.headlineSmall,
                ),
              ),
              if (artist != null) ...<Widget>[
                const SizedBox(width: 8),
                MediaProviderIcon(
                  providerLabel: artist.provider,
                  iconSvg: ProviderManifestRegistry.iconSvgForProvider(artist.provider),
                  size: 20,
                ),
              ],
            ],
          ),
        ),
        const SizedBox(height: 4),
        Text('${_controller.albums.length} Alben', style: TextStyle(color: colors.textSecondary, fontSize: 12)),
      ],
    );
  }

  Widget _buildActions(MashinColors colors) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: <Widget>[
        _MobileActionButton(
          icon: Icons.play_arrow,
          onTap: _controller.isLoadingMetadata ? null : _controller.playArtist,
        ),
        const SizedBox(width: 14),
        _MobileActionButton(
          icon: Icons.shuffle,
          onTap: _controller.isLoadingMetadata ? null : _controller.shuffleArtist,
        ),
        const SizedBox(width: 14),
        _MobileActionButton(
          icon: _controller.isArtistFavorite ? Icons.favorite : Icons.favorite_border_outlined,
          onTap: _controller.isLoadingMetadata ? null : _controller.toggleArtistFavorite,
        ),
        const SizedBox(width: 14),
        _MobileActionButton(
          icon: Icons.more_vert_outlined,
          onTap: _controller.artist == null ? null : () => _showItemMenu(_controller.artist!),
        ),
      ],
    );
  }

  Widget _buildDescription(BuildContext context, MashinColors colors) {
    if (_controller.isLoadingMetadata) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Container(width: 160, height: 14, color: colors.bgTertiary),
          const SizedBox(height: 8),
          Container(width: double.infinity, height: 12, color: colors.bgTertiary),
        ],
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text('Über ${_controller.artistName}', style: TextStyle(color: colors.textPrimary, fontSize: 14)),
        const SizedBox(height: 6),
        GestureDetector(
          onTap: _controller.toggleDescription,
          child: Text(
            _controller.description ?? '',
            maxLines: _controller.descriptionMaxLines,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(color: colors.textSecondary, fontSize: 13),
          ),
        ),
        Center(
          child: IconButton(
            onPressed: _controller.toggleDescription,
            icon: Icon(_controller.isDescriptionExpanded ? Icons.expand_less_outlined : Icons.expand_more_outlined),
          ),
        ),
      ],
    );
  }
}

class _MobileActionButton extends StatelessWidget {
  final IconData icon;
  final VoidCallback? onTap;

  const _MobileActionButton({required this.icon, required this.onTap});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Material(
      color: colors.bgSecondary,
      shape: const CircleBorder(),
      child: InkWell(
        customBorder: const CircleBorder(),
        onTap: onTap,
        child: SizedBox(
          width: 44,
          height: 44,
          child: Icon(icon, color: colors.textPrimary, size: 20),
        ),
      ),
    );
  }
}
