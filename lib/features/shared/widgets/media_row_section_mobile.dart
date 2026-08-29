import 'package:flutter/material.dart';
import 'dart:async';

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../../playback/playback_coordinator.dart';
import 'media_table_section_desktop.dart';
import 'media_image.dart';

class MediaRowSectionMobile<T> extends StatelessWidget {
  static const double _artistHeight = 142;

  final String title;
  final bool isLoading;
  final List<T> items;
  final String emptyText;
  final double itemWidth;
  final double coverSize;
  final double itemSpacing;
  final int maxVisibleItems;
  final double height;
  final int skeletonCount;
  final PlaybackCoordinator? playbackCoordinator;

  const MediaRowSectionMobile({
    super.key,
    required this.title,
    required this.isLoading,
    required this.items,
    required this.emptyText,
    this.itemWidth = 142,
    this.coverSize = 112,
    this.itemSpacing = 8,
    this.maxVisibleItems = 12,
    this.height = 198,
    this.skeletonCount = 6,
    this.playbackCoordinator,
  });

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
    final double sectionHeight = T == Artist ? _artistHeight : height;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        if (hasTitle)
          Text(
            title,
            style: Theme.of(
              context,
            ).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
          ),
        if (hasTitle) const SizedBox(height: 10),
        if (!isLoading && items.isEmpty)
          Text(emptyText, style: TextStyle(color: colors.textSecondary))
        else
          SizedBox(
            height: sectionHeight,
            child: isLoading
                ? ListView.separated(
                    scrollDirection: Axis.horizontal,
                    itemCount: skeletonCount,
                    separatorBuilder: (_, _) => SizedBox(width: itemSpacing),
                    itemBuilder: (_, _) {
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
                    },
                  )
                : ListView.separated(
                    scrollDirection: Axis.horizontal,
                    itemCount: items.length > maxVisibleItems
                        ? maxVisibleItems
                        : items.length,
                    separatorBuilder: (_, _) => SizedBox(width: itemSpacing),
                    itemBuilder: (BuildContext context, int index) {
                      final Object? item = items[index];
                      if (item is Album) {
                        return _buildMobileMediaRowAlbum(
                          item: item,
                          itemWidth: itemWidth,
                          coverSize: coverSize,
                          onPlayTap: () => _onPlayTap(index),
                        );
                      }
                      if (item is Playlist) {
                        return _buildMobileMediaRowPlaylist(
                          item: item,
                          itemWidth: itemWidth,
                          coverSize: coverSize,
                          onPlayTap: () => _onPlayTap(index),
                        );
                      }
                      if (item is Artist) {
                        return _buildMobileMediaRowArtist(
                          item: item,
                          itemWidth: itemWidth,
                          coverSize: coverSize,
                          onPlayTap: () => _onPlayTap(index),
                        );
                      }
                      if (item is Track) {
                        return _buildMobileMediaRowTrack(
                          item: item,
                          itemWidth: itemWidth,
                          coverSize: coverSize,
                          onPlayTap: () => _onPlayTap(index),
                          playbackCoordinator: playbackCoordinator,
                        );
                      }

                      return _buildMobileMediaRowUnknown(
                        itemWidth: itemWidth,
                        coverSize: coverSize,
                        onPlayTap: () => _onPlayTap(index),
                      );
                    },
                  ),
          ),
      ],
    );
  }
}

// Typed mobile row builders
Widget _buildMobileMediaRowAlbum({
  required Album item,
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
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
  );
}

Widget _buildMobileMediaRowPlaylist({
  required Playlist item,
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
}) {
  return _buildMobileMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.queue_music_outlined,
    title: _prefer(item.name, 'Unknown Playlist'),
    secondaryText: _playlistSubtitle(item),
    tertiaryText: _playlistTertiary(item),
    coverUrl: item.imageUriForSize(256) ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
    onPlayTap: onPlayTap,
  );
}

Widget _buildMobileMediaRowArtist({
  required Artist item,
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
}) {
  return _buildMobileMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.person_outlined,
    title: _prefer(item.name, 'Unknown Artist'),
    secondaryText: null,
    tertiaryText: null,
    coverUrl: item.imageUriForSize(256) ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
    onPlayTap: onPlayTap,
  );
}

Widget _buildMobileMediaRowTrack({
  required Track item,
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
  PlaybackCoordinator? playbackCoordinator,
}) {
  return _buildMobileMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.music_note_outlined,
    title: _prefer(item.name, 'Unknown Track'),
    secondaryText: _trackSubtitle(item),
    tertiaryText: _trackTertiary(item),
    coverUrl: item.imageUriForSize(256) ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
    onPlayTap: onPlayTap,
    playbackCoordinator: playbackCoordinator,
    playbackItem: item,
  );
}

Widget _buildMobileMediaRowUnknown({
  required double itemWidth,
  required double coverSize,
  required VoidCallback? onPlayTap,
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
  );
}

Widget _buildMobileMediaRowItemBase({
  required double itemWidth,
  required double coverSize,
  required IconData icon,
  required String title,
  required String? secondaryText,
  required String? tertiaryText,
  required String? coverUrl,
  required String providerLabel,
  required String? providerIconSvg,
  required VoidCallback? onPlayTap,
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

      return SizedBox(
        width: itemWidth,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: <Widget>[
            Stack(
              children: <Widget>[
                ClipRRect(
                  borderRadius: BorderRadius.circular(8),
                  child: Container(
                    width: coverSize,
                    height: coverSize,
                    color: colors.bgTertiary,
                    child: coverUrl != null && coverUrl.trim().isNotEmpty
                        ? MediaImage(
                            source: coverUrl,
                            fit: BoxFit.cover,
                            fallback: Icon(
                              icon,
                              color: colors.accent,
                              size: 28,
                            ),
                          )
                        : Icon(icon, color: colors.accent, size: 28),
                  ),
                ),
                Positioned(
                  left: 6,
                  top: 6,
                  child: MediaProviderIcon(
                    providerLabel: providerLabel,
                    iconSvg: providerIconSvg,
                    size: 18,
                  ),
                ),
                if (showPlayOverlay)
                  Positioned(
                    left: 6,
                    bottom: 6,
                    child: GestureDetector(
                      behavior: HitTestBehavior.opaque,
                      onTap: onPlayTap,
                      child: Container(
                        width: 40,
                        height: 40,
                        decoration: BoxDecoration(
                          color: colors.overlayPlayButtonBackground,
                          borderRadius: BorderRadius.circular(20),
                        ),
                        child: Icon(
                          Icons.play_arrow_outlined,
                          size: 20,
                          color: colors.overlayPlayButtonForeground,
                        ),
                      ),
                    ),
                  ),
              ],
            ),
            const SizedBox(height: 8),
            PlaybackAwareBuilder(
              coordinator: playbackCoordinator,
              item: playbackItem,
              builder: (BuildContext context, bool isCurrent) {
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
      );
    },
  );
}

// Typed mobile row skeleton builders
Widget _buildMobileMediaRowAlbumSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
  );
}

Widget _buildMobileMediaRowPlaylistSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
  );
}

Widget _buildMobileMediaRowArtistSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
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
  );
}

Widget _buildMobileMediaRowUnknownSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowSkeletonBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
  );
}

Widget _buildMobileMediaRowSkeletonBase({
  required double itemWidth,
  required double coverSize,
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
                  borderRadius: BorderRadius.circular(8),
                  child: Container(
                    width: coverSize,
                    height: coverSize,
                    color: colors.bgTertiary,
                  ),
                ),
                Positioned(
                  left: 6,
                  top: 6,
                  child: Container(
                    width: 18,
                    height: 18,
                    decoration: BoxDecoration(
                      color: colors.bgSecondary,
                      borderRadius: BorderRadius.circular(9),
                    ),
                  ),
                ),
                Positioned(
                  left: 6,
                  bottom: 6,
                  child: Container(
                    width: 40,
                    height: 40,
                    decoration: BoxDecoration(
                      color: colors.bgSecondary,
                      borderRadius: BorderRadius.circular(20),
                      border: Border.all(color: colors.border),
                    ),
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

String _trackTertiary(Track track) {
  final albumName = _stripOrEmpty(track.album?.name);
  if (albumName.isNotEmpty) {
    return albumName;
  }
  return _formatDuration(track.duration);
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

  final duration = Duration(seconds: seconds);
  final minutes = duration.inMinutes;
  final remainingSeconds = duration.inSeconds % 60;

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
