import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import '../shared/widgets/media_image.dart';

class QueueOverlayDesktop extends StatelessWidget {
  final String trackTitle;
  final String trackSubtitle;
  final String? trackCoverUrl;
  final String queueTrackCountText;
  final String queueTotalDurationText;
  final bool dontStopTheMusicEnabled;
  final List<QueueItem> queueItems;
  final int? currentQueueIndex;
  final PlaybackCoordinator playbackCoordinator;
  final Future<void> Function() onToggleDontStopTheMusic;
  final ValueChanged<Artist>? onOpenArtist;
  final ValueChanged<Album>? onOpenAlbum;

  const QueueOverlayDesktop({
    required this.trackTitle,
    required this.trackSubtitle,
    required this.trackCoverUrl,
    required this.queueTrackCountText,
    required this.queueTotalDurationText,
    required this.dontStopTheMusicEnabled,
    required this.queueItems,
    required this.currentQueueIndex,
    required this.playbackCoordinator,
    required this.onToggleDontStopTheMusic,
    this.onOpenArtist,
    this.onOpenAlbum,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Padding(
      padding: const EdgeInsets.fromLTRB(48, 52, 48, 10),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          SizedBox(
            width: 300,
            child: Padding(
              padding: const EdgeInsets.only(top: 58),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.center,
                children: <Widget>[
                  Container(
                    width: 300,
                    height: 300,
                    decoration: BoxDecoration(
                      color: colors.bgTertiary,
                      borderRadius: BorderRadius.circular(6),
                    ),
                    child: ClipRRect(
                      borderRadius: BorderRadius.circular(6),
                      child: trackCoverUrl != null
                          ? MediaImage(
                              source: trackCoverUrl!,
                              fit: BoxFit.cover,
                              fallback: Icon(
                                Icons.music_note_outlined,
                                size: 64,
                                color: colors.textSecondary,
                              ),
                            )
                          : Icon(
                              Icons.music_note_outlined,
                              size: 64,
                              color: colors.textSecondary,
                            ),
                    ),
                  ),
                  const SizedBox(height: 16),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: <Widget>[
                      _TrackActionIcon(icon: Icons.favorite_border_outlined),
                      const SizedBox(width: 22),
                      _TrackActionIcon(icon: Icons.add_outlined),
                    ],
                  ),
                  const SizedBox(height: 16),
                  Text(
                    trackTitle,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    textAlign: TextAlign.center,
                    style: TextStyle(
                      color: colors.textPrimary,
                      fontWeight: FontWeight.w700,
                      fontSize: 16,
                    ),
                  ),
                  const SizedBox(height: 4),
                  Text(
                    trackSubtitle,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    textAlign: TextAlign.center,
                    style: TextStyle(color: colors.textSecondary, fontSize: 14),
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(width: 40),
          Expanded(
            child: Column(
              children: <Widget>[
                Row(
                  children: <Widget>[
                    Text(
                      'Warteschlange',
                      style: TextStyle(
                        color: colors.textSecondary,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Text('•', style: TextStyle(color: colors.textTertiary)),
                    const SizedBox(width: 8),
                    Text(
                      queueTrackCountText,
                      style: TextStyle(color: colors.textTertiary),
                    ),
                    const SizedBox(width: 8),
                    Text('•', style: TextStyle(color: colors.textTertiary)),
                    const SizedBox(width: 8),
                    Text(
                      queueTotalDurationText,
                      style: TextStyle(color: colors.textTertiary),
                    ),
                    const Spacer(),
                    Text(
                      'Automatische Empfehlungen:',
                      style: TextStyle(color: colors.textSecondary),
                    ),
                    const SizedBox(width: 2),
                    Align(
                      alignment: Alignment.center,
                      child: Transform.scale(
                        alignment: Alignment.center,
                        scale: 0.65,
                        child: Switch(
                          value: dontStopTheMusicEnabled,
                          onChanged: (_) {
                            unawaited(onToggleDontStopTheMusic());
                          },
                        ),
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 16),
                Expanded(
                  child: MediaTableSectionDesktop<QueueItem>(
                    title: ' ',
                    items: queueItems,
                    isLoading: false,
                    emptyText: 'Queue ist leer',
                    maxItemsPerPage: 0,
                    enableSelection: true,
                    playbackCoordinator: playbackCoordinator,
                    onArtistTap: onOpenArtist,
                    onAlbumTap: onOpenAlbum,
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

class _TrackActionIcon extends StatelessWidget {
  final IconData icon;

  const _TrackActionIcon({required this.icon});

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    return SizedBox(
      width: 36,
      height: 36,
      child: IconButton(
        onPressed: () {},
        splashRadius: 16,
        visualDensity: VisualDensity.compact,
        icon: Icon(icon, size: 20, color: colors.textSecondary),
        style: IconButton.styleFrom(
          backgroundColor: Colors.transparent,
          foregroundColor: colors.textSecondary,
        ),
      ),
    );
  }
}
