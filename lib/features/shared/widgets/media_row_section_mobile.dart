import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'dart:async';
import 'dart:math' as math;

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../../playback/playback_coordinator.dart';
import '../selection/mobile_section_selection.dart';
import 'media_table_section_desktop.dart';
import 'media_image.dart';
import 'mobile_section_header.dart';

class MediaRowSectionMobile<T> extends StatefulWidget {
  final String title;
  final bool isLoading;
  final List<T> items;
  final String emptyText;
  final double itemWidth;
  final double coverSize;
  final double itemSpacing;
  final int skeletonCount;
  final PlaybackCoordinator? playbackCoordinator;
  final ValueChanged<T>? onItemTap;
  final ValueChanged<List<T>>? onSelectionMenuTap;
  final bool showPlaylistDetails;

  const MediaRowSectionMobile({
    super.key,
    required this.title,
    required this.isLoading,
    required this.items,
    required this.emptyText,
    this.itemWidth = 146,
    this.coverSize = 146,
    this.itemSpacing = 8,
    this.skeletonCount = 6,
    this.playbackCoordinator,
    this.onItemTap,
    this.onSelectionMenuTap,
    this.showPlaylistDetails = true,
  });

  @override
  State<MediaRowSectionMobile<T>> createState() =>
      _MediaRowSectionMobileState<T>();
}

class _MediaRowSectionMobileState<T> extends State<MediaRowSectionMobile<T>> {
  static const double _rowTitleHeight = 20;
  static const double _rowSecondaryHeight = 18;
  static const double _rowTertiaryHeight = 15;

  late final MobileSectionSelectionState _selectionController;

  String get title => widget.title;
  bool get isLoading => widget.isLoading;
  List<T> get items => widget.items;
  String get emptyText => widget.emptyText;
  double get itemWidth => widget.itemWidth;
  double get coverSize => widget.coverSize;
  double get itemSpacing => widget.itemSpacing;
  int get skeletonCount => widget.skeletonCount;
  PlaybackCoordinator? get playbackCoordinator => widget.playbackCoordinator;
  ValueChanged<T>? get onItemTap => widget.onItemTap;
  bool get showPlaylistDetails => widget.showPlaylistDetails;

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
  void didUpdateWidget(covariant MediaRowSectionMobile<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    _selectionController.updateItemCount(items.length);
  }

  @override
  void dispose() {
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
        .where((int index) => index >= 0 && index < items.length)
        .map((int index) => items[index])
        .toList(growable: false);
    if (selectedItems.isNotEmpty) {
      onSelectionMenuTap(selectedItems);
    }
  }

  MediaItem? _resolvePlayableMediaItem(Object? rawItem) {
    if (rawItem is QueueItem) {
      return rawItem.mediaItem;
    }
    if (rawItem is MediaItem) {
      return rawItem;
    }
    return null;
  }

  bool _isValidPlayable(MediaItem item) {
    return item.itemId.trim().isNotEmpty && item.provider.trim().isNotEmpty;
  }

  bool get _isTrackSection => T == Track;

  List<MediaItem> _playableItemsFromIndex(int startIndex) {
    if (items.isEmpty || startIndex < 0 || startIndex >= items.length) {
      return const <MediaItem>[];
    }

    final List<MediaItem> playItems = <MediaItem>[];
    for (int index = startIndex; index < items.length; index++) {
      final MediaItem? resolved = _resolvePlayableMediaItem(items[index]);
      if (resolved == null || !_isValidPlayable(resolved)) {
        continue;
      }
      playItems.add(resolved);
    }

    return playItems;
  }

  bool _hasPlayableItemsFromIndex(int startIndex) {
    if (items.isEmpty || startIndex < 0 || startIndex >= items.length) {
      return false;
    }

    for (int index = startIndex; index < items.length; index++) {
      final MediaItem? resolved = _resolvePlayableMediaItem(items[index]);
      if (resolved != null && _isValidPlayable(resolved)) {
        return true;
      }
    }

    return false;
  }

  void _onPlayAllTap() {
    final PlaybackCoordinator? coordinator = playbackCoordinator;
    if (coordinator == null) {
      return;
    }

    final List<MediaItem> playItems = _playableItemsFromIndex(0);
    if (playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await coordinator.playMedia(
          _isTrackSection ? playItems : <MediaItem>[playItems.first],
        );
      } catch (_) {
        // Ignore failures to keep header interactions responsive.
      }
    }());
  }

  void _onShuffleAllTap() {
    final PlaybackCoordinator? coordinator = playbackCoordinator;
    if (coordinator == null) {
      return;
    }

    final List<MediaItem> playItems = _playableItemsFromIndex(0);
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
        // Ignore failures to keep header interactions responsive.
      }
    }());
  }

  void _onPlayTap(int index) {
    final PlaybackCoordinator? coordinator = playbackCoordinator;
    if (coordinator == null || index < 0 || index >= items.length) {
      return;
    }

    final MediaItem? item = _resolvePlayableMediaItem(items[index]);
    if (item == null || !_isValidPlayable(item)) {
      return;
    }

    unawaited(() async {
      try {
        await coordinator.playMedia(<MediaItem>[item]);
      } catch (_) {
        // Ignore failures to keep row interactions responsive.
      }
    }());
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool hasTitle = title.trim().isNotEmpty;
    final bool hasPlayableContent = !isLoading && _hasPlayableItemsFromIndex(0);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        if (hasTitle)
          MobileSectionHeader(
            title: title,
            itemCount: hasPlayableContent ? items.length : null,
            showPlaybackActions: playbackCoordinator != null,
            onPlayTap: hasPlayableContent ? _onPlayAllTap : null,
            onShuffleTap: hasPlayableContent ? _onShuffleAllTap : null,
            shuffleTooltip: 'Zufällig',
          ),
        if (hasTitle) const SizedBox(height: 10),
        if (!isLoading && items.isEmpty)
          Text(emptyText, style: TextStyle(color: colors.textSecondary))
        else
          SizedBox(height: _sectionHeight, child: _buildHorizontalRows()),
      ],
    );
  }

  double get _sectionHeight {
    final int metadataLineCount = _metadataLineCount;
    final double metadataHeight =
        _rowTitleHeight +
        (metadataLineCount > 1 ? 2 + _rowSecondaryHeight : 0) +
        (metadataLineCount > 2 ? 2 + _rowTertiaryHeight : 0);
    return coverSize + 8 + metadataHeight;
  }

  int get _metadataLineCount {
    if (T == Artist) {
      return 1;
    }
    if (T == Playlist) {
      return showPlaylistDetails ? 2 : 1;
    }
    if (T == Track) {
      return 2;
    }
    return 3;
  }

  Widget _buildHorizontalRows() {
    final int itemCount = isLoading ? skeletonCount : _visibleItemCount;

    return ListView.builder(
      primary: false,
      scrollDirection: Axis.horizontal,
      itemCount: itemCount,
      itemExtent: itemWidth + itemSpacing,
      scrollCacheExtent: ScrollCacheExtent.pixels(0),
      addAutomaticKeepAlives: false,
      addRepaintBoundaries: true,
      addSemanticIndexes: false,
      itemBuilder: (BuildContext context, int index) {
        final bool isLastItem = index == itemCount - 1;
        final Widget item = isLoading
            ? _buildMobileTypedRowSkeleton()
            : MobileSelectableSectionItem(
                selectionModeActive: _selectionController.isSelectionModeActive,
                isSelected: _selectionController.isSelected(index),
                onLongPress: () =>
                    _selectionController.select(index, items.length),
                onToggleSelection: () =>
                    _selectionController.toggle(index, items.length),
                onTap: _itemTapCallback(items[index]),
                child: _buildMobileTypedRow(
                  item: items[index],
                  index: index,
                  onItemTap: null,
                ),
              );

        return Padding(
          padding: EdgeInsets.only(right: isLastItem ? 0 : itemSpacing),
          child: item,
        );
      },
    );
  }

  Widget _buildMobileTypedRow({
    required T item,
    required int index,
    required VoidCallback? onItemTap,
  }) {
    if (item is Album) {
      return _buildMobileMediaRowAlbum(
        item: item,
        itemWidth: itemWidth,
        coverSize: coverSize,
        onPlayTap: () => _onPlayTap(index),
        onItemTap: onItemTap,
      );
    }
    if (item is Playlist) {
      return _buildMobileMediaRowPlaylist(
        item: item,
        itemWidth: itemWidth,
        coverSize: coverSize,
        onPlayTap: () => _onPlayTap(index),
        onItemTap: onItemTap,
        showDetails: showPlaylistDetails,
      );
    }
    if (item is Artist) {
      return _buildMobileMediaRowArtist(
        item: item,
        itemWidth: itemWidth,
        coverSize: coverSize,
        onPlayTap: () => _onPlayTap(index),
        onItemTap: onItemTap,
      );
    }
    if (item is Track) {
      return _buildMobileMediaRowTrack(
        item: item,
        itemWidth: itemWidth,
        coverSize: coverSize,
        onPlayTap: () => _onPlayTap(index),
        onItemTap: onItemTap,
        playbackCoordinator: playbackCoordinator,
      );
    }
    return _buildMobileMediaRowUnknown(
      itemWidth: itemWidth,
      coverSize: coverSize,
      onPlayTap: () => _onPlayTap(index),
      onItemTap: onItemTap,
    );
  }

  Widget _buildMobileTypedRowSkeleton() {
    if (T == Album) {
      return _buildMobileMediaRowAlbumSkeleton(
        itemWidth: itemWidth,
        coverSize: coverSize,
      );
    }
    if (T == Playlist) {
      return _buildMobileMediaRowPlaylistSkeleton(
        itemWidth: itemWidth,
        coverSize: coverSize,
        showDetails: showPlaylistDetails,
      );
    }
    if (T == Artist) {
      return _buildMobileMediaRowArtistSkeleton(
        itemWidth: itemWidth,
        coverSize: coverSize,
      );
    }
    if (T == Track) {
      return _buildMobileMediaRowTrackSkeleton(
        itemWidth: itemWidth,
        coverSize: coverSize,
      );
    }
    return _buildMobileMediaRowUnknownSkeleton(
      itemWidth: itemWidth,
      coverSize: coverSize,
    );
  }

  VoidCallback? _itemTapCallback(T item) {
    final ValueChanged<T>? onTap = onItemTap;
    return onTap == null ? null : () => onTap(item);
  }

  int get _visibleItemCount => items.length;
}

// Typed mobile row builders
Widget _buildMobileMediaRowAlbum({
  required Album item,
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
  required VoidCallback? onItemTap,
}) {
  return _buildMobileMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.album_outlined,
    title: _prefer(item.name, 'Unknown Album'),
    secondaryText: _albumSubtitle(item),
    tertiaryText: _albumTertiary(item),
    coverUrl: item.imageUriForSize(256) ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
    onPlayTap: onPlayTap,
    onItemTap: onItemTap,
  );
}

Widget _buildMobileMediaRowPlaylist({
  required Playlist item,
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
  required VoidCallback? onItemTap,
  required bool showDetails,
}) {
  return _buildMobileMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.queue_music_outlined,
    title: _prefer(item.name, 'Unknown Playlist'),
    secondaryText: showDetails ? _playlistSubtitle(item) : null,
    tertiaryText: null,
    coverUrl: item.imageUriForSize(256) ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
    onPlayTap: onPlayTap,
    onItemTap: onItemTap,
  );
}

Widget _buildMobileMediaRowArtist({
  required Artist item,
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
  required VoidCallback? onItemTap,
}) {
  return _buildMobileMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    coverBorderRadius: coverSize / 2,
    icon: Icons.person_outlined,
    title: _prefer(item.name, 'Unknown Artist'),
    secondaryText: null,
    tertiaryText: null,
    coverUrl: item.imageUriForSize(256) ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
    onPlayTap: onPlayTap,
    onItemTap: onItemTap,
  );
}

Widget _buildMobileMediaRowTrack({
  required Track item,
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
  required VoidCallback? onItemTap,
  PlaybackCoordinator? playbackCoordinator,
}) {
  return _buildMobileMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.music_note_outlined,
    title: _prefer(item.name, 'Unknown Track'),
    secondaryText: _trackSubtitle(item),
    tertiaryText: null,
    coverUrl: item.imageUriForSize(256) ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
    onPlayTap: onPlayTap,
    onItemTap: onItemTap,
    playbackCoordinator: playbackCoordinator,
    playbackItem: item,
  );
}

Widget _buildMobileMediaRowUnknown({
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
  required VoidCallback? onItemTap,
}) {
  return _buildMobileMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.music_note_outlined,
    title: 'Unknown Item',
    secondaryText: '-',
    tertiaryText: '-',
    coverUrl: '',
    providerLabel: '-',
    providerIconSvg: null,
    onPlayTap: onPlayTap,
    onItemTap: onItemTap,
  );
}

Widget _buildMobileMediaRowItemBase({
  required double itemWidth,
  required double coverSize,
  required IconData icon,
  double? coverBorderRadius,
  required String title,
  required String? secondaryText,
  required String? tertiaryText,
  required String? coverUrl,
  required String providerLabel,
  required String? providerIconSvg,
  required VoidCallback? onPlayTap,
  required VoidCallback? onItemTap,
  bool showPlayOverlay = true,
  PlaybackCoordinator? playbackCoordinator,
  MediaItem? playbackItem,
}) {
  return Builder(
    builder: (BuildContext context) {
      final colors = context.mashinColors;
      final bool hasSecondary =
          secondaryText != null && secondaryText.trim().isNotEmpty;
      final bool hasTertiary =
          tertiaryText != null && tertiaryText.trim().isNotEmpty;
      final double effectiveCoverRadius = coverBorderRadius ?? 8;
      final double coverScale = MobileSectionSelectionScope.coverScaleOf(
        context,
      );

      return GestureDetector(
        onTap: onItemTap,
        behavior: HitTestBehavior.opaque,
        child: SizedBox(
          width: itemWidth,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.center,
            children: <Widget>[
              Stack(
                children: <Widget>[
                  Transform.scale(
                    alignment: Alignment.center,
                    scale: coverScale,
                    child: ClipRRect(
                      borderRadius: BorderRadius.circular(effectiveCoverRadius),
                      child: Container(
                        width: coverSize,
                        height: coverSize,
                        color: colors.bgTertiary,
                        child: coverUrl != null && coverUrl.trim().isNotEmpty
                            ? _DeferredRowMediaImage(
                                source: coverUrl,
                                fit: BoxFit.cover,
                                fallback: Icon(
                                  icon,
                                  color: colors.textSecondary,
                                  size: 56,
                                ),
                              )
                            : Icon(icon, color: colors.textSecondary, size: 56),
                      ),
                    ),
                  ),
                  if (showPlayOverlay)
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
                    child: IgnorePointer(
                      child: MediaProviderIcon(
                        providerLabel: providerLabel,
                        iconSvg: providerIconSvg,
                        size: 18,
                      ),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 8),
              if (playbackCoordinator == null || playbackItem == null)
                _MobileRowTitleText(title: title, isCurrent: false)
              else
                PlaybackAwareBuilder(
                  coordinator: playbackCoordinator,
                  item: playbackItem,
                  builder: (BuildContext context, bool isCurrent) {
                    return _MobileRowTitleText(
                      title: title,
                      isCurrent: isCurrent,
                    );
                  },
                ),
              if (hasSecondary) ...<Widget>[
                const SizedBox(height: 2),
                Text(
                  secondaryText,
                  maxLines: 1,
                  textAlign: TextAlign.center,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: colors.textSecondary, fontSize: 12),
                ),
              ],
              if (hasTertiary) ...<Widget>[
                const SizedBox(height: 2),
                Text(
                  tertiaryText,
                  maxLines: 1,
                  textAlign: TextAlign.center,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: colors.textTertiary, fontSize: 11),
                ),
              ],
            ],
          ),
        ),
      );
    },
  );
}

class _MobileRowTitleText extends StatelessWidget {
  const _MobileRowTitleText({required this.title, required this.isCurrent});

  final String title;
  final bool isCurrent;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Text(
      title,
      maxLines: 1,
      textAlign: TextAlign.center,
      overflow: TextOverflow.ellipsis,
      style: Theme.of(context).textTheme.titleSmall?.copyWith(
        fontWeight: FontWeight.w600,
        color: isCurrent ? colors.accent : null,
      ),
    );
  }
}

class _DeferredRowMediaImage extends StatefulWidget {
  const _DeferredRowMediaImage({
    required this.source,
    required this.fit,
    required this.fallback,
  });

  final String source;
  final BoxFit fit;
  final Widget fallback;

  @override
  State<_DeferredRowMediaImage> createState() => _DeferredRowMediaImageState();
}

class _DeferredRowMediaImageState extends State<_DeferredRowMediaImage> {
  bool _hasPendingRecheck = false;

  @override
  void didUpdateWidget(covariant _DeferredRowMediaImage oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.source != widget.source) {
      _hasPendingRecheck = false;
    }
  }

  void _scheduleRecheck() {
    if (_hasPendingRecheck) {
      return;
    }

    _hasPendingRecheck = true;
    Future<void>.delayed(const Duration(milliseconds: 80), () {
      if (!mounted) {
        return;
      }
      _hasPendingRecheck = false;
      setState(() {});
    });
  }

  @override
  Widget build(BuildContext context) {
    if (Scrollable.recommendDeferredLoadingForContext(context)) {
      _scheduleRecheck();
      return widget.fallback;
    }

    return MediaImage(
      source: widget.source,
      fit: widget.fit,
      fallback: widget.fallback,
    );
  }
}

// Typed mobile row skeleton builders
Widget _buildMobileMediaRowAlbumSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.album_outlined,
  );
}

Widget _buildMobileMediaRowPlaylistSkeleton({
  required double itemWidth,
  required double coverSize,
  required bool showDetails,
}) {
  return _buildMobileMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.queue_music_outlined,
    lineCount: showDetails ? 3 : 1,
  );
}

Widget _buildMobileMediaRowArtistSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.person_outlined,
    coverBorderRadius: coverSize / 2,
    lineCount: 1,
  );
}

Widget _buildMobileMediaRowTrackSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.music_note_outlined,
    lineCount: 2,
  );
}

Widget _buildMobileMediaRowUnknownSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.music_note_outlined,
  );
}

Widget _buildMobileMediaRowSkeletonBase({
  required double itemWidth,
  required double coverSize,
  required IconData icon,
  double? coverBorderRadius,
  int lineCount = 3,
}) {
  return Builder(
    builder: (BuildContext context) {
      final colors = context.mashinColors;
      final double titleWidth = (itemWidth * 0.72)
          .clamp(48, itemWidth)
          .toDouble();
      final double? secondaryWidth = lineCount > 1
          ? (itemWidth * 0.56).clamp(42, itemWidth).toDouble()
          : null;
      final double? tertiaryWidth = lineCount > 2
          ? (itemWidth * 0.46).clamp(36, itemWidth).toDouble()
          : null;
      final double effectiveCoverRadius = coverBorderRadius ?? 8;

      Widget line(double width) {
        return Container(
          width: width,
          height: 10,
          decoration: BoxDecoration(
            color: colors.bgTertiary,
            borderRadius: BorderRadius.circular(6),
          ),
        );
      }

      return SizedBox(
        width: itemWidth,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: <Widget>[
            Stack(
              children: <Widget>[
                ClipRRect(
                  borderRadius: BorderRadius.circular(effectiveCoverRadius),
                  child: Container(
                    width: coverSize,
                    height: coverSize,
                    color: colors.bgTertiary,
                    child: Icon(icon, color: colors.textSecondary, size: 56),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 8),
            line(titleWidth),
            if (secondaryWidth != null) ...<Widget>[
              const SizedBox(height: 6),
              line(secondaryWidth),
            ],
            if (tertiaryWidth != null) ...<Widget>[
              const SizedBox(height: 6),
              line(tertiaryWidth),
            ],
          ],
        ),
      );
    },
  );
}

// Helper
String _stripOrEmpty(String? value) {
  return value?.trim() ?? '';
}

String _prefer(String primary, String fallback) {
  final a = primary.trim();
  if (a.isNotEmpty) {
    return a;
  }
  return fallback.trim();
}

String _albumSubtitle(Album album) {
  final artist = _firstArtistName(album.artists);
  if (artist != '-') {
    return artist;
  }
  return _mediaTypeLabel(album.albumType);
}

String _trackSubtitle(Track track) {
  final artist = _firstArtistName(track.artists);
  if (artist != '-') {
    return artist;
  }
  return _prefer(track.album?.name ?? '', 'Track');
}

String _albumTertiary(Album album) {
  if (album.year != null && album.year! > 0) {
    return '${album.year}';
  }
  return 'Album';
}

String _playlistSubtitle(Playlist playlist) {
  final owner = _stripOrEmpty(playlist.owner);
  if (owner.isNotEmpty) {
    return owner;
  }
  return 'Playlist';
}

String _mediaTypeLabel(String? rawType) {
  final value = rawType?.trim().toLowerCase() ?? '';
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

  return artists.first.name;
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
