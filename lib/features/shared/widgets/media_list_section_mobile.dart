import 'dart:async';

import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';
import '../../core/core_bridge.dart';
import '../../core/core_events.dart';
import '../../core/core_models.dart';
import '../selection/mobile_section_selection.dart';
import 'detail_provider_switcher.dart';
import 'media_list_section_desktop.dart';
import 'media_table_section_desktop.dart';
import 'media_track_row_skeleton.dart';
import 'mobile_section_header.dart';

class MediaListSectionMobile<T extends MediaItem> extends StatefulWidget {
  final String title;
  final List<T> items;
  final bool isLoading;
  final String emptyText;
  final double itemWidth;
  final MashinCoreBridgeApi? coreBridge;
  final MashinCoreBridgeEvents? coreEvents;
  final ValueChanged<T>? onItemTap;
  final ValueChanged<List<T>>? onSelectionMenuTap;

  const MediaListSectionMobile({
    super.key,
    required this.title,
    required this.items,
    required this.isLoading,
    required this.emptyText,
    this.itemWidth = 320,
    this.coreBridge,
    this.coreEvents,
    this.onItemTap,
    this.onSelectionMenuTap,
  });

  bool get shouldShowLoadingPlaceholder => isLoading && items.isEmpty;

  @override
  State<MediaListSectionMobile<T>> createState() =>
      _MediaListSectionMobileState<T>();
}

class _MediaListSectionMobileState<T extends MediaItem>
    extends State<MediaListSectionMobile<T>> {
  static const double _mobileChunkViewportFraction = 0.92;
  static const int _rowsPerColumn = 3;
  static const double _columnGap = 8;

  final PageController _mobileChunkPageController = PageController(
    viewportFraction: _mobileChunkViewportFraction,
  );
  late final MobileSectionSelectionState _selectionController;

  // Selection and viewport lifecycle.
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
  void didUpdateWidget(covariant MediaListSectionMobile<T> oldWidget) {
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

  // Selection menu commands.
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

  // Playback commands.
  void _onPlayAllTap() {
    final MashinCoreBridgeApi? bridge = widget.coreBridge;
    final List<MediaItem> playItems = _playableItems();
    if (bridge == null || playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await bridge.queuePlayMedia(
          _isTrackSection ? playItems : <MediaItem>[playItems.first],
        );
      } catch (_) {
        // Playback failures should not block header actions.
      }
    }());
  }

  void _onShuffleAllTap() {
    final MashinCoreBridgeApi? bridge = widget.coreBridge;
    final List<MediaItem> playItems = _playableItems();
    if (bridge == null || playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await bridge.queuePlayMedia(playItems, shuffle: true);
      } catch (_) {
        // Playback failures should not block header actions.
      }
    }());
  }

  // Playback errors stay local so paging and selection remain usable.
  Future<void> _playTracksFromIndex(int index) async {
    try {
      await widget.coreBridge?.queuePlayMedia(
        widget.items
            .skip(index)
            .where(_isValidPlayable)
            .toList(growable: false),
      );
    } catch (_) {}
  }

  // Playback eligibility and page layout.
  bool get _isTrackSection => T == Track;

  double get _rowExtent => _isTrackSection ? 64 : 50;

  double get _rowGap => _isTrackSection ? 0 : _columnGap;

  List<MediaItem> _playableItems() {
    return widget.items.where(_isValidPlayable).toList(growable: false);
  }

  bool _isValidPlayable(MediaItem item) {
    return item.canPlay &&
        item.itemId.trim().isNotEmpty &&
        item.provider.trim().isNotEmpty;
  }

  int get _visibleColumnCount => (widget.items.length / _rowsPerColumn).ceil();

  // Section and row rendering.
  @override
  Widget build(BuildContext context) {
    final bool hasTitle = widget.title.trim().isNotEmpty;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        if (hasTitle) _buildHeader(context),
        if (hasTitle) const SizedBox(height: 10),
        _buildContent(context),
      ],
    );
  }

  Widget _buildHeader(BuildContext context) {
    final hasPlayableContent =
        !widget.shouldShowLoadingPlaceholder && _playableItems().isNotEmpty;
    return MobileSectionHeader(
      title: widget.title,
      itemCount: !widget.shouldShowLoadingPlaceholder && widget.items.isNotEmpty
          ? widget.items.length
          : null,
      showPlaybackActions: widget.coreBridge != null,
      onPlayTap: hasPlayableContent ? _onPlayAllTap : null,
      onShuffleTap: hasPlayableContent ? _onShuffleAllTap : null,
      shuffleTooltip: 'Zufällig abspielen',
    );
  }

  Widget _buildContent(BuildContext context) {
    if (widget.shouldShowLoadingPlaceholder) return _buildTypedListSkeleton();
    if (widget.items.isEmpty) {
      return Text(
        widget.emptyText,
        style: TextStyle(color: context.mashinColors.textSecondary),
      );
    }
    return SizedBox(
      height: _rowsPerColumn * _rowExtent + (_rowsPerColumn - 1) * _rowGap,
      child: PageView.builder(
        controller: _mobileChunkPageController,
        padEnds: false,
        physics: const PageScrollPhysics(parent: BouncingScrollPhysics()),
        itemCount: _visibleColumnCount,
        itemBuilder: (context, columnIndex) => _buildPage(columnIndex),
      ),
    );
  }

  Widget _buildPage(int columnIndex) {
    final isLastPage = columnIndex == _visibleColumnCount - 1;
    final startIndex = columnIndex * _rowsPerColumn;
    return Padding(
      padding: EdgeInsets.only(right: isLastPage ? 0 : _columnGap),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          for (var row = 0; row < _rowsPerColumn; row++)
            if (startIndex + row < widget.items.length) ...<Widget>[
              if (row > 0) SizedBox(height: _rowGap),
              _buildSelectableListTile(startIndex + row),
            ],
        ],
      ),
    );
  }

  // The selection wrapper owns row gestures independently of child buttons.
  Widget _buildSelectableListTile(int index) {
    if (_isTrackSection) return _buildTrackListTile(index);
    if (widget.items[index] is Playlist) return _buildMediaListTile(index);
    return MobileSelectableSectionItem(
      selectionModeActive: _selectionController.isSelectionModeActive,
      isSelected: _selectionController.isSelected(index),
      onLongPress: () =>
          _selectionController.select(index, widget.items.length),
      onToggleSelection: () =>
          _selectionController.toggle(index, widget.items.length),
      onTap: widget.onItemTap == null
          ? null
          : () => widget.onItemTap!(widget.items[index]),
      child: _buildMediaListTile(index),
    );
  }

  // Loading placeholders follow the section's media type.
  Widget _buildTypedListSkeleton() => _MobileListSkeletonGrid(
    itemWidth: widget.itemWidth,
    isTrack: _isTrackSection,
  );

  // Track rows share the table's ink feedback while child buttons own actions.
  Widget _buildTrackListTile(int index) {
    final item = widget.items[index];
    return _MobileListTrackTile(
      track: item as Track,
      isFirst: index % _rowsPerColumn == 0,
      selectionModeActive: _selectionController.isSelectionModeActive,
      isSelected: _selectionController.isSelected(index),
      onLongPress: () =>
          _selectionController.select(index, widget.items.length),
      onTap: _selectionController.isSelectionModeActive
          ? () => _selectionController.toggle(index, widget.items.length)
          : widget.coreBridge != null && _isValidPlayable(item)
          ? () => unawaited(_playTracksFromIndex(index))
          : null,
      onToggleSelection: () =>
          _selectionController.toggle(index, widget.items.length),
      coreEvents: widget.coreEvents,
      onPlayTap: widget.coreBridge == null
          ? null
          : () => unawaited(_playTracksFromIndex(index)),
      onMoreTap: widget.onSelectionMenuTap == null
          ? null
          : (anchor) => widget.onSelectionMenuTap!(<T>[item]),
    );
  }

  Widget _buildMediaListTile(int index) => _MobileListTile<T>(
    item: widget.items[index],
    coreBridge: widget.coreBridge,
    onItemTap: widget.items[index] is Playlist ? widget.onItemTap : null,
  );

  // Selection changes invalidate only the section's visible state.
  void _onSelectionChanged() {
    if (mounted) {
      setState(() {});
    }
  }
}

class _MobileListTrackTile extends StatelessWidget {
  const _MobileListTrackTile({
    required this.track,
    required this.isFirst,
    required this.selectionModeActive,
    required this.isSelected,
    required this.onLongPress,
    required this.onTap,
    required this.onToggleSelection,
    this.coreEvents,
    this.onPlayTap,
    this.onMoreTap,
  });

  final Track track;
  final bool isFirst;
  final bool selectionModeActive;
  final bool isSelected;
  final VoidCallback onLongPress;
  final VoidCallback? onTap;
  final VoidCallback onToggleSelection;
  final MashinCoreBridgeEvents? coreEvents;
  final VoidCallback? onPlayTap;
  final ValueChanged<BuildContext>? onMoreTap;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final styles = context.mashinTableTextStyles;
    final artists = track.artists
        .map((artist) => artist.name.trim())
        .where((name) => name.isNotEmpty)
        .join(', ');
    final subtitle = artists.isNotEmpty
        ? artists
        : track.album?.name ?? 'Track';
    return Opacity(
      opacity: track.canPlay ? 1 : 0.35,
      child: Material(
        color: Colors.transparent,
        shape: isFirst
            ? const Border()
            : Border(top: BorderSide(color: colors.border)),
        clipBehavior: Clip.antiAlias,
        child: Ink(
          color: isSelected ? colors.selection : Colors.transparent,
          child: InkWell(
            onLongPress: onLongPress,
            onTap: onTap,
            splashColor: colors.selection.withValues(alpha: 0.42),
            highlightColor: colors.selection.withValues(alpha: 0.18),
            child: AbsorbPointer(
              absorbing: selectionModeActive,
              child: SizedBox(
                height: 64,
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(8, 6, 0, 6),
                  child: Row(
                    children: [
                      MediaTableCover(
                        coverUrl: track.imageUriForSize(80),
                        fallbackIcon: Icons.music_note_outlined,
                        size: 52,
                        radius: 8,
                        onPlayTap: track.canPlay ? onPlayTap : null,
                      ),
                      const SizedBox(width: 10),
                      Expanded(
                        child: Column(
                          mainAxisAlignment: MainAxisAlignment.center,
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            PlaybackAwareBuilder(
                              coreEvents: coreEvents,
                              item: track,
                              builder: (context, isCurrent) => Text(
                                track.name.trim().isEmpty
                                    ? 'Unbekannt'
                                    : track.name,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: styles.mobileTitle.copyWith(
                                  color: isCurrent ? colors.accent : null,
                                ),
                              ),
                            ),
                            const SizedBox(height: 1),
                            Text(
                              subtitle,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: styles.mobileSubtitle,
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(width: 10),
                      SizedBox(
                        width: 42,
                        child: Text(
                          formatDuration(track.duration),
                          maxLines: 1,
                          textAlign: TextAlign.right,
                          style: styles.mobileTrailing,
                        ),
                      ),
                      const SizedBox(width: 10),
                      SizedBox(
                        width: 48,
                        child: MediaItemProviderIcon(item: track),
                      ),
                      Builder(
                        builder: (anchor) => SizedBox(
                          width: 48,
                          height: 48,
                          child: Center(
                            child: selectionModeActive
                                ? MobileSelectionCheckBox(
                                    isChecked: isSelected,
                                    isVisible: true,
                                    onTap: onToggleSelection,
                                  )
                                : IconButton(
                                    tooltip: 'Weitere Optionen',
                                    onPressed: onMoreTap == null
                                        ? null
                                        : () => onMoreTap!(anchor),
                                    icon: Icon(
                                      Icons.more_vert_outlined,
                                      size: 18,
                                      color: colors.textSecondary,
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
          ),
        ),
      ),
    );
  }
}

class _MobileListTile<T extends MediaItem> extends StatelessWidget {
  final T item;
  final MashinCoreBridgeApi? coreBridge;
  final ValueChanged<T>? onItemTap;

  const _MobileListTile({
    required this.item,
    required this.coreBridge,
    required this.onItemTap,
  });

  void _play() {
    final MashinCoreBridgeApi? bridge = coreBridge;
    if (bridge == null ||
        !item.canPlay ||
        item.itemId.trim().isEmpty ||
        item.provider.trim().isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await bridge.queuePlayMedia(<MediaItem>[item]);
      } catch (_) {
        // Playback failures should not block list taps.
      }
    }());
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final String title = item.name.trim().isEmpty
        ? 'Unbekannt'
        : item.name.trim();

    return Opacity(
      opacity: item.canPlay ? 1 : 0.35,
      child: GestureDetector(
        onTap: onItemTap == null || (item is Track && !item.canPlay)
            ? null
            : () => onItemTap!(item),
        behavior: HitTestBehavior.opaque,
        child: Container(
          height: 50,
          decoration: BoxDecoration(
            color: colors.bgSecondary,
            borderRadius: BorderRadius.circular(10),
          ),
          clipBehavior: Clip.antiAlias,
          child: Row(
            children: <Widget>[
              Container(width: 10, color: accentColorFromText(title)),
              Expanded(
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 12),
                  child: Text(
                    title,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: colors.textSecondary,
                      fontWeight: FontWeight.w500,
                      fontSize: 14,
                    ),
                  ),
                ),
              ),
              GestureDetector(
                onTap: item.canPlay ? _play : null,
                child: Container(
                  width: 28,
                  height: 28,
                  margin: const EdgeInsets.only(right: 4),
                  decoration: BoxDecoration(
                    color: colors.overlayPlayButtonBackground.withValues(
                      alpha: 0.88,
                    ),
                    borderRadius: BorderRadius.circular(24),
                  ),
                  child: Icon(
                    Icons.play_arrow_sharp,
                    size: 18,
                    color: colors.overlayPlayButtonForeground,
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _MobileListSkeletonGrid extends StatelessWidget {
  final double itemWidth;
  final bool isTrack;

  const _MobileListSkeletonGrid({
    required this.itemWidth,
    required this.isTrack,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return SizedBox(
      height: isTrack ? 192 : 166,
      child: PageView.builder(
        controller: PageController(viewportFraction: 0.92),
        padEnds: false,
        physics: const PageScrollPhysics(parent: BouncingScrollPhysics()),
        itemCount: 3,
        itemBuilder: (BuildContext context, int columnIndex) {
          return Padding(
            padding: EdgeInsets.only(right: columnIndex == 2 ? 0 : 8),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                for (int row = 0; row < 3; row++) ...<Widget>[
                  if (row > 0 && !isTrack) const SizedBox(height: 8),
                  if (isTrack)
                    MediaTrackRowSkeleton(
                      isFirst: row == 0,
                      durationColumnWidth: 42,
                      providerColumnWidth: 48,
                      menuColumnWidth: 48,
                      menuGap: 0,
                      trailingPadding: 0,
                    )
                  else
                    SizedBox(
                      height: 50,
                      child: DecoratedBox(
                        decoration: BoxDecoration(
                          color: colors.bgSecondary,
                          borderRadius: BorderRadius.circular(10),
                        ),
                        child: Row(
                          children: <Widget>[
                            Container(width: 10, color: colors.bgTertiary),
                            const SizedBox(width: 12),
                            Expanded(
                              child: Container(
                                height: 20,
                                decoration: BoxDecoration(
                                  color: colors.bgTertiary,
                                  borderRadius: BorderRadius.circular(8),
                                ),
                              ),
                            ),
                            const SizedBox(width: 12),
                          ],
                        ),
                      ),
                    ),
                ],
              ],
            ),
          );
        },
      ),
    );
  }
}
