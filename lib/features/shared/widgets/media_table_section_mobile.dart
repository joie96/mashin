import 'dart:math' as math;
import 'dart:async';

import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../../playback/playback_coordinator.dart';
import '../selection/selection_controller.dart';
import 'media_table_section_desktop.dart';

class MediaTableSectionMobile<T> extends StatefulWidget {
  final String title;
  final List<T> items;
  final bool isLoading;
  final String emptyText;
  final int skeletonRowCount;
  final int maxItemsPerPage;
  final bool enableSelection;
  final DesktopRowAction<T>? onRowTap;
  final ValueChanged<List<T>>? onSelectionChanged;
  final VoidCallback? onSelectionMenuTap;
  final PlaybackCoordinator? playbackCoordinator;

  const MediaTableSectionMobile({
    super.key,
    required this.title,
    required this.items,
    required this.isLoading,
    required this.emptyText,
    this.skeletonRowCount = 10,
    this.maxItemsPerPage = 10,
    this.enableSelection = true,
    this.onRowTap,
    this.onSelectionChanged,
    this.onSelectionMenuTap,
    this.playbackCoordinator,
  });

  @override
  State<MediaTableSectionMobile<T>> createState() =>
      _MediaTableSectionMobileState<T>();
}

class _MediaTableSectionMobileState<T>
    extends State<MediaTableSectionMobile<T>> {
  static const double _mobileRowExtent = 64;
  static const double _mobileChunkViewportFraction = 0.92;
  static const double _mobileChunkGap = 8;

  final Set<int> _selectedIndexes = <int>{};
  final Object _selectionSessionOwner = Object();
  final SelectionController _selectionSessionController =
      SelectionController.instance;
  final PageController _mobileChunkPageController = PageController(
    viewportFraction: _mobileChunkViewportFraction,
  );

  bool _isMobileSelectionMode = false;

  bool get _isChunkModeEnabled => widget.maxItemsPerPage > 0;
  int get _chunkSize => _isChunkModeEnabled ? widget.maxItemsPerPage : 0;
  bool get _isAllSelected =>
      widget.items.isNotEmpty && _selectedIndexes.length == widget.items.length;
  bool get _isTrackSection => T == Track;

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

  List<MediaItem> _playableItemsFromIndex(int startIndex) {
    if (widget.items.isEmpty ||
        startIndex < 0 ||
        startIndex >= widget.items.length) {
      return const <MediaItem>[];
    }

    final List<MediaItem> playItems = <MediaItem>[];
    for (int i = startIndex; i < widget.items.length; i++) {
      final MediaItem? resolved = _resolvePlayableMediaItem(widget.items[i]);
      if (resolved == null || !_isValidPlayable(resolved)) {
        continue;
      }
      playItems.add(resolved);
    }

    return playItems;
  }

  void _onPlayAllTap() {
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
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
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
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

  Future<bool> _handleRowActivate({required T item, required int index}) async {
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
    if (coordinator == null || index < 0 || index >= widget.items.length) {
      return false;
    }

    final Object? rawItem = item;
    if (rawItem is QueueItem) {
      try {
        await coordinator.playQueueIndex(index);
        return true;
      } catch (_) {
        return false;
      }
    }

    final List<MediaItem> playItems = _playableItemsFromIndex(index);
    if (playItems.isEmpty) {
      return false;
    }

    try {
      await coordinator.playMedia(playItems);
      return true;
    } catch (_) {
      return false;
    }
  }

  void _onRowActivated({required T item, required int index}) {
    final DesktopRowAction<T>? onRowTap = widget.onRowTap;
    if (onRowTap != null) {
      onRowTap(item, index);
      return;
    }

    unawaited(() async {
      final bool handled = await _handleRowActivate(item: item, index: index);
      if (!handled) {
        return;
      }
    }());
  }

  @override
  void didUpdateWidget(covariant MediaTableSectionMobile<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    _selectedIndexes.removeWhere(
      (int index) => index < 0 || index >= widget.items.length,
    );

    if (_selectedIndexes.isEmpty) {
      _isMobileSelectionMode = false;
      _deactivateSelectionSession();
    } else {
      _syncSelectionSession();
    }
  }

  @override
  void dispose() {
    _mobileChunkPageController.dispose();
    _deactivateSelectionSession();
    super.dispose();
  }

  // Selection and Paging
  void _emitSelectionChanged() {
    if (widget.onSelectionChanged == null) {
      return;
    }

    final List<T> selected = _selectedIndexes
        .where((int index) => index >= 0 && index < widget.items.length)
        .map((int index) => widget.items[index])
        .toList(growable: false);
    widget.onSelectionChanged!(selected);
  }

  void _syncSelectionSession() {
    if (_selectedIndexes.isEmpty) {
      _deactivateSelectionSession();
      return;
    }

    _selectionSessionController.activate(
      owner: _selectionSessionOwner,
      registration: SelectionSessionRegistration(
        allSelected: _isAllSelected,
        onToggleAll: _toggleAllSelectedIndexes,
        onMenuTap: widget.onSelectionMenuTap,
        onClose: _clearSelectionFromController,
      ),
    );
  }

  void _deactivateSelectionSession() {
    _selectionSessionController.deactivate(_selectionSessionOwner);
  }

  void _toggleAllSelectedIndexes() {
    if (!widget.enableSelection || widget.items.isEmpty) {
      return;
    }

    setState(() {
      if (_isAllSelected) {
        _selectedIndexes.clear();
      } else {
        _selectedIndexes
          ..clear()
          ..addAll(
            List<int>.generate(widget.items.length, (int index) => index),
          );
      }
    });
    _syncSelectionSession();
    _emitSelectionChanged();
  }

  void _enterSelectionMode(int index) {
    if (!widget.enableSelection || index < 0 || index >= widget.items.length) {
      return;
    }

    setState(() {
      _isMobileSelectionMode = true;
      _selectedIndexes.add(index);
    });
    _syncSelectionSession();
    _emitSelectionChanged();
  }

  void _toggleSelectedIndex(int index) {
    if (!widget.enableSelection ||
        !_isMobileSelectionMode ||
        index < 0 ||
        index >= widget.items.length) {
      return;
    }

    setState(() {
      if (_selectedIndexes.contains(index)) {
        _selectedIndexes.remove(index);
      } else {
        _selectedIndexes.add(index);
      }

      if (_selectedIndexes.isEmpty) {
        _isMobileSelectionMode = false;
        _deactivateSelectionSession();
      } else {
        _syncSelectionSession();
      }
    });
    _emitSelectionChanged();
  }

  void _clearSelectionFromController() {
    if (!mounted) {
      return;
    }
    setState(() {
      _selectedIndexes.clear();
      _isMobileSelectionMode = false;
    });
    _deactivateSelectionSession();
    _emitSelectionChanged();
  }

  // Build
  @override
  Widget build(BuildContext context) {
    final bool hasTitle = widget.title.trim().isNotEmpty;
    final bool hasPlayableContent =
        !widget.isLoading && _playableItemsFromIndex(0).isNotEmpty;

    final Widget section = LayoutBuilder(
      builder: (BuildContext context, BoxConstraints constraints) {
        final bool hasBoundedHeight = constraints.maxHeight.isFinite;
        // Mirror desktop behavior: in non-paged mode, fill the constrained parent
        // and scroll inside the table viewport.
        final bool fillScrollableViewport =
            hasBoundedHeight && !_isChunkModeEnabled;

        final Widget tableContent = _buildMobile(
          context,
          constrainToHeight: hasBoundedHeight,
          fillScrollableViewport: fillScrollableViewport,
        );

        final Widget content = Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: hasBoundedHeight ? MainAxisSize.max : MainAxisSize.min,
          children: <Widget>[
            if (hasTitle)
              Row(
                children: <Widget>[
                  Text(
                    widget.title,
                    style: Theme.of(context).textTheme.titleMedium?.copyWith(
                      fontWeight: FontWeight.w700,
                    ),
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
                      onTap: hasPlayableContent ? _onPlayAllTap : null,
                    ),
                    const SizedBox(width: 8),
                    SectionHeaderIconButton(
                      icon: Icons.shuffle,
                      tooltip: 'Zufällig',
                      onTap: hasPlayableContent ? _onShuffleAllTap : null,
                    ),
                  ],
                ],
              ),
            if (hasTitle) const SizedBox(height: 10),
            if (hasBoundedHeight)
              Expanded(child: ClipRect(child: tableContent))
            else
              tableContent,
          ],
        );

        return content;
      },
    );

    return section;
  }

  // Mobile Table Content
  Widget _buildMobile(
    BuildContext context, {
    required bool constrainToHeight,
    required bool fillScrollableViewport,
  }) {
    final bool useInternalScroll = fillScrollableViewport || constrainToHeight;

    if (widget.isLoading) {
      // Keep loading skeletons scrollable inside bounded viewports to avoid overflow.
      final Widget body = ListView.builder(
        primary: false,
        shrinkWrap: !useInternalScroll,
        physics: useInternalScroll
            ? const ClampingScrollPhysics()
            : const NeverScrollableScrollPhysics(),
        addAutomaticKeepAlives: false,
        addRepaintBoundaries: true,
        addSemanticIndexes: false,
        itemCount: widget.skeletonRowCount,
        itemExtent: _mobileRowExtent,
        itemBuilder: (BuildContext context, int index) {
          return _MobileTableRowSkeleton(isFirst: index == 0);
        },
      );

      return Column(
        mainAxisSize: useInternalScroll ? MainAxisSize.max : MainAxisSize.min,
        children: <Widget>[
          if (useInternalScroll) Expanded(child: body) else body,
        ],
      );
    }

    if (widget.items.isEmpty) {
      return Text(
        widget.emptyText,
        style: context.mashinTableTextStyles.emptyState,
      );
    }

    if (_isChunkModeEnabled) {
      final int chunkCount = (widget.items.length / _chunkSize).ceil();
      final double pageHeight = _chunkSize * _mobileRowExtent;

      return SizedBox(
        height: pageHeight,
        child: PageView.builder(
          controller: _mobileChunkPageController,
          padEnds: false,
          physics: const PageScrollPhysics(parent: BouncingScrollPhysics()),
          itemCount: chunkCount,
          itemBuilder: (BuildContext context, int pageIndex) {
            final int start = pageIndex * _chunkSize;
            final int end = math.min(start + _chunkSize, widget.items.length);
            final List<T> chunkItems = widget.items.sublist(start, end);

            return Padding(
              padding: EdgeInsets.only(
                right: pageIndex == chunkCount - 1 ? 0 : _mobileChunkGap,
              ),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: List<Widget>.generate(_chunkSize, (int localIndex) {
                  if (localIndex >= chunkItems.length) {
                    return const SizedBox(height: _mobileRowExtent);
                  }

                  final int itemIndex = start + localIndex;
                  return SizedBox(
                    height: _mobileRowExtent,
                    child: _buildMobileTypedRow(
                      item: chunkItems[localIndex],
                      index: itemIndex,
                      isFirst: localIndex == 0,
                      isSelected: _selectedIndexes.contains(itemIndex),
                      selectionModeActive: _isMobileSelectionMode,
                      selectionEnabled: widget.enableSelection,
                      onToggleSelected: () => _toggleSelectedIndex(itemIndex),
                      onLongPressSelect: () => _enterSelectionMode(itemIndex),
                      onTap: () => _onRowActivated(
                        item: chunkItems[localIndex],
                        index: itemIndex,
                      ),
                    ),
                  );
                }),
              ),
            );
          },
        ),
      );
    }

    final Widget list = ListView.builder(
      primary: false,
      shrinkWrap: !useInternalScroll,
      physics: useInternalScroll
          ? const ClampingScrollPhysics()
          : const NeverScrollableScrollPhysics(),
      addAutomaticKeepAlives: false,
      addRepaintBoundaries: true,
      addSemanticIndexes: false,
      itemCount: widget.items.length,
      itemExtent: _mobileRowExtent,
      itemBuilder: (BuildContext context, int index) {
        final int itemIndex = index;
        return _buildMobileTypedRow(
          item: widget.items[index],
          index: itemIndex,
          isFirst: index == 0,
          isSelected: _selectedIndexes.contains(itemIndex),
          selectionModeActive: _isMobileSelectionMode,
          selectionEnabled: widget.enableSelection,
          onToggleSelected: () => _toggleSelectedIndex(itemIndex),
          onLongPressSelect: () => _enterSelectionMode(itemIndex),
          onTap: () =>
              _onRowActivated(item: widget.items[index], index: itemIndex),
        );
      },
    );

    return list;
  }

  Widget _buildMobileTypedRow({
    required T item,
    required int index,
    required bool isFirst,
    required bool isSelected,
    required bool selectionModeActive,
    required bool selectionEnabled,
    required VoidCallback onToggleSelected,
    required VoidCallback onLongPressSelect,
    required VoidCallback? onTap,
  }) {
    final Object? typedItem = item;

    if (typedItem is Track) {
      return _buildMobileTableRowTrack(
        item: typedItem,
        isFirst: isFirst,
        isSelected: isSelected,
        selectionModeActive: selectionModeActive,
        selectionEnabled: selectionEnabled,
        onToggleSelected: onToggleSelected,
        onLongPressSelect: onLongPressSelect,
        onTap: onTap,
      );
    }

    if (typedItem is QueueItem) {
      return _buildMobileTableRowQueue(
        item: typedItem,
        isFirst: isFirst,
        isSelected: isSelected,
        selectionModeActive: selectionModeActive,
        selectionEnabled: selectionEnabled,
        onToggleSelected: onToggleSelected,
        onLongPressSelect: onLongPressSelect,
        onTap: onTap,
      );
    }

    if (typedItem is Album) {
      return _buildMobileTableRowAlbum(
        item: typedItem,
        isFirst: isFirst,
        isSelected: isSelected,
        selectionModeActive: selectionModeActive,
        selectionEnabled: selectionEnabled,
        onToggleSelected: onToggleSelected,
        onLongPressSelect: onLongPressSelect,
        onTap: onTap,
      );
    }

    if (typedItem is Playlist) {
      return _buildMobileTableRowPlaylist(
        item: typedItem,
        isFirst: isFirst,
        isSelected: isSelected,
        selectionModeActive: selectionModeActive,
        selectionEnabled: selectionEnabled,
        onToggleSelected: onToggleSelected,
        onLongPressSelect: onLongPressSelect,
        onTap: onTap,
      );
    }

    if (typedItem is Artist) {
      return _buildMobileTableRowArtist(
        item: typedItem,
        isFirst: isFirst,
        isSelected: isSelected,
        selectionModeActive: selectionModeActive,
        selectionEnabled: selectionEnabled,
        onToggleSelected: onToggleSelected,
        onLongPressSelect: onLongPressSelect,
        onTap: onTap,
      );
    }

    return _buildMobileTableRowUnknown(
      itemLabel: typedItem?.toString() ?? '-',
      isFirst: isFirst,
      isSelected: isSelected,
      selectionModeActive: selectionModeActive,
      selectionEnabled: selectionEnabled,
      onToggleSelected: onToggleSelected,
      onLongPressSelect: onLongPressSelect,
      onTap: onTap,
    );
  }

  Widget _buildMobileTableRowTrack({
    required Track item,
    required bool isFirst,
    required bool isSelected,
    required bool selectionModeActive,
    required bool selectionEnabled,
    required VoidCallback onToggleSelected,
    required VoidCallback onLongPressSelect,
    required VoidCallback? onTap,
  }) {
    return _buildMobileTableRowBase(
      title: _prefer(item.name, 'Unknown Track'),
      subtitle: _trackSubtitle(item),
      trailing: _formatDuration(item.duration),
      coverUrl: item.imageUriForSize(80),
      fallbackIcon: Icons.music_note_outlined,
      providerLabel: _providerLabelFromId(item.provider),
      providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(
        item.provider,
      ),
      isFirst: isFirst,
      isSelected: isSelected,
      selectionModeActive: selectionModeActive,
      selectionEnabled: selectionEnabled,
      onToggleSelected: onToggleSelected,
      onLongPressSelect: onLongPressSelect,
      onTap: onTap,
      playbackCoordinator: widget.playbackCoordinator,
      playbackItem: item,
    );
  }

  Widget _buildMobileTableRowQueue({
    required QueueItem item,
    required bool isFirst,
    required bool isSelected,
    required bool selectionModeActive,
    required bool selectionEnabled,
    required VoidCallback onToggleSelected,
    required VoidCallback onLongPressSelect,
    required VoidCallback? onTap,
  }) {
    final String provider = item.mediaItem?.provider.trim() ?? '';
    final String title = (item.mediaItem?.name.trim().isNotEmpty == true)
        ? item.mediaItem!.name.trim()
        : item.name.trim().isNotEmpty
        ? item.name.trim()
        : 'Unbekannter Titel';
    final String subtitle =
        item.mediaItem?.artists
            .map((artist) => artist.name.trim())
            .where((name) => name.isNotEmpty)
            .join(', ') ??
        '';

    return _buildMobileTableRowBase(
      title: title,
      subtitle: subtitle.isEmpty ? '-' : subtitle,
      trailing: _formatQueueDuration((item.duration ?? 0).toDouble()),
      coverUrl: item.mediaItem?.imageUriForSize(80),
      fallbackIcon: Icons.music_note_outlined,
      providerLabel: _providerLabelFromId(provider),
      providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(provider),
      isFirst: isFirst,
      isSelected: isSelected,
      selectionModeActive: selectionModeActive,
      selectionEnabled: selectionEnabled,
      onToggleSelected: onToggleSelected,
      onLongPressSelect: onLongPressSelect,
      onTap: onTap,
      playbackCoordinator: widget.playbackCoordinator,
      playbackItem: item.mediaItem,
    );
  }

  Widget _buildMobileTableRowAlbum({
    required Album item,
    required bool isFirst,
    required bool isSelected,
    required bool selectionModeActive,
    required bool selectionEnabled,
    required VoidCallback onToggleSelected,
    required VoidCallback onLongPressSelect,
    required VoidCallback? onTap,
  }) {
    return _buildMobileTableRowBase(
      title: _prefer(item.name, 'Unknown Album'),
      subtitle: _albumSubtitle(item),
      trailing: item.year != null && item.year! > 0 ? '${item.year}' : '-',
      coverUrl: item.imageUriForSize(80),
      fallbackIcon: Icons.album_outlined,
      providerLabel: _providerLabelFromId(item.provider),
      providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(
        item.provider,
      ),
      isFirst: isFirst,
      isSelected: isSelected,
      selectionModeActive: selectionModeActive,
      selectionEnabled: selectionEnabled,
      onToggleSelected: onToggleSelected,
      onLongPressSelect: onLongPressSelect,
      onTap: onTap,
    );
  }

  Widget _buildMobileTableRowPlaylist({
    required Playlist item,
    required bool isFirst,
    required bool isSelected,
    required bool selectionModeActive,
    required bool selectionEnabled,
    required VoidCallback onToggleSelected,
    required VoidCallback onLongPressSelect,
    required VoidCallback? onTap,
  }) {
    return _buildMobileTableRowBase(
      title: _prefer(item.name, 'Unknown Playlist'),
      subtitle: _playlistSubtitle(item),
      trailing: item.isEditable ? 'Editierbar' : 'Readonly',
      coverUrl: item.imageUriForSize(80),
      fallbackIcon: Icons.queue_music_outlined,
      providerLabel: _providerLabelFromId(item.provider),
      providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(
        item.provider,
      ),
      isFirst: isFirst,
      isSelected: isSelected,
      selectionModeActive: selectionModeActive,
      selectionEnabled: selectionEnabled,
      onToggleSelected: onToggleSelected,
      onLongPressSelect: onLongPressSelect,
      onTap: onTap,
    );
  }

  Widget _buildMobileTableRowArtist({
    required Artist item,
    required bool isFirst,
    required bool isSelected,
    required bool selectionModeActive,
    required bool selectionEnabled,
    required VoidCallback onToggleSelected,
    required VoidCallback onLongPressSelect,
    required VoidCallback? onTap,
  }) {
    return _buildMobileTableRowBase(
      title: _prefer(item.name, 'Unknown Artist'),
      subtitle: 'Artist',
      trailing: _artistTertiary(item),
      coverUrl: item.imageUriForSize(80),
      fallbackIcon: Icons.person_outlined,
      providerLabel: _providerLabelFromId(item.provider),
      providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(
        item.provider,
      ),
      isFirst: isFirst,
      isSelected: isSelected,
      selectionModeActive: selectionModeActive,
      selectionEnabled: selectionEnabled,
      onToggleSelected: onToggleSelected,
      onLongPressSelect: onLongPressSelect,
      onTap: onTap,
    );
  }

  Widget _buildMobileTableRowUnknown({
    required String itemLabel,
    required bool isFirst,
    required bool isSelected,
    required bool selectionModeActive,
    required bool selectionEnabled,
    required VoidCallback onToggleSelected,
    required VoidCallback onLongPressSelect,
    required VoidCallback? onTap,
  }) {
    return _buildMobileTableRowBase(
      title: itemLabel,
      subtitle: '-',
      trailing: '-',
      coverUrl: null,
      fallbackIcon: Icons.music_note_outlined,
      providerLabel: '-',
      providerIconSvg: null,
      isFirst: isFirst,
      isSelected: isSelected,
      selectionModeActive: selectionModeActive,
      selectionEnabled: selectionEnabled,
      onToggleSelected: onToggleSelected,
      onLongPressSelect: onLongPressSelect,
      onTap: onTap,
    );
  }
}

class _MobileTableRowBase extends StatefulWidget {
  final String title;
  final String subtitle;
  final String trailing;
  final String? coverUrl;
  final IconData fallbackIcon;
  final String providerLabel;
  final String? providerIconSvg;
  final bool isFirst;
  final bool isSelected;
  final bool selectionModeActive;
  final bool selectionEnabled;
  final VoidCallback onToggleSelected;
  final VoidCallback onLongPressSelect;
  final VoidCallback? onTap;
  final PlaybackCoordinator? playbackCoordinator;
  final MediaItem? playbackItem;

  const _MobileTableRowBase({
    required this.title,
    required this.subtitle,
    required this.trailing,
    required this.coverUrl,
    required this.fallbackIcon,
    required this.providerLabel,
    required this.providerIconSvg,
    required this.isFirst,
    required this.isSelected,
    required this.selectionModeActive,
    required this.selectionEnabled,
    required this.onToggleSelected,
    required this.onLongPressSelect,
    this.onTap,
    this.playbackCoordinator,
    this.playbackItem,
  });

  @override
  State<_MobileTableRowBase> createState() => _MobileTableRowBaseState();
}

class _MobileTableRowBaseState extends State<_MobileTableRowBase> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final textStyles = context.mashinTableTextStyles;

    Color rowBackground = Colors.transparent;
    if (widget.isSelected) {
      rowBackground = _isHovered ? colors.hover : colors.selection;
    } else if (_isHovered) {
      rowBackground = colors.hover;
    }

    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onLongPress: widget.selectionEnabled ? widget.onLongPressSelect : null,
        onTap: widget.selectionModeActive
            ? widget.onToggleSelected
            : widget.onTap,
        child: Container(
          constraints: const BoxConstraints(minHeight: 64),
          decoration: BoxDecoration(
            color: rowBackground,
            border: widget.isFirst
                ? null
                : Border(top: BorderSide(color: colors.border)),
          ),
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
          child: Row(
            children: <Widget>[
              MediaTableCover(
                coverUrl: widget.coverUrl,
                fallbackIcon: widget.fallbackIcon,
                size: 52,
                radius: 8,
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: <Widget>[
                    PlaybackAwareBuilder(
                      coordinator: widget.playbackCoordinator,
                      item: widget.playbackItem,
                      builder: (BuildContext context, bool isCurrent) {
                        return Text(
                          widget.title,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: textStyles.mobileTitle.copyWith(
                            color: isCurrent ? colors.accent : null,
                          ),
                        );
                      },
                    ),
                    const SizedBox(height: 1),
                    Text(
                      widget.subtitle,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: textStyles.mobileSubtitle,
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 10),
              Text(
                widget.trailing,
                textAlign: TextAlign.right,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: textStyles.mobileTrailing,
              ),
              const SizedBox(width: 10),
              SizedBox(
                width: 20,
                child: Align(
                  alignment: Alignment.center,
                  child: MediaProviderIcon(
                    providerLabel: widget.providerLabel,
                    iconSvg: widget.providerIconSvg,
                    size: 16,
                  ),
                ),
              ),
              const SizedBox(width: 10),
              SizedBox(
                width: 40,
                height: 40,
                child: Center(
                  child: widget.selectionEnabled && widget.selectionModeActive
                      ? _MobileSelectionCheckBox(
                          isChecked: widget.isSelected,
                          isVisible: true,
                          onTap: widget.onToggleSelected,
                        )
                      : Icon(
                          Icons.more_vert_outlined,
                          size: 18,
                          color: colors.textSecondary,
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

Widget _buildMobileTableRowBase({
  required String title,
  required String subtitle,
  required String trailing,
  required String? coverUrl,
  required IconData fallbackIcon,
  required String providerLabel,
  required String? providerIconSvg,
  required bool isFirst,
  required bool isSelected,
  required bool selectionModeActive,
  required bool selectionEnabled,
  required VoidCallback onToggleSelected,
  required VoidCallback onLongPressSelect,
  required VoidCallback? onTap,
  PlaybackCoordinator? playbackCoordinator,
  MediaItem? playbackItem,
}) {
  return _MobileTableRowBase(
    title: title,
    subtitle: subtitle,
    trailing: trailing,
    coverUrl: coverUrl,
    fallbackIcon: fallbackIcon,
    providerLabel: providerLabel,
    providerIconSvg: providerIconSvg,
    isFirst: isFirst,
    isSelected: isSelected,
    selectionModeActive: selectionModeActive,
    selectionEnabled: selectionEnabled,
    onToggleSelected: onToggleSelected,
    onLongPressSelect: onLongPressSelect,
    onTap: onTap,
    playbackCoordinator: playbackCoordinator,
    playbackItem: playbackItem,
  );
}

class _MobileTableRowSkeleton extends StatelessWidget {
  final bool isFirst;

  const _MobileTableRowSkeleton({required this.isFirst});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    Widget skeletonBox({
      required double width,
      required double height,
      double radius = 6,
    }) {
      return Container(
        width: width,
        height: height,
        decoration: BoxDecoration(
          color: colors.bgTertiary,
          borderRadius: BorderRadius.circular(radius),
        ),
      );
    }

    return Container(
      constraints: const BoxConstraints(minHeight: 64),
      decoration: BoxDecoration(
        color: Colors.transparent,
        border: isFirst ? null : Border(top: BorderSide(color: colors.border)),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
      child: Row(
        children: <Widget>[
          skeletonBox(width: 52, height: 52, radius: 8),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisAlignment: MainAxisAlignment.center,
              children: <Widget>[
                skeletonBox(width: 176, height: 15),
                const SizedBox(height: 4),
                skeletonBox(width: 124, height: 12),
              ],
            ),
          ),
          const SizedBox(width: 10),
          skeletonBox(width: 34, height: 12, radius: 4),
          const SizedBox(width: 10),
          SizedBox(
            width: 20,
            child: Center(child: skeletonBox(width: 16, height: 16, radius: 8)),
          ),
          const SizedBox(width: 10),
          SizedBox(
            width: 40,
            height: 40,
            child: Center(child: skeletonBox(width: 18, height: 18, radius: 9)),
          ),
        ],
      ),
    );
  }
}

class _MobileSelectionCheckBox extends StatelessWidget {
  final bool isChecked;
  final bool isVisible;
  final VoidCallback? onTap;

  const _MobileSelectionCheckBox({
    required this.isChecked,
    required this.isVisible,
    this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return IgnorePointer(
      ignoring: !isVisible,
      child: GestureDetector(
        onTap: onTap,
        child: Opacity(
          opacity: isVisible ? 1 : 0,
          child: Container(
            width: 20,
            height: 20,
            decoration: BoxDecoration(
              color: isChecked ? colors.accent : Colors.transparent,
              borderRadius: BorderRadius.circular(10),
              border: Border.all(
                color: isChecked ? colors.accent : colors.textSecondary,
                width: 0.9,
              ),
            ),
            child: isChecked
                ? Icon(
                    Icons.check,
                    size: 14,
                    color: Theme.of(context).colorScheme.onPrimary,
                  )
                : null,
          ),
        ),
      ),
    );
  }
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

String _playlistSubtitle(Playlist playlist) {
  final owner = _stripOrEmpty(playlist.owner);
  if (owner.isNotEmpty) {
    return owner;
  }
  return 'Playlist';
}

String _artistTertiary(Artist artist) {
  final provider = _stripOrEmpty(artist.provider);
  if (provider.isEmpty) {
    return '-';
  }

  final cut = provider.split('--').first.trim();
  return cut.isEmpty ? provider : cut;
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

String _formatDuration(int seconds) {
  if (seconds <= 0) {
    return '0:00';
  }

  final Duration duration = Duration(seconds: seconds);
  final int minutes = duration.inMinutes;
  final int remainingSeconds = duration.inSeconds % 60;
  return '$minutes:${remainingSeconds.toString().padLeft(2, '0')}';
}

String _formatQueueDuration(double seconds) {
  if (!seconds.isFinite || seconds <= 0) {
    return '0:00';
  }

  final int totalSeconds = seconds.round();
  final int minutes = totalSeconds ~/ 60;
  final int remainingSeconds = totalSeconds % 60;
  return '$minutes:${remainingSeconds.toString().padLeft(2, '0')}';
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
