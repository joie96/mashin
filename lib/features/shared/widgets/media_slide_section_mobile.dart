import 'dart:async';

import 'package:flutter/material.dart';
import 'detail_provider_switcher.dart';

import '../../../app/theme/app_theme.dart';
import '../../core/core_bridge.dart';
import '../../core/core_models.dart';
import '../../core/core_events.dart';
import '../selection/mobile_section_selection.dart';
import 'media_image.dart';
import 'media_table_section_desktop.dart';
import 'mobile_section_header.dart';

class MediaSlideSectionMobile<T> extends StatefulWidget {
  final String title;
  final List<T> items;
  final bool isLoading;
  final String emptyText;
  final double itemWidth;
  final double itemHeight;
  final double coverSize;
  final double itemSpacing;
  final int skeletonCount;
  final MashinCoreBridgeApi? coreBridge;
  final MashinCoreBridgeEvents? coreEvents;
  final ValueChanged<T>? onItemTap;
  final ValueChanged<List<T>>? onSelectionMenuTap;

  const MediaSlideSectionMobile({
    super.key,
    required this.title,
    required this.items,
    required this.isLoading,
    required this.emptyText,
    this.itemWidth = 320,
    this.itemHeight = 112,
    this.coverSize = 112,
    this.itemSpacing = 8,
    this.skeletonCount = 4,
    this.coreBridge,
    this.coreEvents,
    this.onItemTap,
    this.onSelectionMenuTap,
  });

  @override
  State<MediaSlideSectionMobile<T>> createState() =>
      _MediaSlideSectionMobileState<T>();
}

class _MediaSlideSectionMobileState<T>
    extends State<MediaSlideSectionMobile<T>> {
  static const double _mobileChunkViewportFraction = 0.92;
  static const double _mobileChunkGap = 8;

  final PageController _mobileChunkPageController = PageController(
    viewportFraction: _mobileChunkViewportFraction,
  );
  late final MobileSectionSelectionState _selectionController;

  @override
  void initState() {
    super.initState();
    _selectionController = MobileSectionSelectionState(
      itemType: T,
      onMenuTap: _showSelectionMenu,
    );
    _selectionController.addListener(_onSelectionChanged);
  }

  @override
  void didUpdateWidget(covariant MediaSlideSectionMobile<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    _selectionController.updateItemCount(widget.items.length);
  }

  @override
  void dispose() {
    _mobileChunkPageController.dispose();
    _selectionController
      ..removeListener(_onSelectionChanged)
      ..disposeSelection()
      ..dispose();
    super.dispose();
  }

  void _onSelectionChanged() {
    if (mounted) {
      setState(() {});
    }
  }

  void _showSelectionMenu() {
    final ValueChanged<List<T>>? onSelectionMenuTap = widget.onSelectionMenuTap;
    if (onSelectionMenuTap == null) {
      return;
    }
    final List<T> selectedItems = _selectionController.selectedIndexes
        .where((int index) => index >= 0 && index < widget.items.length)
        .map((int index) => widget.items[index])
        .toList(growable: false);
    if (selectedItems.isNotEmpty) {
      onSelectionMenuTap(selectedItems);
    }
  }

  void _playItem(int index) {
    final MashinCoreBridgeApi? bridge = widget.coreBridge;
    if (bridge == null || index < 0 || index >= widget.items.length) {
      return;
    }

    final MediaItem? item = resolvePlayableMediaItem(widget.items[index]);
    if (item == null || !isValidPlayable(item)) {
      return;
    }

    unawaited(() async {
      try {
        await bridge.queuePlayMedia(<MediaItem>[item]);
      } catch (_) {
        // Keep row interactions responsive when playback rejects an item.
      }
    }());
  }

  bool get _isTrackSection => T == Track;

  List<MediaItem> _playableItems() {
    final List<MediaItem> playItems = <MediaItem>[];
    for (final T item in widget.items) {
      final MediaItem? mediaItem = resolvePlayableMediaItem(item);
      if (mediaItem != null && isValidPlayable(mediaItem)) {
        playItems.add(mediaItem);
      }
    }
    return playItems;
  }

  void _playAll() {
    final MashinCoreBridgeApi? bridge = widget.coreBridge;
    if (bridge == null) {
      return;
    }

    final List<MediaItem> playItems = _playableItems();
    if (playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await bridge.queuePlayMedia(
          _isTrackSection ? playItems : <MediaItem>[playItems.first],
        );
      } catch (_) {
        // Keep header interactions responsive when playback rejects an item.
      }
    }());
  }

  void _shufflePlayAll() {
    final MashinCoreBridgeApi? bridge = widget.coreBridge;
    if (bridge == null) {
      return;
    }

    final List<MediaItem> playItems = _playableItems();
    if (playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await bridge.queuePlayMedia(playItems, shuffle: true);
      } catch (_) {
        // Keep header interactions responsive when playback rejects an item.
      }
    }());
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool hasTitle = widget.title.trim().isNotEmpty;
    final bool hasPlayableContent =
        !widget.isLoading && _playableItems().isNotEmpty;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        if (hasTitle)
          MobileSectionHeader(
            title: widget.title,
            itemCount: !widget.isLoading && widget.items.isNotEmpty
                ? widget.items.length
                : null,
            showPlaybackActions: widget.coreBridge != null,
            onPlayTap: hasPlayableContent ? _playAll : null,
            onShuffleTap: hasPlayableContent ? _shufflePlayAll : null,
            shuffleTooltip: 'Zufällig abspielen',
          ),
        if (hasTitle) const SizedBox(height: 10),
        if (!widget.isLoading && widget.items.isEmpty)
          Text(widget.emptyText, style: TextStyle(color: colors.textSecondary))
        else
          SizedBox(
            height: widget.itemHeight,
            child: widget.isLoading ? _buildSkeletonList() : _buildItemList(),
          ),
      ],
    );
  }

  Widget _buildSkeletonList() {
    return PageView.builder(
      controller: _mobileChunkPageController,
      padEnds: false,
      physics: const PageScrollPhysics(parent: BouncingScrollPhysics()),
      itemCount: widget.skeletonCount,
      itemBuilder: (BuildContext context, int index) {
        return Padding(
          padding: EdgeInsets.only(
            right: index == widget.skeletonCount - 1 ? 0 : _mobileChunkGap,
          ),
          child: _MobileSlideSkeleton(
            itemWidth: double.infinity,
            coverSize: widget.coverSize,
          ),
        );
      },
    );
  }

  Widget _buildItemList() {
    return PageView.builder(
      controller: _mobileChunkPageController,
      padEnds: false,
      physics: const PageScrollPhysics(parent: BouncingScrollPhysics()),
      itemCount: widget.items.length,
      itemBuilder: (BuildContext context, int index) {
        final bool isLastPage = index == widget.items.length - 1;

        return Padding(
          padding: EdgeInsets.only(right: isLastPage ? 0 : _mobileChunkGap),
          child: MobileSelectableSectionItem(
            selectionModeActive: _selectionController.isSelectionModeActive,
            isSelected: _selectionController.isSelected(index),
            onLongPress: () =>
                _selectionController.select(index, widget.items.length),
            onToggleSelection: () =>
                _selectionController.toggle(index, widget.items.length),
            onTap:
                widget.onItemTap == null ||
                    (widget.items[index] is Track &&
                        !(widget.items[index] as Track).canPlay)
                ? null
                : () => widget.onItemTap!(widget.items[index]),
            child: Opacity(
              opacity:
                  resolvePlayableMediaItem(widget.items[index])?.canPlay ==
                      false
                  ? 0.35
                  : 1,
              child: _MobileSlideCard<T>(
                item: widget.items[index],
                itemWidth: double.infinity,
                coverSize: widget.coverSize,
                coreEvents: widget.coreEvents,
                onPlayTap:
                    resolvePlayableMediaItem(widget.items[index])?.canPlay ==
                        true
                    ? () => _playItem(index)
                    : null,
                onTap: null,
              ),
            ),
          ),
        );
      },
    );
  }
}

class _MobileSlideCard<T> extends StatelessWidget {
  final T item;
  final double itemWidth;
  final double coverSize;
  final MashinCoreBridgeEvents? coreEvents;
  final VoidCallback? onPlayTap;
  final VoidCallback? onTap;

  const _MobileSlideCard({
    required this.item,
    required this.itemWidth,
    required this.coverSize,
    this.coreEvents,
    required this.onPlayTap,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final MediaSlideItemData data = resolveMediaSlideItemData(item);
    final String? coverUrl = data.coverUrl?.trim();
    final bool hasCover = coverUrl != null && coverUrl.isNotEmpty;
    final double coverScale = MobileSectionSelectionScope.coverScaleOf(context);

    return GestureDetector(
      onTap: onTap,
      behavior: HitTestBehavior.opaque,
      child: SizedBox(
        width: itemWidth,
        height: coverSize,
        child: ClipRRect(
          borderRadius: BorderRadius.circular(14),
          child: Stack(
            fit: StackFit.expand,
            children: <Widget>[
              if (hasCover)
                Transform.scale(
                  alignment: Alignment.center,
                  scale: coverScale,
                  child: MediaArtworkBackground(
                    source: coverUrl,
                    borderRadius: BorderRadius.circular(14),
                    child: const SizedBox.shrink(),
                  ),
                )
              else
                Transform.scale(
                  alignment: Alignment.center,
                  scale: coverScale,
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(14),
                    child: ColoredBox(color: colors.bgSecondary),
                  ),
                ),
              Row(
                children: <Widget>[
                  SizedBox.square(
                    dimension: coverSize,
                    child: Stack(
                      fit: StackFit.expand,
                      children: <Widget>[
                        Transform.scale(
                          alignment: Alignment.center,
                          scale: coverScale,
                          child: ColoredBox(
                            color: colors.bgTertiary,
                            child: hasCover
                                ? MediaImage(
                                    source: coverUrl,
                                    fit: BoxFit.cover,
                                    fallback: Icon(
                                      data.fallbackIcon,
                                      color: colors.textSecondary,
                                      size: 34,
                                    ),
                                  )
                                : Icon(
                                    data.fallbackIcon,
                                    color: colors.textSecondary,
                                    size: 34,
                                  ),
                          ),
                        ),
                        Positioned(
                          left: 6,
                          bottom: 6,
                          child: Material(
                            color: Colors.transparent,
                            shape: const CircleBorder(),
                            child: InkWell(
                              customBorder: const CircleBorder(),
                              onTap: onPlayTap,
                              splashColor: colors.selection,
                              child: Container(
                                width: 40,
                                height: 40,
                                decoration: BoxDecoration(
                                  color: colors.overlayPlayButtonBackground
                                      .withValues(alpha: 0.88),
                                  borderRadius: BorderRadius.circular(24),
                                ),
                                child: Icon(
                                  Icons.play_arrow_sharp,
                                  size: 24,
                                  color: colors.overlayPlayButtonForeground,
                                ),
                              ),
                            ),
                          ),
                        ),
                        Positioned(
                          left: 6,
                          top: 6,
                          child: data.mediaItem == null
                              ? const SizedBox.shrink()
                              : MediaItemProviderIcon(
                                  item: data.mediaItem!,
                                  size: 18,
                                  primaryIconOffset: Offset.zero,
                                ),
                        ),
                      ],
                    ),
                  ),
                  Expanded(
                    child: Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 12),
                      child: Column(
                        mainAxisAlignment: MainAxisAlignment.center,
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: <Widget>[
                          PlaybackAwareBuilder(
                            coreEvents: coreEvents,
                            item: data.mediaItem,
                            builder: (BuildContext context, bool isCurrent) {
                              return Text(
                                data.title,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(
                                  fontFamily: 'PoppinsRegular',
                                  color: Colors.white,
                                  fontSize: 17,
                                  shadows: _artworkTextShadows,
                                ),
                              );
                            },
                          ),
                          const SizedBox(height: 3),
                          Text(
                            data.subtitle,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                              fontFamily: 'PoppinsRegular',
                              color: Colors.white,
                              fontSize: 12,
                              shadows: _artworkTextShadows,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

const List<Shadow> _artworkTextShadows = <Shadow>[
  Shadow(color: Color(0x99000000), blurRadius: 8),
];

class _MobileSlideSkeleton extends StatelessWidget {
  final double itemWidth;
  final double coverSize;

  const _MobileSlideSkeleton({
    required this.itemWidth,
    required this.coverSize,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Container(
      width: itemWidth,
      height: coverSize,
      decoration: BoxDecoration(
        color: colors.bgSecondary,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Row(
        children: <Widget>[
          Container(
            width: coverSize,
            height: coverSize,
            decoration: BoxDecoration(
              color: colors.bgTertiary,
              borderRadius: BorderRadius.circular(12),
            ),
            child: Icon(
              Icons.music_note_outlined,
              color: colors.textSecondary,
              size: 34,
            ),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                _skeletonLine(context, 150, 20),
                const SizedBox(height: 3),
                _skeletonLine(context, 110, 14),
              ],
            ),
          ),
          const SizedBox(width: 12),
        ],
      ),
    );
  }

  Widget _skeletonLine(BuildContext context, double width, double height) {
    final colors = context.mashinColors;
    return Container(
      width: width,
      height: height,
      decoration: BoxDecoration(
        color: colors.bgTertiary,
        borderRadius: BorderRadius.circular(8),
      ),
    );
  }
}

// Normalizes supported media models into slide card display data.
class MediaSlideItemData {
  final MediaItem? mediaItem;
  final String title;
  final String subtitle;
  final String? coverUrl;
  final IconData fallbackIcon;

  const MediaSlideItemData({
    required this.mediaItem,
    required this.title,
    required this.subtitle,
    required this.coverUrl,
    required this.fallbackIcon,
  });
}

MediaSlideItemData resolveMediaSlideItemData(Object? item) {
  if (item is QueueItem) {
    final MediaItem mediaItem = item.mediaItem;
    final Track? track = mediaItem is Track ? mediaItem : null;
    return MediaSlideItemData(
      mediaItem: mediaItem,
      title: _prefer(mediaItem.name, 'Unbekannter Titel'),
      subtitle: track == null ? '-' : _trackSubtitle(track),
      coverUrl: item.imageUriForSize(512),
      fallbackIcon: Icons.music_note_outlined,
    );
  }

  if (item is Track) {
    return MediaSlideItemData(
      mediaItem: item,
      title: _prefer(item.name, 'Unknown Track'),
      subtitle: _trackSubtitle(item),
      coverUrl: item.imageUriForSize(512),
      fallbackIcon: Icons.music_note_outlined,
    );
  }

  if (item is Album) {
    return MediaSlideItemData(
      mediaItem: item,
      title: _prefer(item.name, 'Unknown Album'),
      subtitle: _albumSubtitle(item),
      coverUrl: item.imageUriForSize(512),
      fallbackIcon: Icons.album_outlined,
    );
  }

  if (item is Playlist) {
    return MediaSlideItemData(
      mediaItem: item,
      title: _prefer(item.name, 'Unknown Playlist'),
      subtitle: _playlistSubtitle(item),
      coverUrl: item.imageUriForSize(512),
      fallbackIcon: Icons.queue_music_outlined,
    );
  }

  if (item is Artist) {
    return MediaSlideItemData(
      mediaItem: item,
      title: _prefer(item.name, 'Unknown Artist'),
      subtitle: 'Artist',
      coverUrl: item.imageUriForSize(512),
      fallbackIcon: Icons.person_outlined,
    );
  }

  return MediaSlideItemData(
    mediaItem: null,
    title: item?.toString().trim().isNotEmpty == true
        ? item.toString().trim()
        : 'Unknown Item',
    subtitle: '-',
    coverUrl: null,
    fallbackIcon: Icons.music_note_outlined,
  );
}

MediaItem? resolvePlayableMediaItem(Object? item) {
  if (item is QueueItem) {
    return item.mediaItem;
  }
  if (item is MediaItem) {
    return item;
  }
  return null;
}

bool isValidPlayable(MediaItem item) {
  return item.canPlay &&
      item.itemId.trim().isNotEmpty &&
      item.provider.trim().isNotEmpty;
}

String _albumSubtitle(Album album) {
  final String artist = _firstArtistName(album.artists);
  if (artist != '-') {
    return artist;
  }
  return _mediaTypeLabel(album.albumType.name);
}

String _playlistSubtitle(Playlist playlist) {
  final String owner = _stripOrEmpty(playlist.owner);
  if (owner.isNotEmpty) {
    return owner;
  }
  return 'Playlist';
}

String _trackSubtitle(Track track) {
  final String artist = _firstArtistName(track.artists);
  if (artist != '-') {
    return artist;
  }
  return _prefer(track.album?.name ?? '', 'Track');
}

String _mediaTypeLabel(String? rawType) {
  final String value = rawType?.trim().toLowerCase() ?? '';
  if (value.isEmpty) {
    return '-';
  }

  switch (value) {
    case 'album':
      return 'Album';
    case 'single':
      return 'Single';
    case 'compilation':
      return 'Compilation';
    case 'ep':
      return 'EP';
    default:
      return rawType ?? '-';
  }
}

String _firstArtistName(List<Artist> artists) {
  if (artists.isEmpty) {
    return '-';
  }
  return _prefer(artists.first.name, '-');
}

String _stripOrEmpty(String? value) {
  return value?.trim() ?? '';
}

String _prefer(String primary, String fallback) {
  final String value = primary.trim();
  if (value.isNotEmpty) {
    return value;
  }
  return fallback.trim();
}
