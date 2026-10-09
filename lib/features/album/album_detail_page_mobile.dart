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
import 'album_detail_controller.dart';

class AlbumDetailPageMobile extends StatefulWidget {
  const AlbumDetailPageMobile({
    super.key,
    required this.coreBridge,
    required this.coreEvents,
    required this.itemId,
    required this.providerInstanceIdOrDomain,
    this.userDataController,
    this.onOpenArtist,
    this.onOpenAlbum,
  });

  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final String itemId;
  final String providerInstanceIdOrDomain;
  final UserData? userDataController;
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
      coreBridge: widget.coreBridge,
      itemId: widget.itemId,
      providerInstanceIdOrDomain: widget.providerInstanceIdOrDomain,
      userDataController: widget.userDataController,
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
          onSelected: () => widget.coreBridge.queuePlayMedia(targets, shuffle: true),
        ),
      ContextMenuAction(
        label: 'Radio starten',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.radio_outlined,
        onSelected: () => widget.coreBridge.startSendspinRadio(targets),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
          onSelected: () => widget.coreBridge.queuePlayMedia(targets, option: QueueOption.next),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        isEnabled: targets.any((target) => target.canPlay),
        icon: Icons.last_page,
          onSelected: () => widget.coreBridge.queuePlayMedia(targets, option: QueueOption.add),
      ),
    ];

    if (isSingleTarget) {
      final Artist? primaryArtist = _primaryArtistOf(item);
      final Album? primaryAlbum = item is Track
          ? item.album
          : (item is Album ? item : null);
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

    final List<ContextMenuAction> playlistActions = _buildPlaylistSubmenu(
      tracks,
    );
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

  Future<void> _showSelectionMenu(List<MediaItem> items) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      actions: _buildItemMenuActions(items.first, items),
    );
  }

  List<ContextMenuAction> _buildHeaderMenu(Album album) => <ContextMenuAction>[
    ContextMenuAction(
      label: 'Als Nächstes spielen',
      icon: Icons.keyboard_return,
      iconQuarterTurns: 2,
      onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[album], option: QueueOption.next),
    ),
    ContextMenuAction(
      label: 'Als Letztes spielen',
      icon: Icons.last_page,
      onSelected: () => widget.coreBridge.queuePlayMedia(<MediaItem>[album], option: QueueOption.add),
    ),
    const ContextMenuAction.separator(),
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
    final bool isLoadingMetadata = _controller.isLoadingMetadata;
    final bool hasImage = imageUri != null && imageUri.trim().isNotEmpty;
    return Container(
      color: colors.bgPrimary,
      child: CustomScrollView(
        slivers: <Widget>[
          SliverPadding(
            padding: const EdgeInsets.only(bottom: 12),
            sliver: SliverToBoxAdapter(
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
                        color: colors.bgPrimary,
                        child: Stack(
                          fit: StackFit.expand,
                          children: <Widget>[
                            isLoadingMetadata
                                ? Icon(
                                    Icons.album_outlined,
                                    color: colors.textSecondary,
                                    size: 72,
                                  )
                                : !hasImage
                                ? Icon(
                                    Icons.album_outlined,
                                    color: colors.textSecondary,
                                    size: 72,
                                  )
                                : MediaImage(
                                    source: imageUri,
                                    fit: BoxFit.cover,
                                    fallback: Icon(
                                      Icons.album_outlined,
                                      color: colors.textSecondary,
                                      size: 72,
                                    ),
                                  ),
                            if (hasImage)
                              Positioned.fill(
                                child: IgnorePointer(
                                  child: DecoratedBox(
                                    decoration: BoxDecoration(
                                      gradient: LinearGradient(
                                        begin: Alignment.topCenter,
                                        end: Alignment.bottomCenter,
                                        colors: <Color>[
                                          Colors.transparent,
                                          colors.bgPrimary,
                                        ],
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
                  ),
                  const SizedBox(height: 8),
                  if (isLoadingMetadata)
                    _buildSkeletonLine(colors, width: 200, height: 24)
                  else
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 16),
                      child: Text(
                        _controller.albumName,
                        textAlign: TextAlign.center,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context).textTheme.headlineSmall
                            ?.copyWith(fontWeight: FontWeight.bold),
                      ),
                    ),
                  const SizedBox(height: 4),
                  if (isLoadingMetadata)
                    _buildSkeletonLine(colors, width: 120, height: 14)
                  else
                    Text(
                      '${_controller.artistName}${_controller.yearText.isEmpty ? '' : ' • ${_controller.yearText}'}',
                      style: TextStyle(color: colors.textSecondary),
                    ),
                  const SizedBox(height: 16),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: <Widget>[
                      _MobileActionButton(
                        icon: Icons.play_arrow,
                        onTap: isLoadingMetadata ? null : _controller.playAlbum,
                      ),
                      const SizedBox(width: 14),
                      _MobileActionButton(
                        icon: Icons.shuffle,
                        onTap: isLoadingMetadata
                            ? null
                            : _controller.shuffleAlbum,
                      ),
                      const SizedBox(width: 14),
                      _MobileActionButton(
                        icon: _controller.isAlbumFavorite
                            ? Icons.favorite
                            : Icons.favorite_border_outlined,
                        onTap: isLoadingMetadata
                            ? null
                            : _controller.toggleAlbumFavorite,
                      ),
                      const SizedBox(width: 14),
                      _MobileActionButton(
                        icon: Icons.more_vert_outlined,
                        onTap: album == null
                            ? null
                            : () => unawaited(
                                const AppContextMenuController()
                                    .showContextMenu(
                                      context: context,
                                      actions: _buildHeaderMenu(album),
                                    ),
                              ),
                      ),
                    ],
                  ),
                  if (isLoadingMetadata || _controller.hasDescription)
                    Padding(
                      padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
                      child: isLoadingMetadata
                          ? Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: <Widget>[
                                _buildSkeletonLine(
                                  colors,
                                  width: double.infinity,
                                  height: 13,
                                ),
                                const SizedBox(height: 6),
                                _buildSkeletonLine(
                                  colors,
                                  width: double.infinity,
                                  height: 13,
                                ),
                                const SizedBox(height: 6),
                                _buildSkeletonLine(
                                  colors,
                                  width: 220,
                                  height: 13,
                                ),
                              ],
                            )
                          : GestureDetector(
                              onTap: _controller.toggleDescription,
                              child: Text(
                                _controller.description ?? '',
                                maxLines: _controller.descriptionMaxLines,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(color: colors.textSecondary),
                              ),
                            ),
                    ),
                ],
              ),
            ),
          ),
          SliverPadding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            sliver: MediaTableSectionMobile<Track>(
              title: 'Titel',
              isLoading: _controller.isLoadingTracks,
              items: _controller.tracks,
              emptyText: 'Keine Titel gefunden.',
              maxItemsPerPage: 0,
              coreBridge: widget.coreBridge,
              coreEvents: widget.coreEvents,
              onSelectionMenuTap: (List<Track> items) =>
                  unawaited(_showSelectionMenu(items)),
            ),
          ),
          const SliverToBoxAdapter(child: SizedBox(height: 16)),
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: MediaRowSectionMobile<Album>(
                title: 'Weitere Alben',
                isLoading: _controller.isLoadingOtherAlbums,
                items: _controller.otherAlbums,
                emptyText: 'Keine weiteren Alben gefunden.',
                coreBridge: widget.coreBridge,
                coreEvents: widget.coreEvents,
                onItemTap: widget.onOpenAlbum,
                onSelectionMenuTap: (List<Album> items) =>
                    unawaited(_showSelectionMenu(items)),
              ),
            ),
          ),
          const SliverToBoxAdapter(child: SizedBox(height: 16)),
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
          const SliverToBoxAdapter(child: SizedBox(height: 12)),
        ],
      ),
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
