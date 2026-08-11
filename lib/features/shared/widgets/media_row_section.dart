import 'package:flutter/material.dart';

import '../../../app/theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';

class MediaRowCardData {
  final IconData icon;
  final String title;
  final String? secondaryText;
  final String? tertiaryText;
  final String? coverUrl;
  final bool showPlayOverlay;

  const MediaRowCardData({
    required this.icon,
    required this.title,
    this.secondaryText,
    this.tertiaryText,
    this.coverUrl,
    this.showPlayOverlay = true,
  });
}

class MediaRowSection<T> extends StatelessWidget {
  final String title;
  final bool isLoading;
  final List<T> items;
  final String emptyText;
  final MediaRowCardData Function(T item) cardMapper;
  final double itemWidth;
  final double coverSize;
  final double itemSpacing;
  final int maxVisibleItems;
  final double height;
  final int skeletonCount;

  const MediaRowSection({
    super.key,
    required this.title,
    required this.isLoading,
    required this.items,
    required this.emptyText,
    required this.cardMapper,
    this.itemWidth = 164,
    this.coverSize = 132,
    this.itemSpacing = 8,
    this.maxVisibleItems = 20,
    this.height = 226,
    this.skeletonCount = 8,
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
                  separatorBuilder: (_, __) => SizedBox(width: itemSpacing),
                  itemBuilder: (_, __) => RowCardSkeleton(
                    itemWidth: itemWidth,
                    coverSize: coverSize,
                  ),
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
                      separatorBuilder: (_, __) => SizedBox(width: itemSpacing),
                      itemBuilder: (BuildContext context, int index) {
                        final data = cardMapper(items[index]);
                        return BaseMediaRowCard(
                          itemWidth: itemWidth,
                          coverSize: coverSize,
                          icon: data.icon,
                          title: data.title,
                          secondaryText: data.secondaryText,
                          tertiaryText: data.tertiaryText,
                          coverUrl: data.coverUrl,
                          showPlayOverlay: data.showPlayOverlay,
                        );
                      },
                    ),
        ),
      ],
    );
  }
}

class BaseMediaRowCard extends StatelessWidget {
  final double itemWidth;
  final double coverSize;
  final IconData icon;
  final String title;
  final String? secondaryText;
  final String? tertiaryText;
  final String? coverUrl;
  final bool showPlayOverlay;

  const BaseMediaRowCard({
    super.key,
    this.itemWidth = 164,
    this.coverSize = 132,
    required this.icon,
    required this.title,
    this.secondaryText,
    this.tertiaryText,
    this.coverUrl,
    this.showPlayOverlay = true,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final hasSecondary = secondaryText != null && secondaryText!.trim().isNotEmpty;
    final hasTertiary = tertiaryText != null && tertiaryText!.trim().isNotEmpty;

    return SizedBox(
      width: itemWidth,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: <Widget>[
          Stack(
            children: <Widget>[
              ClipRRect(
                borderRadius: BorderRadius.circular(12),
                child: Container(
                  width: coverSize,
                  height: coverSize,
                  color: colors.bgTertiary,
                  child: coverUrl != null && coverUrl!.trim().isNotEmpty
                      ? Image.network(
                          coverUrl!,
                          fit: BoxFit.cover,
                          errorBuilder: (_, __, ___) => Icon(icon, color: colors.accent, size: 34),
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
                      Icons.play_arrow_rounded,
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
  }
}

class RowCardSkeleton extends StatelessWidget {
  final double itemWidth;
  final double coverSize;

  const RowCardSkeleton({
    super.key,
    this.itemWidth = 164,
    this.coverSize = 132,
  });

  @override
  Widget build(BuildContext context) {
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
  }
}

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

String _albumSubtitle(MaAlbum album) {
  final artist = firstArtistName(album.artists);
  if (artist != '-') {
    return artist;
  }
  return _mediaTypeLabel(album.albumType);
}

String _trackSubtitle(MaTrack track) {
  final artist = firstArtistName(track.artists);
  if (artist != '-') {
    return artist;
  }
  return _prefer(track.album?.name ?? '', 'Track');
}

String _trackTertiary(MaTrack track) {
  final albumName = _stripOrEmpty(track.album?.name);
  if (albumName.isNotEmpty) {
    return albumName;
  }
  return formatDuration(track.duration);
}

String _albumTertiary(MaAlbum album) {
  if (album.year != null && album.year! > 0) {
    return '${album.year}';
  }
  return 'Album';
}

String _playlistSubtitle(MaPlaylist playlist) {
  final owner = _stripOrEmpty(playlist.owner);
  if (owner.isNotEmpty) {
    return owner;
  }
  return 'Playlist';
}

String _playlistTertiary(MaPlaylist playlist) {
  return playlist.isEditable ? 'Editierbar' : 'Readonly';
}

String _artistTertiary(MaArtist artist) {
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

MediaRowCardData albumRowCardData(MaAlbum album) {
  return MediaRowCardData(
    icon: Icons.album_rounded,
    title: _prefer(album.name, 'Unknown Album'),
    secondaryText: _albumSubtitle(album),
    tertiaryText: _albumTertiary(album),
    coverUrl: album.imageUri ?? '',
  );
}

MediaRowCardData playlistRowCardData(MaPlaylist playlist) {
  return MediaRowCardData(
    icon: Icons.queue_music_rounded,
    title: _prefer(playlist.name, 'Unknown Playlist'),
    secondaryText: _playlistSubtitle(playlist),
    tertiaryText: _playlistTertiary(playlist),
    coverUrl: playlist.imageUri ?? '',
  );
}

MediaRowCardData artistRowCardData(MaArtist artist) {
  return MediaRowCardData(
    icon: Icons.person_rounded,
    title: _prefer(artist.name, 'Unknown Artist'),
    secondaryText: 'Artist',
    tertiaryText: _artistTertiary(artist),
    coverUrl: artist.imageUri ?? '',
  );
}

MediaRowCardData trackRowCardData(MaTrack track) {
  return MediaRowCardData(
    icon: Icons.music_note_rounded,
    title: _prefer(track.name, 'Unknown Track'),
    secondaryText: _trackSubtitle(track),
    tertiaryText: _trackTertiary(track),
    coverUrl: track.imageUri ?? '',
  );
}

String firstArtistName(List<MaArtist> artists) {
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
