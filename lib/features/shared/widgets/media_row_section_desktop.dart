import 'package:flutter/material.dart';
import 'dart:math' as math;

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';

class MediaRowSectionDesktop<T> extends StatefulWidget {
  static const double _defaultHeight = 226;
  static const double _reservedTextHeight = 66;

  final String title;
  final bool isLoading;
  final List<T> items;
  final String emptyText;
  final double minItemWith;
  final double itemSpacing;
  final int skeletonCount;

  const MediaRowSectionDesktop({
    super.key,
    required this.title,
    required this.isLoading,
    required this.items,
    required this.emptyText,
    this.minItemWith = 160,
    this.itemSpacing = 16,
    this.skeletonCount = 8,
  });

  @override
  State<MediaRowSectionDesktop<T>> createState() => _MediaRowSectionDesktopState<T>();
}

class _MediaRowSectionDesktopState<T> extends State<MediaRowSectionDesktop<T>> {
  static const Duration _pageOutDuration = Duration(milliseconds: 220);
  static const Duration _pageInDuration = Duration(milliseconds: 220);
  static const Curve _pageTransitionCurve = Curves.easeInOutCubic;
  static const double _chunkTranslatePx = 50;

  int _pageStartIndex = 0;
  int _navigationDirection = 1;
  int? _outgoingStartIndex;
  int? _incomingStartIndex;
  bool _isAnimatingOut = false;
  bool _isAnimatingIn = false;
  int _transitionRunId = 0;

  _RowLayoutMetrics _computeLayoutMetrics(double maxWidth) {
    final int itemCount = widget.items.length;

    if (itemCount <= 0) {
      return const _RowLayoutMetrics(visibleCount: 1, computedItemWidth: 0);
    }

    final double minItemWidth = widget.minItemWith > 0 ? widget.minItemWith : 164;
    final double spacing = widget.itemSpacing;

    if (!maxWidth.isFinite || maxWidth <= 0) {
      final int fallbackVisible = math.max(1, itemCount);
      return _RowLayoutMetrics(visibleCount: fallbackVisible, computedItemWidth: minItemWidth);
    }

    final double denominator = minItemWidth + spacing;
    int fitByMinSize = denominator > 0 ? ((maxWidth + spacing) / denominator).floor() : 1;
    fitByMinSize = math.max(1, fitByMinSize);

    final int visibleCount = math.min(itemCount, fitByMinSize);
    final double computedWidth = (maxWidth - ((visibleCount - 1) * spacing)) / visibleCount;

    return _RowLayoutMetrics(
      visibleCount: visibleCount,
      computedItemWidth: math.max(minItemWidth, computedWidth),
    );
  }

  double _resolveCoverSize(double resolvedItemWidth) {
    return math.max(1, resolvedItemWidth);
  }

  @override
  void didUpdateWidget(covariant MediaRowSectionDesktop<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.items.isEmpty) {
      _pageStartIndex = 0;
      return;
    }
    if (_pageStartIndex >= widget.items.length) {
      _pageStartIndex = widget.items.length - 1;
    }
    if (_pageStartIndex < 0) {
      _pageStartIndex = 0;
    }
  }

  int _clampStartIndex({required int visibleCount}) {
    if (widget.items.isEmpty) {
      return 0;
    }
    final int pageCount = (widget.items.length / visibleCount).ceil();
    final int maxStart = math.max(0, (pageCount - 1) * visibleCount);
    return _pageStartIndex.clamp(0, maxStart);
  }

  bool get _isTransitionRunning => _isAnimatingOut || _isAnimatingIn;

  void _finishRunningTransitionImmediately() {
    if (!_isTransitionRunning) {
      return;
    }

    _transitionRunId++;
    if (_incomingStartIndex != null) {
      _pageStartIndex = _incomingStartIndex!;
    }

    _outgoingStartIndex = null;
    _incomingStartIndex = null;
    _isAnimatingOut = false;
    _isAnimatingIn = false;
  }

  Future<void> _startPageTransition({
    required int targetStartIndex,
    required int direction,
  }) async {
    if (_isTransitionRunning || targetStartIndex == _pageStartIndex) {
      return;
    }

    final int runId = ++_transitionRunId;

    setState(() {
      _navigationDirection = direction;
      _outgoingStartIndex = _pageStartIndex;
      _incomingStartIndex = targetStartIndex;
      _isAnimatingOut = true;
      _isAnimatingIn = false;
    });

    await Future<void>.delayed(_pageOutDuration);
    if (!mounted || runId != _transitionRunId) {
      return;
    }

    setState(() {
      _pageStartIndex = targetStartIndex;
      _outgoingStartIndex = null;
      _isAnimatingOut = false;
      _isAnimatingIn = true;
    });

    await Future<void>.delayed(_pageInDuration);
    if (!mounted || runId != _transitionRunId) {
      return;
    }

    setState(() {
      _incomingStartIndex = null;
      _isAnimatingIn = false;
    });
  }

  List<T> _sliceItems({required int startIndex, required int visibleCount}) {
    if (widget.items.isEmpty) {
      return <T>[];
    }
    final int safeStart = startIndex.clamp(0, widget.items.length - 1);
    final int end = math.min(widget.items.length, safeStart + visibleCount);
    return widget.items.sublist(safeStart, end);
  }

  Widget _buildSlideList({
    required int startIndex,
    required int visibleCount,
    required double resolvedItemWidth,
    required double resolvedCoverSize,
  }) {
    final List<T> visibleItems = _sliceItems(startIndex: startIndex, visibleCount: visibleCount);

    return ListView.separated(
      key: ValueKey<int>(startIndex),
      scrollDirection: Axis.horizontal,
      physics: const NeverScrollableScrollPhysics(),
      itemCount: visibleItems.length,
      separatorBuilder: (_, _) => SizedBox(width: widget.itemSpacing),
      itemBuilder: (BuildContext context, int index) {
        final Object? item = visibleItems[index];
        if (item is Album) {
          return _buildDesktopMediaRowAlbum(
            item: item,
            itemWidth: resolvedItemWidth,
            coverSize: resolvedCoverSize,
          );
        }
        if (item is Playlist) {
          return _buildDesktopMediaRowPlaylist(
            item: item,
            itemWidth: resolvedItemWidth,
            coverSize: resolvedCoverSize,
          );
        }
        if (item is Artist) {
          return _buildDesktopMediaRowArtist(
            item: item,
            itemWidth: resolvedItemWidth,
            coverSize: resolvedCoverSize,
          );
        }
        if (item is Track) {
          return _buildDesktopMediaRowTrack(
            item: item,
            itemWidth: resolvedItemWidth,
            coverSize: resolvedCoverSize,
          );
        }

        return _buildDesktopMediaRowUnknown(
          itemWidth: resolvedItemWidth,
          coverSize: resolvedCoverSize,
        );
      },
    );
  }

  void _goToPreviousPage(int visibleCount) {
    if (_isTransitionRunning) {
      setState(_finishRunningTransitionImmediately);
    }

    if (_pageStartIndex <= 0) {
      return;
    }

    final int target = math.max(0, _pageStartIndex - visibleCount);
    _startPageTransition(targetStartIndex: target, direction: -1);
  }

  void _goToNextPage(int visibleCount, int pageCount) {
    if (_isTransitionRunning) {
      setState(_finishRunningTransitionImmediately);
    }

    final int maxStart = math.max(0, (pageCount - 1) * visibleCount);
    if (_pageStartIndex >= maxStart) {
      return;
    }
    final int target = math.min(maxStart, _pageStartIndex + visibleCount);
    _startPageTransition(targetStartIndex: target, direction: 1);
  }

  Widget _buildPaginationControls(BuildContext context, {required int visibleCount, required int pageCount}) {
    final int clampedStart = _clampStartIndex(visibleCount: visibleCount);
    final int maxStart = math.max(0, (pageCount - 1) * visibleCount);
    final bool canGoBack = clampedStart > 0;
    final bool canGoNext = clampedStart < maxStart;

    return Row(
      mainAxisSize: MainAxisSize.min,
      children: <Widget>[
        IconButton(
          tooltip: 'Vorherige Seite',
          onPressed: canGoBack ? () => _goToPreviousPage(visibleCount) : null,
          icon: const Icon(Icons.chevron_left_outlined),
          visualDensity: VisualDensity.compact,
        ),
        IconButton(
          tooltip: 'Nächste Seite',
          onPressed: canGoNext ? () => _goToNextPage(visibleCount, pageCount) : null,
          icon: const Icon(Icons.chevron_right_outlined),
          visualDensity: VisualDensity.compact,
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return LayoutBuilder(
      builder: (BuildContext context, BoxConstraints constraints) {
        final _RowLayoutMetrics metrics = _computeLayoutMetrics(constraints.maxWidth);
        final int visibleCount = metrics.visibleCount;
        final double resolvedItemWidth = metrics.computedItemWidth;
        final double resolvedCoverSize = _resolveCoverSize(resolvedItemWidth);
        final double sectionHeight = math.max(
          MediaRowSectionDesktop._defaultHeight,
          resolvedCoverSize + MediaRowSectionDesktop._reservedTextHeight,
        );
        final int pageCount = widget.items.isEmpty ? 1 : (widget.items.length / visibleCount).ceil();
        final bool isPagingEnabled = pageCount > 1;
        _pageStartIndex = _clampStartIndex(visibleCount: visibleCount);

        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Row(
              children: <Widget>[
                Expanded(
                  child: Text(
                    widget.title,
                    style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
                  ),
                ),
                if (isPagingEnabled && !widget.isLoading && widget.items.isNotEmpty)
                  _buildPaginationControls(context, visibleCount: visibleCount, pageCount: pageCount),
              ],
            ),
            const SizedBox(height: 10),
            SizedBox(
              height: sectionHeight,
              child: widget.isLoading
                  ? ListView.separated(
                      scrollDirection: Axis.horizontal,
                      itemCount: widget.skeletonCount,
                      separatorBuilder: (_, _) => SizedBox(width: widget.itemSpacing),
                      itemBuilder: (_, _) {
                        if (T == Album) {
                          return _buildDesktopMediaRowAlbumSkeleton(
                            itemWidth: resolvedItemWidth,
                            coverSize: resolvedCoverSize,
                          );
                        }
                        if (T == Playlist) {
                          return _buildDesktopMediaRowPlaylistSkeleton(
                            itemWidth: resolvedItemWidth,
                            coverSize: resolvedCoverSize,
                          );
                        }
                        if (T == Artist) {
                          return _buildDesktopMediaRowArtistSkeleton(
                            itemWidth: resolvedItemWidth,
                            coverSize: resolvedCoverSize,
                          );
                        }
                        if (T == Track) {
                          return _buildDesktopMediaRowTrackSkeleton(
                            itemWidth: resolvedItemWidth,
                            coverSize: resolvedCoverSize,
                          );
                        }

                        return _buildDesktopMediaRowUnknownSkeleton(
                          itemWidth: resolvedItemWidth,
                          coverSize: resolvedCoverSize,
                        );
                      },
                    )
                  : widget.items.isEmpty
                      ? Container(
                          width: double.infinity,
                          padding: const EdgeInsets.all(16),
                          decoration: BoxDecoration(
                            borderRadius: BorderRadius.circular(12),
                            border: Border.all(color: colors.border),
                            color: colors.bgSecondary,
                          ),
                          child: Align(
                            alignment: Alignment.centerLeft,
                            child: Text(
                              widget.emptyText,
                              style: TextStyle(color: colors.textSecondary),
                            ),
                          ),
                        )
                      : ClipRect(
                          child: Stack(
                            fit: StackFit.expand,
                            children: <Widget>[
                              if (_isAnimatingOut && _outgoingStartIndex != null)
                                TweenAnimationBuilder<double>(
                                  key: ValueKey<int>(_outgoingStartIndex!),
                                  tween: Tween<double>(begin: 0, end: 1),
                                  duration: _pageOutDuration,
                                  curve: _pageTransitionCurve,
                                  builder: (BuildContext context, double value, Widget? child) {
                                    final double dx = (_navigationDirection >= 0 ? -1 : 1) * _chunkTranslatePx * value;
                                    return Opacity(
                                      opacity: 1 - value,
                                      child: Transform.translate(
                                        offset: Offset(dx, 0),
                                        child: child,
                                      ),
                                    );
                                  },
                                  child: _buildSlideList(
                                    startIndex: _outgoingStartIndex!,
                                    visibleCount: visibleCount,
                                    resolvedItemWidth: resolvedItemWidth,
                                    resolvedCoverSize: resolvedCoverSize,
                                  ),
                                )
                              else if (_isAnimatingIn && _incomingStartIndex != null)
                                TweenAnimationBuilder<double>(
                                  key: ValueKey<int>(_incomingStartIndex!),
                                  tween: Tween<double>(begin: 0, end: 1),
                                  duration: _pageInDuration,
                                  curve: _pageTransitionCurve,
                                  builder: (BuildContext context, double value, Widget? child) {
                                    final double dx = (_navigationDirection >= 0 ? 1 : -1) * _chunkTranslatePx * (1 - value);
                                    return Opacity(
                                      opacity: value,
                                      child: Transform.translate(
                                        offset: Offset(dx, 0),
                                        child: child,
                                      ),
                                    );
                                  },
                                  child: _buildSlideList(
                                    startIndex: _incomingStartIndex!,
                                    visibleCount: visibleCount,
                                    resolvedItemWidth: resolvedItemWidth,
                                    resolvedCoverSize: resolvedCoverSize,
                                  ),
                                )
                              else
                                _buildSlideList(
                                  startIndex: _pageStartIndex,
                                  visibleCount: visibleCount,
                                  resolvedItemWidth: resolvedItemWidth,
                                  resolvedCoverSize: resolvedCoverSize,
                                ),
                            ],
                          ),
                        ),
            ),
          ],
        );
      },
    );
  }
}

class _RowLayoutMetrics {
  final int visibleCount;
  final double computedItemWidth;

  const _RowLayoutMetrics({
    required this.visibleCount,
    required this.computedItemWidth,
  });
}

// Typed desktop row builders
Widget _buildDesktopMediaRowAlbum({
  required Album item,
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.album_outlined,
    title: _prefer(item.name, 'Unknown Album'),
    secondaryText: _albumSubtitle(item),
    tertiaryText: _albumTertiary(item),
    coverUrl: item.imageUri ?? '',
  );
}

Widget _buildDesktopMediaRowPlaylist({
  required Playlist item,
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.queue_music_outlined,
    title: _prefer(item.name, 'Unknown Playlist'),
    secondaryText: _playlistSubtitle(item),
    tertiaryText: _playlistTertiary(item),
    coverUrl: item.imageUri ?? '',
  );
}

Widget _buildDesktopMediaRowArtist({
  required Artist item,
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.person_outlined,
    title: _prefer(item.name, 'Unknown Artist'),
    secondaryText: 'Artist',
    tertiaryText: _artistTertiary(item),
    coverUrl: item.imageUri ?? '',
    isCircularCover: true,
  );
}

Widget _buildDesktopMediaRowTrack({
  required Track item,
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.music_note_outlined,
    title: _prefer(item.name, 'Unknown Track'),
    secondaryText: _trackSubtitle(item),
    tertiaryText: _trackTertiary(item),
    coverUrl: item.imageUri ?? '',
  );
}

Widget _buildDesktopMediaRowUnknown({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.music_note_outlined,
    title: 'Unknown Item',
    secondaryText: '-',
    tertiaryText: '-',
    coverUrl: '',
  );
}

Widget _buildDesktopMediaRowItemBase({
  required double itemWidth,
  required double coverSize,
  required IconData icon,
  required String title,
  required String? secondaryText,
  required String? tertiaryText,
  required String? coverUrl,
  bool showPlayOverlay = true,
  bool isCircularCover = false,
}) {
  return Builder(
    builder: (BuildContext context) {
      final colors = context.mashinColors;
      final bool hasSecondary = secondaryText != null && secondaryText.trim().isNotEmpty;
      final bool hasTertiary = tertiaryText != null && tertiaryText.trim().isNotEmpty;

      return SizedBox(
        width: itemWidth,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: <Widget>[
            Stack(
              children: <Widget>[
                if (isCircularCover)
                  ClipOval(
                    child: Container(
                      width: coverSize,
                      height: coverSize,
                      color: colors.bgTertiary,
                      child: coverUrl != null && coverUrl.trim().isNotEmpty
                          ? Image.network(
                              coverUrl,
                              fit: BoxFit.cover,
                              errorBuilder: (_, _, _) => Icon(icon, color: colors.accent, size: 34),
                            )
                          : Icon(icon, color: colors.accent, size: 34),
                    ),
                  )
                else
                  ClipRRect(
                    borderRadius: BorderRadius.circular(12),
                    child: Container(
                      width: coverSize,
                      height: coverSize,
                      color: colors.bgTertiary,
                      child: coverUrl != null && coverUrl.trim().isNotEmpty
                          ? Image.network(
                              coverUrl,
                              fit: BoxFit.cover,
                              errorBuilder: (_, _, _) => Icon(icon, color: colors.accent, size: 34),
                            )
                          : Icon(icon, color: colors.accent, size: 34),
                    ),
                  ),
                if (showPlayOverlay)
                  Positioned(
                    left: 6,
                    bottom: 6,
                    child: Container(
                      width: 38,
                      height: 38,
                      decoration: BoxDecoration(
                        color: Theme.of(context).colorScheme.surface.withValues(alpha: 0.9),
                        borderRadius: BorderRadius.circular(19),
                        border: Border.all(color: colors.border),
                      ),
                      child: Icon(
                        Icons.play_arrow_outlined,
                        size: 22,
                        color: Theme.of(context).colorScheme.onSurface,
                      ),
                    ),
                  ),
              ],
            ),
            const SizedBox(height: 8),
            Text(
              title,
              maxLines: 1,
              textAlign: TextAlign.center,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(context).textTheme.titleSmall?.copyWith(fontWeight: FontWeight.w700),
            ),
            if (hasSecondary) ...<Widget>[
              const SizedBox(height: 2),
              Text(
                secondaryText!,
                maxLines: 1,
                textAlign: TextAlign.center,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: colors.textSecondary, fontSize: 12),
              ),
            ],
            if (hasTertiary) ...<Widget>[
              const SizedBox(height: 2),
              Text(
                tertiaryText!,
                maxLines: 1,
                textAlign: TextAlign.center,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: colors.textSecondary, fontSize: 12),
              ),
            ],
          ],
        ),
      );
    },
  );
}

// Typed desktop row skeleton builders
Widget _buildDesktopMediaRowAlbumSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildDesktopMediaRowPlaylistSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildDesktopMediaRowArtistSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildDesktopMediaRowTrackSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildDesktopMediaRowUnknownSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildDesktopMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildDesktopMediaRowSkeletonBase({
  required double itemWidth,
  required double coverSize,
}) {
  return Builder(
    builder: (BuildContext context) {
      final colors = context.mashinColors;

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
            Container(
              width: coverSize,
              height: coverSize,
              decoration: BoxDecoration(
                borderRadius: BorderRadius.circular(12),
                color: colors.bgTertiary,
              ),
            ),
            const SizedBox(height: 8),
            line(108),
            const SizedBox(height: 6),
            line(86),
            const SizedBox(height: 6),
            line(72),
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
  final artist = firstArtistName(album.artists);
  if (artist != '-') {
    return artist;
  }
  return _mediaTypeLabel(album.albumType);
}

String _trackSubtitle(Track track) {
  final artist = firstArtistName(track.artists);
  if (artist != '-') {
    return artist;
  }
  return _prefer(track.album?.name ?? '', 'Track');
}

String _trackTertiary(Track track) {
  final albumName = _stripOrEmpty(track.album?.name);
  if (albumName.isNotEmpty) {
    return albumName;
  }
  return formatDuration(track.duration);
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

String _playlistTertiary(Playlist playlist) {
  return playlist.isEditable ? 'Editierbar' : 'Readonly';
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

String firstArtistName(List<Artist> artists) {
  if (artists.isEmpty) {
    return '-';
  }

  return artists.first.name;
}

String formatDuration(int seconds) {
  if (seconds <= 0) {
    return '0:00';
  }

  final duration = Duration(seconds: seconds);
  final minutes = duration.inMinutes;
  final remainingSeconds = duration.inSeconds % 60;

  return '$minutes:${remainingSeconds.toString().padLeft(2, '0')}';
}
