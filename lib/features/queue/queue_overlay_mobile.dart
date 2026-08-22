import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/widgets/media_table_section_mobile.dart';

class QueueOverlayMobile extends StatelessWidget {
  final String queueTrackCountText;
  final String queueTotalDurationText;
  final List<QueueItem> queueItems;
  final int? currentQueueIndex;
  final PlaybackCoordinator playbackCoordinator;
  final VoidCallback onClose;

  const QueueOverlayMobile({
    required this.queueTrackCountText,
    required this.queueTotalDurationText,
    required this.queueItems,
    required this.currentQueueIndex,
    required this.playbackCoordinator,
    required this.onClose,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;

    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 10, 16, 10),
      child: Column(
        children: <Widget>[
          Container(
            width: 52,
            height: 4,
            decoration: BoxDecoration(
              color: colors.bgTertiary,
              borderRadius: BorderRadius.circular(2),
            ),
          ),
          const SizedBox(height: 14),
          Row(
            children: <Widget>[
              Text(
                'Warteschlange',
                style: TextStyle(color: colors.textSecondary, fontWeight: FontWeight.w700),
              ),
              const SizedBox(width: 8),
              Text('•', style: TextStyle(color: colors.textTertiary)),
              const SizedBox(width: 8),
              Text(queueTrackCountText, style: TextStyle(color: colors.textTertiary)),
              const SizedBox(width: 8),
              Text('•', style: TextStyle(color: colors.textTertiary)),
              const SizedBox(width: 8),
              Text(queueTotalDurationText, style: TextStyle(color: colors.textTertiary)),
              const Spacer(),
              IconButton(onPressed: onClose, icon: const Icon(Icons.close_outlined)),
            ],
          ),
          Expanded(
            child: MediaTableSectionMobile<QueueItem>(
              title: ' ',
              items: queueItems,
              isLoading: false,
              emptyText: 'Queue ist leer',
              maxItemsPerPage: 0,
              enableSelection: true,
              playbackCoordinator: playbackCoordinator,
            ),
          ),
        ],
      ),
    );
  }
}