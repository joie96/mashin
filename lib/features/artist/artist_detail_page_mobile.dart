import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_row_section_mobile.dart';
import '../shared/widgets/media_table_section_mobile.dart';
import '../shared/widgets/media_image.dart';
import 'artist_detail_controller.dart';
import '../shared/widgets/detail_provider_switcher.dart';

/// Mobile artist detail page: image banner header, centered actions, description and stacked album/similar-artist/top-track sections
class ArtistDetailPageMobile extends StatefulWidget {
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

  const ArtistDetailPageMobile({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
  });

  @override
  State<ArtistDetailPageMobile> createState() => _ArtistDetailPageMobileState();
}

class _ArtistDetailPageMobileState extends State<ArtistDetailPageMobile>
    with TickerProviderStateMixin {
  static const double _bannerHeight = 220;
  static const int _descriptionCollapsedMaxLines = 3;

  late final ArtistDetailController _controller;

  @override
  void initState() {
    super.initState();
    _controller = ArtistDetailController(
      coreBridge: widget.coreBridge,
      itemId: widget.itemId,
      providerInstanceIdOrDomain: widget.providerInstanceIdOrDomain,
      userDataController: widget.userDataController,
    )..addListener(_onControllerChanged);
    unawaited(_controller.loadArtist());
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

  List<ContextMenuAction> _buildPlaylistSubmenu(List<Track> tracks) {
    final UserData? controller = widget.userDataController;
    if (controller == null || tracks.isEmpty) {
      return const <ContextMenuAction>[];
    }

    return controller.playlists
        .map(
          (Playlist playlist) => ContextMenuAction(
            label: playlist.name,
            icon: Icons.playlist_play_outlined,
            onSelected: () =>
                controller.addPlaylistTracks(playlist.itemId, tracks),
          ),
        )
        .toList(growable: false);
  }

  List<ContextMenuAction> _buildItemMenuActions(
    MediaItem item, [
    List<MediaItem> selectedItems = const <MediaItem>[],
  ]) {
    final List<MediaItem> targets = selectedItems.isEmpty
        ? <MediaItem>[item]
        : selectedItems;
    final List<Track> tracks = targets.whereType<Track>().toList(
      growable: false,
    );
    final bool isSingleTarget = targets.length == 1;
    final bool allFavorite = targets.every(_controller.isItemFavorite);
    final List<ContextMenuAction> playlistActions = _buildPlaylistSubmenu(
      tracks,
    );
    final List<ContextMenuAction> actions = <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.play_arrow,
        onSelected: () => widget.coreBridge.queuePlayMedia(targets),
      ),
      if (item is Album || item is Playlist || item is Artist)
        ContextMenuAction(
          label: 'Zufällig abspielen',
          isEnabled: targets.any((target) => target.canPlay),
          icon: Icons.shuffle_outlined,
          onSelected: () =>
              widget.coreBridge.queuePlayMedia(targets, shuffle: true),
        ),
      ContextMenuAction(
        label: 'Radio starten',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.radio_outlined,
        onSelected: () => widget.coreBridge.startRadio(targets),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () =>
            widget.coreBridge.queuePlayMedia(targets, option: QueueOption.next),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.last_page,
        onSelected: () =>
            widget.coreBridge.queuePlayMedia(targets, option: QueueOption.add),
      ),
    ];

    if (isSingleTarget) {
      final Artist? primaryArtist = item is Artist
          ? item
          : _primaryArtistOf(item);
      final Album? primaryAlbum = item is Track ? item.album : null;
      if ((primaryArtist != null && widget.onOpenArtist != null) ||
          (primaryAlbum != null && widget.onOpenAlbum != null)) {
        actions.add(const ContextMenuAction.separator());
        if (primaryArtist != null && widget.onOpenArtist != null) {
          actions.add(
            ContextMenuAction(
              label: 'Künstler:in öffnen',
              icon: Icons.person_outline,
              onSelected: () => widget.onOpenArtist!(primaryArtist),
            ),
          );
        }
        if (primaryAlbum != null && widget.onOpenAlbum != null) {
          actions.add(
            ContextMenuAction(
              label: 'Album öffnen',
              icon: Icons.album_outlined,
              onSelected: () => widget.onOpenAlbum!(primaryAlbum),
            ),
          );
        }
      }
    }

    if (playlistActions.isNotEmpty) {
      actions
        ..add(const ContextMenuAction.separator())
        ..add(
          ContextMenuAction.submenu(
            label: 'Zu Wiedergabeliste hinzufügen',
            icon: Icons.playlist_add_outlined,
            children: playlistActions,
          ),
        );
    }
    actions
      ..add(const ContextMenuAction.separator())
      ..add(
        ContextMenuAction(
          label: allFavorite
              ? 'Aus Favoriten entfernen'
              : 'Zu Favoriten hinzufügen',
          icon: allFavorite
              ? Icons.favorite_outlined
              : Icons.favorite_border_outlined,
          onSelected: () async {
            for (final MediaItem target in targets) {
              if (_controller.isItemFavorite(target) == allFavorite) {
                await _controller.toggleItemFavorite(target);
              }
            }
          },
        ),
      );
    return actions;
  }

  List<ContextMenuAction> _buildHeaderMenuActions(Artist artist) {
    final bool isFavorite = _controller.isArtistFavorite;
    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Radio starten',
        icon: Icons.radio_outlined,
        onSelected: _controller.startArtistRadio,
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[
          artist,
        ], option: QueueOption.next),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[
          artist,
        ], option: QueueOption.add),
      ),
      const ContextMenuAction.separator(),
      ContextMenuAction(
        label: isFavorite
            ? 'Aus Favoriten entfernen'
            : 'Zu Favoriten hinzufügen',
        icon: isFavorite
            ? Icons.favorite_outlined
            : Icons.favorite_border_outlined,
        onSelected: _controller.toggleArtistFavorite,
      ),
    ];
  }

  Artist? _primaryArtistOf(MediaItem item) {
    final List<Artist> artists = switch (item) {
      Track track => track.artists,
      Album album => album.artists,
      _ => const <Artist>[],
    };
    for (final Artist artist in artists) {
      if (artist.itemId.trim().isNotEmpty &&
          artist.provider.trim().isNotEmpty) {
        return artist;
      }
    }
    return null;
  }

  Future<void> _showItemMenu(MediaItem item) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      actions: _buildItemMenuActions(item),
    );
  }

  Future<void> _showSelectionMenu(List<MediaItem> items) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      actions: _buildItemMenuActions(items.first, items),
    );
  }

  Future<void> _showHeaderMenu(Artist artist) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      actions: _buildHeaderMenuActions(artist),
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Container(
      color: colors.bgPrimary,
      child: CustomScrollView(
        slivers: <Widget>[
          SliverPadding(
            padding: const EdgeInsets.only(bottom: 12),
            sliver: SliverToBoxAdapter(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  _buildBanner(colors),
                  const SizedBox(height: 4),
                  _buildNameAndMeta(context, colors),
                  const SizedBox(height: 16),
                  _buildActions(colors),
                  if (_controller.isLoadingMetadata ||
                      _controller.hasDescription) ...<Widget>[
                    const SizedBox(height: 16),
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 16),
                      child: _buildDescription(context, colors),
                    ),
                  ],
                ],
              ),
            ),
          ),
          const SliverToBoxAdapter(child: SizedBox(height: 32)),
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: MediaRowSectionMobile<Album>(
                title: 'Veröffentlichungen',
                isLoading: _controller.isLoadingAlbums,
                items: _controller.albums,
                emptyText: 'Keine Veröffentlichungen gefunden.',
                showAlbumArtist: false,
                coreBridge: widget.coreBridge,
                coreEvents: widget.coreEvents,
                onItemTap: widget.onOpenAlbum,
                onSelectionMenuTap: (List<Album> items) =>
                    unawaited(_showSelectionMenu(items)),
              ),
            ),
          ),
          const SliverToBoxAdapter(child: SizedBox(height: 32)),
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: MediaRowSectionMobile<Artist>(
                title: 'Ähnliche Interpreten',
                isLoading: _controller.isLoadingSimilarArtists,
                items: _controller.similarArtists,
                emptyText: 'Keine ähnlichen Interpreten gefunden.',
                coreBridge: widget.coreBridge,
                coreEvents: widget.coreEvents,
                onItemTap: widget.onOpenArtist,
                onSelectionMenuTap: (List<Artist> items) =>
                    unawaited(_showSelectionMenu(items)),
              ),
            ),
          ),
          const SliverToBoxAdapter(child: SizedBox(height: 32)),
          SliverPadding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            sliver: MediaTableSectionMobile<Track>(
              title: 'Top Tracks',
              isLoading: _controller.isLoadingTopTracks,
              items: _controller.topTracks,
              emptyText: 'Keine Top Tracks gefunden.',
              maxItemsPerPage: 0,
              coreBridge: widget.coreBridge,
              coreEvents: widget.coreEvents,
              onRowTap: (Track item, int index) => _showItemMenu(item),
              onSelectionMenuTap: (List<Track> items) =>
                  unawaited(_showSelectionMenu(items)),
            ),
          ),
          const SliverToBoxAdapter(child: SizedBox(height: 12)),
        ],
      ),
    );
  }

  Widget _buildBanner(MashinColors colors) {
    final Artist? artist = _controller.artist;
    final String? imageUri = artist?.imageUri;
    final bool hasImage =
        !_controller.isLoadingMetadata &&
        imageUri != null &&
        imageUri.trim().isNotEmpty;

    return SizedBox(
      height: _bannerHeight,
      width: double.infinity,
      child: ClipRRect(
        borderRadius: const BorderRadius.only(
          bottomLeft: Radius.circular(16),
          bottomRight: Radius.circular(16),
        ),
        child: Container(
          color: colors.bgPrimary,
          child: Stack(
            fit: StackFit.expand,
            children: <Widget>[
              if (hasImage)
                MediaImage(
                  source: imageUri,
                  fit: BoxFit.cover,
                  fallback: Icon(
                    Icons.person_outlined,
                    color: colors.textSecondary,
                    size: 72,
                  ),
                )
              else if (_controller.isLoadingMetadata)
                Icon(
                  Icons.person_outlined,
                  color: colors.textSecondary,
                  size: 72,
                )
              else if (!_controller.isLoadingMetadata)
                Icon(
                  Icons.person_outlined,
                  color: colors.textSecondary,
                  size: 72,
                ),
              if (hasImage)
                Positioned.fill(
                  child: IgnorePointer(
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        gradient: LinearGradient(
                          begin: Alignment.topCenter,
                          end: Alignment.bottomCenter,
                          colors: <Color>[Colors.transparent, colors.bgPrimary],
                          stops: const <double>[0.44, 1],
                        ),
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildNameAndMeta(BuildContext context, MashinColors colors) {
    final Artist? artist = _controller.artist;

    if (_controller.isLoadingMetadata) {
      return SizedBox(
        width: double.infinity,
        child: Column(
          children: <Widget>[
            Container(
              width: 200,
              height: 24,
              margin: const EdgeInsets.symmetric(horizontal: 16),
              decoration: BoxDecoration(
                color: colors.bgTertiary,
                borderRadius: BorderRadius.circular(6),
              ),
            ),
            const SizedBox(height: 4),
            Container(
              width: 64,
              height: 12,
              decoration: BoxDecoration(
                color: colors.bgTertiary,
                borderRadius: BorderRadius.circular(6),
              ),
            ),
          ],
        ),
      );
    }

    return Column(
      children: <Widget>[
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            crossAxisAlignment: CrossAxisAlignment.center,
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
                Flexible(
                  child: DetailProviderSwitcher(
                    selection: _controller.sectionProvider,
                    item: artist,
                    isLoading: _controller.isLoadingSections,
                    onSelected: _controller.selectProvider,
                  ),
                ),
              ],
            ],
          ),
        ),
        const SizedBox(height: 4),
        Text(
          '${_controller.albums.length} Alben',
          style: TextStyle(color: colors.textSecondary, fontSize: 12),
        ),
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
          onTap: _controller.isLoadingMetadata
              ? null
              : _controller.shuffleArtist,
        ),
        const SizedBox(width: 14),
        _MobileActionButton(
          icon: Icons.radio_outlined,
          onTap: _controller.isLoadingMetadata
              ? null
              : _controller.startArtistRadio,
        ),
        const SizedBox(width: 14),
        _MobileActionButton(
          icon: _controller.isArtistFavorite
              ? Icons.favorite
              : Icons.favorite_border_outlined,
          onTap: _controller.isLoadingMetadata
              ? null
              : _controller.toggleArtistFavorite,
        ),
        const SizedBox(width: 14),
        _MobileActionButton(
          icon: Icons.more_vert_outlined,
          onTap: _controller.artist == null
              ? null
              : () => _showHeaderMenu(_controller.artist!),
        ),
      ],
    );
  }

  Widget _buildDescription(BuildContext context, MashinColors colors) {
    if (_controller.isLoadingMetadata) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          _buildSkeletonLine(colors, width: 160, height: 14),
          const SizedBox(height: 6),
          _buildSkeletonLine(colors, width: double.infinity, height: 13),
          const SizedBox(height: 6),
          _buildSkeletonLine(colors, width: double.infinity, height: 13),
          const SizedBox(height: 6),
          _buildSkeletonLine(colors, width: 220, height: 13),
        ],
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text(
          'Über ${_controller.artistName}',
          style: TextStyle(color: colors.textPrimary, fontSize: 14),
        ),
        const SizedBox(height: 6),
        ListenableBuilder(
          listenable: _controller.metadataListenable,
          builder: (BuildContext context, Widget? child) {
            final TextStyle descriptionStyle = TextStyle(
              color: colors.textSecondary,
              fontSize: 13,
            );
            final String description = _controller.description ?? '';
            final bool isExpanded = _controller.isDescriptionExpanded;

            return LayoutBuilder(
              builder: (BuildContext context, BoxConstraints constraints) {
                final TextPainter textPainter = TextPainter(
                  text: TextSpan(text: description, style: descriptionStyle),
                  textDirection: Directionality.of(context),
                  textScaler: MediaQuery.textScalerOf(context),
                  maxLines: _descriptionCollapsedMaxLines,
                )..layout(maxWidth: constraints.maxWidth);
                final bool canExpand = textPainter.didExceedMaxLines;

                return Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: <Widget>[
                    AnimatedSize(
                      duration: const Duration(milliseconds: 260),
                      curve: Curves.easeInOutCubic,
                      alignment: Alignment.topCenter,
                      clipBehavior: Clip.hardEdge,
                      child: GestureDetector(
                        onTap: canExpand ? _controller.toggleDescription : null,
                        child: SizedBox(
                          width: constraints.maxWidth,
                          child: Text(
                            description,
                            maxLines: isExpanded
                                ? 1000
                                : _descriptionCollapsedMaxLines,
                            overflow: TextOverflow.ellipsis,
                            softWrap: true,
                            textWidthBasis: TextWidthBasis.parent,
                            style: descriptionStyle,
                          ),
                        ),
                      ),
                    ),
                    if (canExpand)
                      Center(
                        child: Material(
                          color: Colors.transparent,
                          borderRadius: BorderRadius.circular(12),
                          child: InkWell(
                            onTap: _controller.toggleDescription,
                            borderRadius: BorderRadius.circular(12),
                            child: SizedBox(
                              width: 24,
                              height: 24,
                              child: AnimatedRotation(
                                turns: isExpanded ? 0.5 : 0,
                                duration: const Duration(milliseconds: 220),
                                curve: Curves.easeOutCubic,
                                child: Icon(
                                  Icons.expand_more_outlined,
                                  size: 14,
                                  color: colors.textPrimary,
                                ),
                              ),
                            ),
                          ),
                        ),
                      ),
                  ],
                );
              },
            );
          },
        ),
      ],
    );
  }

  Widget _buildSkeletonLine(
    MashinColors colors, {
    required double width,
    required double height,
  }) {
    return Container(
      width: width,
      height: height,
      decoration: BoxDecoration(
        color: colors.bgTertiary,
        borderRadius: BorderRadius.circular(6),
      ),
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
      color: Colors.transparent,
      shape: const CircleBorder(),
      child: InkWell(
        customBorder: const CircleBorder(),
        onTap: onTap,
        highlightColor: Colors.transparent,
        splashColor: colors.selection,
        child: SizedBox(
          width: 44,
          height: 44,
          child: Icon(icon, color: colors.textPrimary, size: 20),
        ),
      ),
    );
  }
}
