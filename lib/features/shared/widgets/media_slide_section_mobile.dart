import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../../playback/playback_coordinator.dart';
import 'media_image.dart';
import 'media_table_section_desktop.dart';

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
  final PlaybackCoordinator? playbackCoordinator;
  final ValueChanged<T>? onItemTap;

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
    this.playbackCoordinator,
    this.onItemTap,
  });

  @override
  State<MediaSlideSectionMobile<T>> createState() =>
      _MediaSlideSectionMobileState<T>();
}

// Shows compact slide cards in a horizontally growing list.

class _MediaSlideSectionMobileState<T>
    extends State<MediaSlideSectionMobile<T>> {
  static const double _mobileChunkViewportFraction = 0.92;
  static const double _mobileChunkGap = 8;

  final PageController _mobileChunkPageController = PageController(
    viewportFraction: _mobileChunkViewportFraction,
  );

  @override
  void dispose() {
    _mobileChunkPageController.dispose();
    super.dispose();
  }

  void _playItem(int index) {
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
    if (coordinator == null || index < 0 || index >= widget.items.length) {
      return;
    }

    final MediaItem? item = resolvePlayableMediaItem(widget.items[index]);
    if (item == null || !isValidPlayable(item)) {
      return;
    }

    unawaited(() async {
      try {
        await coordinator.playMedia(<MediaItem>[item]);
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
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
    if (coordinator == null) {
      return;
    }

    final List<MediaItem> playItems = _playableItems();
    if (playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await coordinator.playMedia(
          _isTrackSection ? playItems : <MediaItem>[playItems.first],
        );
      } catch (_) {
        // Keep header interactions responsive when playback rejects an item.
      }
    }());
  }

  void _shufflePlayAll() {
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
    if (coordinator == null) {
      return;
    }

    final List<MediaItem> playItems = _playableItems();
    if (playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        if (_isTrackSection) {
          await coordinator.shufflePlayMedia(playItems);
          return;
        }

        final MediaItem item =
            playItems[math.Random().nextInt(playItems.length)];
        await coordinator.playMedia(<MediaItem>[item]);
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
          Row(
            children: <Widget>[
              Text(
                widget.title,
                style: Theme.of(
                  context,
                ).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
              ),
              if (!widget.isLoading && widget.items.isNotEmpty) ...<Widget>[
                const SizedBox(width: 10),
                SectionCountBadge(count: widget.items.length),
              ],
              const SizedBox(width: 16),
              const Expanded(child: SectionHeaderDivider()),
              const SizedBox(width: 16),
              if (widget.playbackCoordinator != null) ...<Widget>[
                SectionHeaderIconButton(
                  icon: Icons.play_arrow,
                  tooltip: 'Abspielen',
                  onTap: hasPlayableContent ? _playAll : null,
                ),
                const SizedBox(width: 8),
                SectionHeaderIconButton(
                  icon: Icons.shuffle,
                  tooltip: 'Zufällig abspielen',
                  onTap: hasPlayableContent ? _shufflePlayAll : null,
                ),
              ],
            ],
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
          child: _MobileSlideCard<T>(
            item: widget.items[index],
            itemWidth: double.infinity,
            coverSize: widget.coverSize,
            playbackCoordinator: widget.playbackCoordinator,
            onPlayTap: () => _playItem(index),
            onTap: widget.onItemTap == null
                ? null
                : () => widget.onItemTap!(widget.items[index]),
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
  final PlaybackCoordinator? playbackCoordinator;
  final VoidCallback? onPlayTap;
  final VoidCallback? onTap;

  const _MobileSlideCard({
    required this.item,
    required this.itemWidth,
    required this.coverSize,
    required this.playbackCoordinator,
    required this.onPlayTap,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final MediaSlideItemData data = resolveMediaSlideItemData(item);
    final String? coverUrl = data.coverUrl?.trim();
    final bool hasCover = coverUrl != null && coverUrl.isNotEmpty;

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
                MediaArtworkBackground(
                  source: coverUrl,
                  borderRadius: BorderRadius.zero,
                  child: const SizedBox.shrink(),
                )
              else
                ColoredBox(color: colors.bgSecondary),
              Row(
                children: <Widget>[
                  SizedBox.square(
                    dimension: coverSize,
                    child: Stack(
                      fit: StackFit.expand,
                      children: <Widget>[
                        ColoredBox(
                          color: colors.bgTertiary,
                          child: hasCover
                              ? MediaImage(
                                  source: coverUrl,
                                  fit: BoxFit.cover,
                                  fallback: Icon(
                                    data.fallbackIcon,
                                    color: colors.accent,
                                    size: 34,
                                  ),
                                )
                              : Icon(
                                  data.fallbackIcon,
                                  color: colors.accent,
                                  size: 34,
                                ),
                        ),
                        Positioned(
                          left: 6,
                          bottom: 6,
                          child: GestureDetector(
                            onTap: onPlayTap,
                            behavior: HitTestBehavior.opaque,
                            child: Container(
                              width: 40,
                              height: 40,
                              decoration: BoxDecoration(
                                color: colors.overlayPlayButtonBackground,
                                borderRadius: BorderRadius.circular(20),
                              ),
                              child: Icon(
                                Icons.play_arrow_sharp,
                                size: 20,
                                color: colors.overlayPlayButtonForeground,
                              ),
                            ),
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
                            coordinator: playbackCoordinator,
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
    final Track? mediaItem = item.mediaItem;
    return MediaSlideItemData(
      mediaItem: mediaItem,
      title: _prefer(mediaItem?.name ?? item.name, 'Unbekannter Titel'),
      subtitle: mediaItem == null ? '-' : _trackSubtitle(mediaItem),
      coverUrl: mediaItem?.imageUriForSize(512),
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
  return item.itemId.trim().isNotEmpty && item.provider.trim().isNotEmpty;
}

String _albumSubtitle(Album album) {
  final String artist = _firstArtistName(album.artists);
  if (artist != '-') {
    return artist;
  }
  return _mediaTypeLabel(album.albumType);
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
