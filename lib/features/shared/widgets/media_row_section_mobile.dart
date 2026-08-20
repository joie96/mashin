import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';
import 'media_table_section_desktop.dart';

class MediaRowSectionMobile<T> extends StatelessWidget {
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
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text(
          title,
          style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
        ),
        const SizedBox(height: 10),
        SizedBox(
          height: height,
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
              : items.isEmpty
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
                          emptyText,
                          style: TextStyle(color: colors.textSecondary),
                        ),
                      ),
                    )
                  : ListView.separated(
                      scrollDirection: Axis.horizontal,
                      itemCount: items.length > maxVisibleItems ? maxVisibleItems : items.length,
                      separatorBuilder: (_, _) => SizedBox(width: itemSpacing),
                      itemBuilder: (BuildContext context, int index) {
                        final Object? item = items[index];
                        if (item is Album) {
                          return _buildMobileMediaRowAlbum(
                            item: item,
                            itemWidth: itemWidth,
                            coverSize: coverSize,
                          );
                        }
                        if (item is Playlist) {
                          return _buildMobileMediaRowPlaylist(
                            item: item,
                            itemWidth: itemWidth,
                            coverSize: coverSize,
                          );
                        }
                        if (item is Artist) {
                          return _buildMobileMediaRowArtist(
                            item: item,
                            itemWidth: itemWidth,
                            coverSize: coverSize,
                          );
                        }
                        if (item is Track) {
                          return _buildMobileMediaRowTrack(
                            item: item,
                            itemWidth: itemWidth,
                            coverSize: coverSize,
                          );
                        }

                        return _buildMobileMediaRowUnknown(
                          itemWidth: itemWidth,
                          coverSize: coverSize,
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
}) {
  return _buildMobileMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.album_outlined,
    title: _prefer(item.name, 'Unknown Album'),
    secondaryText: _albumSubtitle(item),
    tertiaryText: _albumTertiary(item),
    coverUrl: item.imageUri ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
  );
}

Widget _buildMobileMediaRowPlaylist({
  required Playlist item,
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.queue_music_outlined,
    title: _prefer(item.name, 'Unknown Playlist'),
    secondaryText: _playlistSubtitle(item),
    tertiaryText: _playlistTertiary(item),
    coverUrl: item.imageUri ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
  );
}

Widget _buildMobileMediaRowArtist({
  required Artist item,
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.person_outlined,
    title: _prefer(item.name, 'Unknown Artist'),
    secondaryText: 'Artist',
    tertiaryText: _artistTertiary(item),
    coverUrl: item.imageUri ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
  );
}

Widget _buildMobileMediaRowTrack({
  required Track item,
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowItemBase(
    itemWidth: itemWidth,
    coverSize: coverSize,
    icon: Icons.music_note_outlined,
    title: _prefer(item.name, 'Unknown Track'),
    secondaryText: _trackSubtitle(item),
    tertiaryText: _trackTertiary(item),
    coverUrl: item.imageUri ?? '',
    providerLabel: _providerLabelFromId(item.provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(item.provider),
  );
}

Widget _buildMobileMediaRowUnknown({
  required double itemWidth,
  required double coverSize,
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
  bool showPlayOverlay = true,
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
                ClipRRect(
                  borderRadius: BorderRadius.circular(8),
                  child: Container(
                    width: coverSize,
                    height: coverSize,
                    color: colors.bgTertiary,
                    child: coverUrl != null && coverUrl.trim().isNotEmpty
                        ? Image.network(
                            coverUrl,
                            fit: BoxFit.cover,
                            errorBuilder: (_, _, _) => Icon(icon, color: colors.accent, size: 28),
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
                    child: Container(
                      width: 40,
                      height: 40,
                      decoration: BoxDecoration(
                        color: Theme.of(context).colorScheme.surface.withValues(alpha: 0.9),
                        borderRadius: BorderRadius.circular(20),
                        border: Border.all(color: colors.border),
                      ),
                      child: Icon(
                        Icons.play_arrow_outlined,
                        size: 20,
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
                style: TextStyle(color: colors.textSecondary, fontSize: 11),
              ),
            ],
            if (hasTertiary) ...<Widget>[
              const SizedBox(height: 2),
              Text(
                tertiaryText!,
                maxLines: 1,
                textAlign: TextAlign.center,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: colors.textSecondary, fontSize: 11),
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
  return _buildMobileMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildMobileMediaRowPlaylistSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildMobileMediaRowArtistSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildMobileMediaRowTrackSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildMobileMediaRowUnknownSkeleton({
  required double itemWidth,
  required double coverSize,
}) {
  return _buildMobileMediaRowSkeletonBase(itemWidth: itemWidth, coverSize: coverSize);
}

Widget _buildMobileMediaRowSkeletonBase({
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
            line(98),
            const SizedBox(height: 6),
            line(78),
            const SizedBox(height: 6),
            line(66),
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
