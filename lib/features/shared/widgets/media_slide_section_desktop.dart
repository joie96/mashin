import 'dart:async';
import 'dart:ui';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';
import '../../core/core_bridge.dart';
import '../../core/core_models.dart';
import '../../core/core_events.dart';
import 'media_image.dart';
import 'media_link_text.dart';
import 'media_table_section_desktop.dart';

class MediaSlideSectionDesktop<T> extends StatefulWidget {
  final String title;
  final List<T> items;
  final bool isLoading;
  final String emptyText;
  final double height;
  final double coverSize;
  final MashinCoreBridgeApi? coreBridge;
  final MashinCoreBridgeEvents? coreEvents;
  final DesktopRowAction<T>? onRowFavoriteTap;
  final DesktopRowFavoriteResolver<T>? isRowFavorite;
  final DesktopRowAction<T>? onRowMoreTap;
  final DesktopRowMoreAnchorAction<T>? onRowMoreAnchorTap;
  final DesktopRowContextMenuAction<T>? onRowContextMenu;
  final ValueChanged<Artist>? onArtistTap;
  final ValueChanged<Album>? onAlbumTap;

  const MediaSlideSectionDesktop({
    super.key,
    required this.title,
    required this.items,
    required this.isLoading,
    required this.emptyText,
    this.height = 240,
    this.coverSize = 200,
    this.coreBridge,
    this.coreEvents,
    this.onRowFavoriteTap,
    this.isRowFavorite,
    this.onRowMoreTap,
    this.onRowMoreAnchorTap,
    this.onRowContextMenu,
    this.onArtistTap,
    this.onAlbumTap,
  });

  @override
  State<MediaSlideSectionDesktop<T>> createState() =>
      _MediaSlideSectionDesktopState<T>();
}

// Shows one featured media item with explicit page controls.

class _MediaSlideSectionDesktopState<T>
    extends State<MediaSlideSectionDesktop<T>> {
  static const Duration _slideDuration = Duration(milliseconds: 250);
  static const Curve _slideCurve = Curves.easeInOutCubic;

  int _currentIndex = 0;
  int _direction = 1;

  @override
  void didUpdateWidget(covariant MediaSlideSectionDesktop<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.items.isEmpty) {
      _currentIndex = 0;
      return;
    }
    _currentIndex = _currentIndex.clamp(0, widget.items.length - 1);
  }

  bool get _canGoPrevious => _currentIndex > 0;
  bool get _canGoNext => _currentIndex < widget.items.length - 1;

  void _goToIndex(int index) {
    if (widget.items.isEmpty) {
      return;
    }

    final int nextIndex = index.clamp(0, widget.items.length - 1);
    if (nextIndex == _currentIndex) {
      return;
    }

    setState(() {
      _direction = nextIndex > _currentIndex ? 1 : -1;
      _currentIndex = nextIndex;
    });
  }

  void _playCurrent() {
    final MashinCoreBridgeApi? bridge = widget.coreBridge;
    if (bridge == null || widget.items.isEmpty) {
      return;
    }

    final MediaItem? item = resolvePlayableMediaItem(
      widget.items[_currentIndex],
    );
    if (item == null || !isValidPlayable(item)) {
      return;
    }

    unawaited(() async {
      try {
        await bridge.playMedia(<MediaItem>[item]);
      } catch (_) {
        // Keep card interactions responsive when playback rejects an item.
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
        await bridge.playMedia(
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
        await bridge.playShuffledMedia(playItems);
      } catch (_) {
        // Keep header interactions responsive when playback rejects an item.
      }
    }());
  }

  bool _isItemFavorite(T item, int index) {
    final DesktopRowFavoriteResolver<T>? resolver = widget.isRowFavorite;
    if (resolver != null) {
      return resolver(item, index);
    }
    if (item is QueueItem) {
      return item.mediaItem.favorite;
    }
    return (item as MediaItem?)?.favorite ?? false;
  }

  void _openMore(BuildContext anchorContext) {
    if (widget.items.isEmpty) {
      return;
    }

    final T item = widget.items[_currentIndex];
    final DesktopRowMoreAnchorAction<T>? anchorHandler =
        widget.onRowMoreAnchorTap;
    if (anchorHandler != null) {
      anchorHandler(item, _currentIndex, anchorContext);
      return;
    }

    widget.onRowMoreTap?.call(item, _currentIndex);
  }

  void _openContextMenu(PointerDownEvent event) {
    if (event.kind != PointerDeviceKind.mouse ||
        (event.buttons & kSecondaryMouseButton) == 0 ||
        widget.items.isEmpty) {
      return;
    }

    widget.onRowContextMenu?.call(
      widget.items[_currentIndex],
      _currentIndex,
      event.position,
    );
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
              if (widget.coreBridge != null) ...<Widget>[
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
              const SizedBox(width: 4),
              const SectionHeaderVerticalDivider(),
              IconButton(
                tooltip: 'Vorherige Seite',
                onPressed: _canGoPrevious
                    ? () => _goToIndex(_currentIndex - 1)
                    : null,
                icon: const Icon(Icons.chevron_left_outlined),
                visualDensity: VisualDensity.compact,
              ),
              IconButton(
                tooltip: 'Nächste Seite',
                onPressed: _canGoNext
                    ? () => _goToIndex(_currentIndex + 1)
                    : null,
                icon: const Icon(Icons.chevron_right_outlined),
                visualDensity: VisualDensity.compact,
              ),
            ],
          ),
        if (hasTitle) const SizedBox(height: 10),
        if (widget.isLoading)
          _DesktopSlideSkeleton(
            height: widget.height,
            coverSize: widget.coverSize,
          )
        else if (widget.items.isEmpty)
          Text(
            widget.emptyText,
            style: context.mashinTableTextStyles.emptyState,
          )
        else
          SizedBox(
            height: widget.height,
            child: ClipRRect(
              borderRadius: BorderRadius.circular(10),
              child: DecoratedBox(
                decoration: BoxDecoration(color: colors.bgSecondary),
                child: AnimatedSwitcher(
                  duration: _slideDuration,
                  switchInCurve: _slideCurve,
                  switchOutCurve: _slideCurve,
                  transitionBuilder:
                      (Widget child, Animation<double> animation) {
                        final bool incoming =
                            child.key == ValueKey<int>(_currentIndex);
                        final double beginOffset = incoming
                            ? _direction.toDouble()
                            : -_direction.toDouble();
                        final Animation<Offset> offsetAnimation = Tween<Offset>(
                          begin: Offset(beginOffset * 0.08, 0),
                          end: Offset.zero,
                        ).animate(animation);

                        return FadeTransition(
                          opacity: animation,
                          child: SlideTransition(
                            position: offsetAnimation,
                            child: child,
                          ),
                        );
                      },
                  child: Listener(
                    key: ValueKey<int>(_currentIndex),
                    onPointerDown: _openContextMenu,
                    child: _DesktopSlideCard<T>(
                      item: widget.items[_currentIndex],
                      index: _currentIndex,
                      coverSize: widget.coverSize,
                      coreEvents: widget.coreEvents,
                      isFavorite: _isItemFavorite(
                        widget.items[_currentIndex],
                        _currentIndex,
                      ),
                      onPlayTap: _playCurrent,
                      onFavoriteTap: widget.onRowFavoriteTap == null
                          ? null
                          : () => widget.onRowFavoriteTap!(
                              widget.items[_currentIndex],
                              _currentIndex,
                            ),
                      onMoreTap: _openMore,
                      onArtistTap: widget.onArtistTap,
                      onAlbumTap: widget.onAlbumTap,
                    ),
                  ),
                ),
              ),
            ),
          ),
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: SizedBox(height: 24),
        ),
      ],
    );
  }
}

class _DesktopSlideCard<T> extends StatefulWidget {
  final T item;
  final int index;
  final double coverSize;
  final MashinCoreBridgeEvents? coreEvents;
  final bool isFavorite;
  final VoidCallback? onPlayTap;
  final VoidCallback? onFavoriteTap;
  final ValueChanged<BuildContext>? onMoreTap;
  final ValueChanged<Artist>? onArtistTap;
  final ValueChanged<Album>? onAlbumTap;

  const _DesktopSlideCard({
    required this.item,
    required this.index,
    required this.coverSize,
    this.coreEvents,
    required this.isFavorite,
    required this.onPlayTap,
    required this.onFavoriteTap,
    required this.onMoreTap,
    required this.onArtistTap,
    required this.onAlbumTap,
  });

  @override
  State<_DesktopSlideCard<T>> createState() => _DesktopSlideCardState<T>();
}

class _DesktopSlideCardState<T> extends State<_DesktopSlideCard<T>> {
  bool _isCoverHovered = false;
  bool _isPlayHovered = false;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final MediaSlideItemData data = resolveMediaSlideItemData(widget.item);
    final String? coverUrl = data.coverUrl?.trim();
    final bool hasCover = coverUrl != null && coverUrl.isNotEmpty;

    return MouseRegion(
      onExit: (_) => setState(() {
        _isCoverHovered = false;
        _isPlayHovered = false;
      }),
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
            DecoratedBox(decoration: BoxDecoration(color: colors.bgSecondary)),
          Padding(
            padding: const EdgeInsets.all(20),
            child: Row(
              children: <Widget>[
                MouseRegion(
                  onEnter: (_) => setState(() => _isCoverHovered = true),
                  onExit: (_) => setState(() {
                    _isCoverHovered = false;
                    _isPlayHovered = false;
                  }),
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(8),
                    child: SizedBox.square(
                      dimension: widget.coverSize,
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
                                      size: 46,
                                    ),
                                  )
                                : Icon(
                                    data.fallbackIcon,
                                    color: colors.accent,
                                    size: 46,
                                  ),
                          ),
                          Positioned(
                            left: 6,
                            top: 6,
                            child: MediaProviderIcon(
                              providerLabel: data.providerLabel,
                              iconSvg: data.providerIconSvg,
                              size: 18,
                            ),
                          ),
                          Positioned.fill(
                            child: AnimatedOpacity(
                              duration: const Duration(milliseconds: 140),
                              opacity: _isCoverHovered ? 1 : 0,
                              child: Stack(
                                fit: StackFit.expand,
                                children: <Widget>[
                                  ColoredBox(
                                    color: Colors.black.withValues(
                                      alpha: colors.coverHoverOverlayOpacity,
                                    ),
                                  ),
                                  Center(
                                    child: MouseRegion(
                                      onEnter: (_) =>
                                          setState(() => _isPlayHovered = true),
                                      onExit: (_) => setState(
                                        () => _isPlayHovered = false,
                                      ),
                                      child: AnimatedScale(
                                        duration: const Duration(
                                          milliseconds: 140,
                                        ),
                                        curve: Curves.easeOutCubic,
                                        scale: _isPlayHovered ? 1.08 : 1,
                                        child: GestureDetector(
                                          onTap: widget.onPlayTap,
                                          child: Container(
                                            width: 56,
                                            height: 56,
                                            decoration: BoxDecoration(
                                              color: colors
                                                  .overlayPlayButtonBackground,
                                              borderRadius:
                                                  BorderRadius.circular(28),
                                            ),
                                            child: Icon(
                                              Icons.play_arrow_sharp,
                                              size: 30,
                                              color: colors
                                                  .overlayPlayButtonForeground,
                                            ),
                                          ),
                                        ),
                                      ),
                                    ),
                                  ),
                                  Positioned(
                                    right: 8,
                                    bottom: 8,
                                    child: Row(
                                      children: <Widget>[
                                        _SlideOverlayIconButton(
                                          icon: widget.isFavorite
                                              ? Icons.favorite_outlined
                                              : Icons.favorite_border_outlined,
                                          onTap: widget.onFavoriteTap,
                                        ),
                                        const SizedBox(width: 4),
                                        _SlideOverlayIconButton(
                                          icon: Icons.more_horiz_outlined,
                                          onAnchorTap: widget.onMoreTap,
                                        ),
                                      ],
                                    ),
                                  ),
                                ],
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: 20),
                Expanded(
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.end,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: <Widget>[
                      PlaybackAwareBuilder(
                        coreEvents: widget.coreEvents,
                        item: data.mediaItem,
                        builder: (BuildContext context, bool isCurrent) {
                          return Text(
                            data.title,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: Theme.of(context).textTheme.headlineSmall
                                ?.copyWith(
                                  color: Colors.white,
                                  fontWeight: FontWeight.w800,
                                  shadows: _artworkTextShadows,
                                ),
                          );
                        },
                      ),
                      const SizedBox(height: 6),
                      if (data.artists.isNotEmpty)
                        MediaLinkText(
                          items: data.artists,
                          fallbackText: data.subtitle,
                          onItemTap: widget.onArtistTap == null
                              ? null
                              : (MediaItem item) =>
                                    widget.onArtistTap!(item as Artist),
                          style: TextStyle(
                            color: Colors.white,
                            fontSize: 18,
                            fontWeight: FontWeight.w700,
                            shadows: _artworkTextShadows,
                          ),
                        )
                      else
                        Text(
                          data.subtitle,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            color: Colors.white,
                            fontSize: 18,
                            fontWeight: FontWeight.w700,
                            shadows: _artworkTextShadows,
                          ),
                        ),
                      const SizedBox(height: 4),
                      if (data.album != null)
                        MediaLinkText(
                          items: <MediaItem>[data.album!],
                          fallbackText: data.tertiary,
                          onItemTap: widget.onAlbumTap == null
                              ? null
                              : (MediaItem item) =>
                                    widget.onAlbumTap!(item as Album),
                          style: TextStyle(
                            color: Colors.white,
                            fontSize: 14,
                            shadows: _artworkTextShadows,
                          ),
                        )
                      else
                        Text(
                          data.tertiary,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            color: Colors.white,
                            fontSize: 14,
                            shadows: _artworkTextShadows,
                          ),
                        ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

const List<Shadow> _artworkTextShadows = <Shadow>[
  Shadow(color: Color(0x99000000), blurRadius: 8),
];

// Normalizes supported media models into slide card display data.
class MediaSlideItemData {
  final MediaItem? mediaItem;
  final String title;
  final String subtitle;
  final String tertiary;
  final String? coverUrl;
  final IconData fallbackIcon;
  final String providerLabel;
  final String? providerIconSvg;
  final List<Artist> artists;
  final Album? album;

  const MediaSlideItemData({
    required this.mediaItem,
    required this.title,
    required this.subtitle,
    required this.tertiary,
    required this.coverUrl,
    required this.fallbackIcon,
    required this.providerLabel,
    required this.providerIconSvg,
    required this.artists,
    required this.album,
  });
}

MediaSlideItemData resolveMediaSlideItemData(Object? item) {
  if (item is QueueItem) {
    final MediaItem mediaItem = item.mediaItem;
    final Track? track = mediaItem is Track ? mediaItem : null;
    final String provider = _resolveQueueProvider(item);
    return MediaSlideItemData(
      mediaItem: mediaItem,
      title: _prefer(mediaItem.name, 'Unbekannter Titel'),
      subtitle: track == null ? '-' : _trackSubtitle(track),
      tertiary: _formatDuration(item.duration ?? track?.duration ?? 0),
      coverUrl: mediaItem.imageUriForSize(512),
      fallbackIcon: Icons.music_note_outlined,
      providerLabel: _providerLabelFromId(provider),
      providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(provider),
      artists: track?.artists ?? const <Artist>[],
      album: track?.album,
    );
  }

  if (item is Track) {
    return MediaSlideItemData(
      mediaItem: item,
      title: _prefer(item.name, 'Unknown Track'),
      subtitle: _trackSubtitle(item),
      tertiary: _trackTertiary(item),
      coverUrl: item.imageUriForSize(512),
      fallbackIcon: Icons.music_note_outlined,
      providerLabel: _providerLabelFromId(item.provider),
      providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(
        item.provider,
      ),
      artists: item.artists,
      album: item.album,
    );
  }

  if (item is Album) {
    return MediaSlideItemData(
      mediaItem: item,
      title: _prefer(item.name, 'Unknown Album'),
      subtitle: _albumSubtitle(item),
      tertiary: _albumTertiary(item),
      coverUrl: item.imageUriForSize(512),
      fallbackIcon: Icons.album_outlined,
      providerLabel: _providerLabelFromId(item.provider),
      providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(
        item.provider,
      ),
      artists: item.artists,
      album: item,
    );
  }

  if (item is Playlist) {
    return MediaSlideItemData(
      mediaItem: item,
      title: _prefer(item.name, 'Unknown Playlist'),
      subtitle: _playlistSubtitle(item),
      tertiary: item.isEditable ? 'Editierbar' : 'Readonly',
      coverUrl: item.imageUriForSize(512),
      fallbackIcon: Icons.queue_music_outlined,
      providerLabel: _providerLabelFromId(item.provider),
      providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(
        item.provider,
      ),
      artists: const <Artist>[],
      album: null,
    );
  }

  if (item is Artist) {
    return MediaSlideItemData(
      mediaItem: item,
      title: _prefer(item.name, 'Unknown Artist'),
      subtitle: 'Artist',
      tertiary: _artistTertiary(item),
      coverUrl: item.imageUriForSize(512),
      fallbackIcon: Icons.person_outlined,
      providerLabel: _providerLabelFromId(item.provider),
      providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(
        item.provider,
      ),
      artists: <Artist>[item],
      album: null,
    );
  }

  return MediaSlideItemData(
    mediaItem: null,
    title: item?.toString().trim().isNotEmpty == true
        ? item.toString().trim()
        : 'Unknown Item',
    subtitle: '-',
    tertiary: '-',
    coverUrl: null,
    fallbackIcon: Icons.music_note_outlined,
    providerLabel: '-',
    providerIconSvg: null,
    artists: const <Artist>[],
    album: null,
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
  return _mediaTypeLabel(album.albumType.name);
}

String _albumTertiary(Album album) {
  if (album.year != null && album.year! > 0) {
    return '${album.year}';
  }
  return 'Album';
}

String _artistTertiary(Artist artist) {
  final List<String> genres = artist.metadata?.genres ?? const <String>[];
  if (genres.isEmpty) {
    return '-';
  }
  return genres.take(2).join(', ');
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

String _trackTertiary(Track track) {
  final String albumName = _stripOrEmpty(track.album?.name);
  if (albumName.isNotEmpty) {
    return albumName;
  }
  return _formatDuration(track.duration);
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

String _formatDuration(int seconds) {
  if (seconds <= 0) {
    return '0m';
  }

  final Duration duration = Duration(seconds: seconds);
  final int hours = duration.inHours;
  final int minutes = duration.inMinutes;
  final int remainingMinutes = minutes % 60;
  if (hours > 0) {
    return '${hours}h ${remainingMinutes}m';
  }
  return '${minutes}m';
}

String _providerLabelFromId(String providerOrDomain) {
  final String trimmed = providerOrDomain.trim();
  if (trimmed.isEmpty) {
    return '-';
  }

  final String domain = trimmed.split('--').first.trim();
  if (domain.isEmpty) {
    return trimmed;
  }

  return domain[0].toUpperCase() + domain.substring(1);
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

String _resolveQueueProvider(QueueItem item) {
  final String directProvider = _stripOrEmpty(item.mediaItem.provider);
  if (directProvider.isNotEmpty) {
    return directProvider;
  }

  final List<ProviderMapping> mappings = item.mediaItem.providerMappings;
  for (final ProviderMapping mapping in mappings) {
    final String domain = _stripOrEmpty(mapping.providerDomain);
    if (domain.isNotEmpty) {
      return domain;
    }

    final String instance = _stripOrEmpty(mapping.providerInstance);
    if (instance.isNotEmpty) {
      return instance;
    }
  }

  return '';
}

class _SlideOverlayIconButton extends StatefulWidget {
  final IconData icon;
  final VoidCallback? onTap;
  final ValueChanged<BuildContext>? onAnchorTap;

  const _SlideOverlayIconButton({
    required this.icon,
    this.onTap,
    this.onAnchorTap,
  });

  @override
  State<_SlideOverlayIconButton> createState() =>
      _SlideOverlayIconButtonState();
}

class _SlideOverlayIconButtonState extends State<_SlideOverlayIconButton> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool isEnabled = widget.onTap != null || widget.onAnchorTap != null;

    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: isEnabled
            ? () {
                final ValueChanged<BuildContext>? anchorTap =
                    widget.onAnchorTap;
                if (anchorTap != null) {
                  anchorTap(context);
                } else {
                  widget.onTap?.call();
                }
              }
            : null,
        child: SizedBox.square(
          dimension: 32,
          child: Center(
            child: AnimatedScale(
              scale: _isHovered && isEnabled ? 1.08 : 1,
              duration: const Duration(milliseconds: 120),
              curve: Curves.easeOutCubic,
              child: Icon(widget.icon, size: 18, color: colors.overlayIcon),
            ),
          ),
        ),
      ),
    );
  }
}

class _DesktopSlideSkeleton extends StatelessWidget {
  final double height;
  final double coverSize;

  const _DesktopSlideSkeleton({required this.height, required this.coverSize});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    Widget line(double width, double height) {
      return Container(
        width: width,
        height: height,
        decoration: BoxDecoration(
          color: colors.bgTertiary,
          borderRadius: BorderRadius.circular(8),
        ),
      );
    }

    return Container(
      height: height,
      padding: const EdgeInsets.all(20),
      decoration: BoxDecoration(
        color: colors.bgSecondary,
        borderRadius: BorderRadius.circular(10),
      ),
      child: Row(
        children: <Widget>[
          Container(
            width: coverSize,
            height: coverSize,
            decoration: BoxDecoration(
              color: colors.bgTertiary,
              borderRadius: BorderRadius.circular(8),
            ),
          ),
          const SizedBox(width: 20),
          Expanded(
            child: Column(
              mainAxisAlignment: MainAxisAlignment.end,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                line(460, 28),
                const SizedBox(height: 6),
                line(280, 22),
                const SizedBox(height: 4),
                line(90, 17),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
