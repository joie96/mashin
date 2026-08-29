import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../../playback/playback_coordinator.dart';
import 'media_link_text.dart';
import 'media_table_section_desktop.dart';

class MediaListSectionDesktop<T extends MediaItem> extends StatelessWidget {
  final String title;
  final List<T> items;
  final bool isLoading;
  final String emptyText;
  final int maxItems;
  final PlaybackCoordinator? playbackCoordinator;
  final ValueChanged<T>? onItemTap;
  final DesktopRowContextMenuAction<T>? onItemContextMenu;

  const MediaListSectionDesktop({
    super.key,
    required this.title,
    required this.items,
    required this.isLoading,
    required this.emptyText,
    this.maxItems = 9,
    this.playbackCoordinator,
    this.onItemTap,
    this.onItemContextMenu,
  });

  List<MediaItem> _playableItems() {
    return items.where(_isValidPlayable).toList(growable: false);
  }

  void _onPlayAllTap() {
    final PlaybackCoordinator? coordinator = playbackCoordinator;
    final List<MediaItem> playItems = _playableItems();
    if (coordinator == null || playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        await coordinator.playMedia(<MediaItem>[playItems.first]);
      } catch (_) {
        // Playback failures should not block header actions.
      }
    }());
  }

  void _onShuffleAllTap() {
    final PlaybackCoordinator? coordinator = playbackCoordinator;
    final List<MediaItem> playItems = _playableItems();
    if (coordinator == null || playItems.isEmpty) {
      return;
    }

    unawaited(() async {
      try {
        final MediaItem item =
            playItems[math.Random().nextInt(playItems.length)];
        await coordinator.playMedia(<MediaItem>[item]);
      } catch (_) {
        // Playback failures should not block header actions.
      }
    }());
  }

  @override
  Widget build(BuildContext context) {
    final bool hasTitle = title.trim().isNotEmpty;
    final int visibleCount = math.min(maxItems, items.length);
    final bool hasPlayableContent = !isLoading && _playableItems().isNotEmpty;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        if (hasTitle)
          Row(
            children: <Widget>[
              Text(
                title,
                style: Theme.of(
                  context,
                ).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
              ),
              if (!isLoading && items.isNotEmpty) ...<Widget>[
                const SizedBox(width: 10),
                SectionCountBadge(count: items.length),
              ],
              const SizedBox(width: 16),
              const Expanded(child: SectionHeaderDivider()),
              const SizedBox(width: 16),
              if (playbackCoordinator != null) ...<Widget>[
                SectionHeaderIconButton(
                  icon: Icons.play_arrow,
                  tooltip: 'Abspielen',
                  onTap: hasPlayableContent ? _onPlayAllTap : null,
                ),
                const SizedBox(width: 8),
                SectionHeaderIconButton(
                  icon: Icons.shuffle,
                  tooltip: 'Zufällig abspielen',
                  onTap: hasPlayableContent ? _onShuffleAllTap : null,
                ),
              ],
            ],
          ),
        if (hasTitle) const SizedBox(height: 10),
        if (isLoading)
          const _DesktopListSkeletonGrid()
        else if (items.isEmpty)
          Text(emptyText, style: context.mashinTableTextStyles.emptyState)
        else
          LayoutBuilder(
            builder: (BuildContext context, BoxConstraints constraints) {
              final int columnCount = _resolveColumnCount(
                constraints.maxWidth,
                visibleCount,
              );
              return GridView.builder(
                primary: false,
                shrinkWrap: true,
                physics: const NeverScrollableScrollPhysics(),
                gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
                  crossAxisCount: columnCount,
                  crossAxisSpacing: 12,
                  mainAxisSpacing: 12,
                  mainAxisExtent: 48,
                ),
                itemCount: visibleCount,
                itemBuilder: (BuildContext context, int index) {
                  return _MediaListTile<T>(
                    item: items[index],
                    playbackCoordinator: playbackCoordinator,
                    onItemTap: onItemTap,
                    onContextMenu: onItemContextMenu == null
                        ? null
                        : (Offset position) => onItemContextMenu!(
                            items[index],
                            index,
                            position,
                          ),
                  );
                },
              );
            },
          ),
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: SizedBox(height: 24),
        ),
      ],
    );
  }

  int _resolveColumnCount(double width, int itemCount) {
    if (!width.isFinite || width <= 0) {
      return math.min(3, math.max(1, itemCount));
    }

    final int count = (width / 220).floor();
    return count.clamp(1, math.min(6, math.max(1, itemCount)));
  }
}

class _MediaListTile<T extends MediaItem> extends StatefulWidget {
  final T item;
  final PlaybackCoordinator? playbackCoordinator;
  final ValueChanged<T>? onItemTap;
  final ValueChanged<Offset>? onContextMenu;

  const _MediaListTile({
    required this.item,
    required this.playbackCoordinator,
    required this.onItemTap,
    required this.onContextMenu,
  });

  @override
  State<_MediaListTile<T>> createState() => _MediaListTileState<T>();
}

class _MediaListTileState<T extends MediaItem>
    extends State<_MediaListTile<T>> {
  bool _isHovered = false;
  bool _isPlayHovered = false;

  void _play() {
    final PlaybackCoordinator? coordinator = widget.playbackCoordinator;
    if (coordinator == null || !_isValidPlayable(widget.item)) {
      return;
    }

    unawaited(() async {
      try {
        await coordinator.playMedia(<MediaItem>[widget.item]);
      } catch (_) {
        // Playback failures should not block list hover actions.
      }
    }());
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final String title = _displayName(widget.item);

    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() {
        _isHovered = false;
        _isPlayHovered = false;
      }),
      child: Listener(
        onPointerDown: (PointerDownEvent event) {
          if (event.kind == PointerDeviceKind.mouse &&
              (event.buttons & kSecondaryMouseButton) != 0) {
            widget.onContextMenu?.call(event.position);
          }
        },
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 150),
          decoration: BoxDecoration(
            color: colors.bgSecondary,
            borderRadius: BorderRadius.circular(5),
            boxShadow: _isHovered
                ? <BoxShadow>[
                    BoxShadow(
                      color: Colors.black.withValues(alpha: 0.20),
                      blurRadius: 8,
                      offset: const Offset(0, 4),
                    ),
                  ]
                : null,
          ),
          clipBehavior: Clip.antiAlias,
          child: Stack(
            children: <Widget>[
            Positioned(
              top: 0,
              bottom: 0,
              left: 0,
              width: 8,
              child: ColoredBox(color: accentColorFromText(title)),
            ),
            Padding(
              padding: const EdgeInsets.only(left: 24, right: 56),
              child: Align(
                alignment: Alignment.centerLeft,
                child: MediaLinkText(
                  items: <MediaItem>[widget.item],
                  fallbackText: title,
                  onItemTap: widget.onItemTap == null
                      ? null
                      : (MediaItem item) => widget.onItemTap!(item as T),
                  style: TextStyle(
                    color: colors.textSecondary,
                    fontWeight: FontWeight.w500,
                  ),
                ),
              ),
            ),
            Positioned(
              top: 8,
              right: 12,
              child: IgnorePointer(
                ignoring: !_isHovered,
                child: AnimatedOpacity(
                  duration: const Duration(milliseconds: 150),
                  opacity: _isHovered ? 1 : 0,
                  child: AnimatedSlide(
                    duration: const Duration(milliseconds: 150),
                    offset: _isHovered ? Offset.zero : const Offset(0.25, 0),
                    child: MouseRegion(
                      onEnter: (_) => setState(() => _isPlayHovered = true),
                      onExit: (_) => setState(() => _isPlayHovered = false),
                      child: AnimatedScale(
                        duration: const Duration(milliseconds: 100),
                        scale: _isPlayHovered ? 1.1 : 1,
                        child: GestureDetector(
                          onTap: _play,
                          child: Container(
                            width: 32,
                            height: 32,
                            decoration: BoxDecoration(
                              color: colors.overlayPlayButtonBackground,
                              borderRadius: BorderRadius.circular(16),
                            ),
                            child: Icon(
                              Icons.play_arrow_sharp,
                              size: 21,
                              color: colors.overlayPlayButtonForeground,
                            ),
                          ),
                        ),
                      ),
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
}

class _DesktopListSkeletonGrid extends StatelessWidget {
  const _DesktopListSkeletonGrid();

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return GridView.builder(
      primary: false,
      shrinkWrap: true,
      physics: const NeverScrollableScrollPhysics(),
      gridDelegate: const SliverGridDelegateWithFixedCrossAxisCount(
        crossAxisCount: 3,
        crossAxisSpacing: 12,
        mainAxisSpacing: 12,
        mainAxisExtent: 48,
      ),
      itemCount: 9,
      itemBuilder: (BuildContext context, int index) {
        return DecoratedBox(
          decoration: BoxDecoration(
            color: colors.bgSecondary,
            borderRadius: BorderRadius.circular(5),
          ),
          child: Row(
            children: <Widget>[
              Container(width: 8, color: colors.bgTertiary),
              const SizedBox(width: 16),
              Container(
                width: 160,
                height: 16,
                decoration: BoxDecoration(
                  color: colors.bgTertiary,
                  borderRadius: BorderRadius.circular(8),
                ),
              ),
            ],
          ),
        );
      },
    );
  }
}

String _displayName(MediaItem item) {
  final String name = item.name.trim();
  return name.isEmpty ? 'Unbekannt' : name;
}

bool _isValidPlayable(MediaItem item) {
  return item.itemId.trim().isNotEmpty && item.provider.trim().isNotEmpty;
}

Color accentColorFromText(String text) {
  final String normalized = text.trim();
  if (normalized.isEmpty) {
    return const Color(0xFF7CA8FF);
  }

  final int hash = _stableTextHash('121212:$normalized');
  final double hue = (hash & 0xFFFF) % 360;
  final double saturation = 0.56 + (((hash >> 16) & 0xFF) / 255 * 0.16);
  final double lightness = 0.50 + (((hash >> 24) & 0xFF) / 255 * 0.12);
  return HSLColor.fromAHSL(1, hue, saturation, lightness).toColor();
}

int _stableTextHash(String text) {
  int hash = 2166136261;
  for (final int unit in text.codeUnits) {
    hash ^= unit;
    hash = (hash * 16777619) & 0xFFFFFFFF;
  }
  return hash;
}
