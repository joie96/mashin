import 'dart:async';

import 'package:flutter/material.dart';

import '../../theme/app_theme.dart';
import '../../../data/music_assistant/ma_models.dart';
import '../../../features/shared/widgets/media_table_section_desktop.dart';
import '../../../features/shared/widgets/media_table_section_mobile.dart';

class QueueOverlay extends StatefulWidget {
  const QueueOverlay({
    required this.isOpen,
    required this.isDesktop,
    required this.trackTitle,
    required this.trackSubtitle,
    required this.trackCoverUrl,
    required this.queueTrackCountText,
    required this.queueTotalDurationText,
    required this.dontStopTheMusicEnabled,
    required this.queueItems,
    required this.currentQueueIndex,
    required this.onClose,
    required this.onPlayQueueIndex,
    required this.onToggleDontStopTheMusic,
    super.key,
  });

  final bool isOpen;
  final bool isDesktop;
  final String trackTitle;
  final String trackSubtitle;
  final String? trackCoverUrl;
  final String queueTrackCountText;
  final String queueTotalDurationText;
  final bool dontStopTheMusicEnabled;
  final List<QueueItem> queueItems;
  final int? currentQueueIndex;
  final VoidCallback onClose;
  final Future<void> Function(int index) onPlayQueueIndex;
  final Future<void> Function() onToggleDontStopTheMusic;

  @override
  State<QueueOverlay> createState() => _QueueOverlayState();
}

class _QueueOverlayState extends State<QueueOverlay> {
  static const double _mobileDismissThreshold = 120;
  static const double _dragResistance = 0.92;
  double _dragOffset = 0;

  @override
  void didUpdateWidget(covariant QueueOverlay oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!widget.isOpen && _dragOffset != 0) {
      _dragOffset = 0;
    }
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final bool interactable = widget.isOpen || _dragOffset > 0;
    final double targetOpacity = widget.isOpen ? 1 : 0;

    return IgnorePointer(
      ignoring: !interactable,
      child: Stack(
        children: <Widget>[
          AnimatedOpacity(
            duration: const Duration(milliseconds: 220),
            curve: Curves.easeOutCubic,
            opacity: targetOpacity,
            child: GestureDetector(
              onTap: widget.onClose,
              child: Container(color: Colors.black54),
            ),
          ),
          Align(
            alignment: Alignment.bottomCenter,
            child: AnimatedSlide(
              duration: const Duration(milliseconds: 260),
              curve: Curves.easeOutCubic,
              offset: widget.isOpen ? Offset.zero : const Offset(0, 1),
              child: Transform.translate(
                offset: Offset(0, _dragOffset),
                child: GestureDetector(
                  onVerticalDragUpdate: widget.isDesktop
                      ? null
                      : (details) {
                          if (!widget.isOpen) {
                            return;
                          }

                          final double next = _dragOffset + (details.delta.dy * _dragResistance);
                          setState(() {
                            _dragOffset = next < 0 ? 0 : next;
                          });
                        },
                  onVerticalDragEnd: widget.isDesktop
                      ? null
                      : (_) {
                          if (_dragOffset >= _mobileDismissThreshold) {
                            setState(() {
                              _dragOffset = 0;
                            });
                            widget.onClose();
                            return;
                          }

                          setState(() {
                            _dragOffset = 0;
                          });
                        },
                  child: Container(
                    constraints: BoxConstraints(
                      maxWidth: widget.isDesktop ? 1400 : double.infinity,
                      minHeight: widget.isDesktop ? 420 : 380,
                      maxHeight: MediaQuery.of(context).size.height * (widget.isDesktop ? 0.86 : 0.78),
                    ),
                    decoration: BoxDecoration(
                      color: colors.bgSecondary,
                      borderRadius: widget.isDesktop
                          ? const BorderRadius.vertical(top: Radius.circular(18))
                          : const BorderRadius.vertical(top: Radius.circular(20)),
                      border: Border(
                        top: BorderSide(color: colors.border),
                      ),
                    ),
                    child: widget.isDesktop
                        ? _buildDesktopContent(context)
                        : _buildMobileContent(context),
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildDesktopContent(BuildContext context) {
    final colors = context.mashinColors;

    return Padding(
      padding: const EdgeInsets.fromLTRB(36, 34, 36, 18),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          SizedBox(
            width: 300,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.center,
              children: <Widget>[
                Container(
                  width: 260,
                  height: 260,
                  decoration: BoxDecoration(
                    color: colors.bgTertiary,
                    borderRadius: BorderRadius.circular(8),
                  ),
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(8),
                    child: widget.trackCoverUrl != null
                        ? Image.network(
                            widget.trackCoverUrl!,
                            fit: BoxFit.cover,
                            errorBuilder: (_, _, _) {
                              return Icon(Icons.music_note_outlined, size: 64, color: colors.textSecondary);
                            },
                          )
                        : Icon(Icons.music_note_outlined, size: 64, color: colors.textSecondary),
                  ),
                ),
                const SizedBox(height: 16),
                Text(
                  widget.trackTitle,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: colors.textPrimary, fontWeight: FontWeight.w700),
                ),
                const SizedBox(height: 4),
                Text(
                  widget.trackSubtitle,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: colors.textSecondary),
                ),
              ],
            ),
          ),
          const SizedBox(width: 30),
          Expanded(
            child: Column(
              children: <Widget>[
                Row(
                  children: <Widget>[
                    Text(
                      'Warteschlange',
                      style: TextStyle(color: colors.textSecondary, fontWeight: FontWeight.w700),
                    ),
                    const SizedBox(width: 8),
                    Text('•', style: TextStyle(color: colors.textTertiary)),
                    const SizedBox(width: 8),
                    Text(widget.queueTrackCountText, style: TextStyle(color: colors.textTertiary)),
                    const SizedBox(width: 8),
                    Text('•', style: TextStyle(color: colors.textTertiary)),
                    const SizedBox(width: 8),
                    Text(widget.queueTotalDurationText, style: TextStyle(color: colors.textTertiary)),
                    const Spacer(),
                    Text('Automatische Empfehlungen', style: TextStyle(color: colors.textSecondary)),
                    const SizedBox(width: 8),
                    Switch(
                      value: widget.dontStopTheMusicEnabled,
                      onChanged: (_) {
                        unawaited(widget.onToggleDontStopTheMusic());
                      },
                    ),
                    const SizedBox(width: 8),
                    IconButton(onPressed: widget.onClose, icon: const Icon(Icons.close_outlined)),
                  ],
                ),
                const SizedBox(height: 12),
                Expanded(
                  child: MediaTableSectionDesktop<QueueItem>(
                    title: ' ',
                    items: widget.queueItems,
                    isLoading: false,
                    emptyText: 'Queue ist leer',
                    rowMapper: _mapQueueRowData,
                    desktopColumns: _buildQueueDesktopColumns(),
                    maxItemsPerPage: 0,
                    enableSelection: false,
                    onRowDoubleTap: (QueueItem item, int index) {
                      unawaited(widget.onPlayQueueIndex(index));
                    },
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildMobileContent(BuildContext context) {
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
              Text(widget.queueTrackCountText, style: TextStyle(color: colors.textTertiary)),
              const SizedBox(width: 8),
              Text('•', style: TextStyle(color: colors.textTertiary)),
              const SizedBox(width: 8),
              Text(widget.queueTotalDurationText, style: TextStyle(color: colors.textTertiary)),
              const Spacer(),
              IconButton(onPressed: widget.onClose, icon: const Icon(Icons.close_outlined)),
            ],
          ),
          Expanded(
            child: MediaTableSectionMobile<QueueItem>(
              title: ' ',
              items: widget.queueItems,
              isLoading: false,
              emptyText: 'Queue ist leer',
              rowMapper: _mapQueueRowData,
              maxItemsPerPage: 0,
              enableSelection: false,
              onRowTap: (QueueItem item, int index) {
                unawaited(widget.onPlayQueueIndex(index));
              },
            ),
          ),
        ],
      ),
    );
  }
}

MediaTableRowData _mapQueueRowData(QueueItem item, int index) {
  final String provider = item.mediaItem?.provider.trim() ?? '';
  final String title = (item.mediaItem?.name.trim().isNotEmpty == true)
      ? item.mediaItem!.name.trim()
      : item.name.trim().isNotEmpty
          ? item.name.trim()
          : 'Unbekannter Titel';
  final String subtitle = item.mediaItem?.artists
          .map((artist) => artist.name.trim())
          .where((name) => name.isNotEmpty)
          .join(', ') ??
      '';

  return MediaTableRowData(
    title: title,
    subtitle: subtitle.isEmpty ? '-' : subtitle,
    trailing: _formatOverlaySeconds((item.duration ?? 0).toDouble()),
    coverUrl: item.mediaItem?.imageUri,
    fallbackIcon: Icons.music_note_outlined,
    providerLabel: _providerLabelFromId(provider),
    providerIconSvg: ProviderManifestRegistry.iconSvgForProvider(provider),
  );
}

List<DesktopTableColumn<QueueItem>> _buildQueueDesktopColumns() {
  return <DesktopTableColumn<QueueItem>>[
    const DesktopTableColumn<QueueItem>(
      header: '#',
      width: 44,
      textAlign: TextAlign.left,
      textTone: DesktopTextTone.tertiary,
      fontSize: 12,
      valueBuilder: _queueIndexValue,
    ),
    DesktopTableColumn<QueueItem>(
      header: 'Cover',
      width: 62,
      childBuilder: (BuildContext context, QueueItem item, int index) {
        return MediaTableCover(
          coverUrl: item.mediaItem?.imageUri,
          fallbackIcon: Icons.music_note_outlined,
          size: 48,
          radius: 8,
        );
      },
    ),
    const DesktopTableColumn<QueueItem>(
      header: 'Titel',
      flex: 5,
      textTone: DesktopTextTone.primary,
      valueBuilder: _queueTitleValue,
    ),
    const DesktopTableColumn<QueueItem>(
      header: 'Artist',
      flex: 3,
      textTone: DesktopTextTone.secondary,
      valueBuilder: _queueArtistValue,
    ),
    const DesktopTableColumn<QueueItem>(
      header: 'Länge',
      width: 76,
      textAlign: TextAlign.right,
      textTone: DesktopTextTone.tertiary,
      valueBuilder: _queueDurationValue,
    ),
    DesktopTableColumn<QueueItem>(
      header: 'Quelle',
      width: 72,
      textAlign: TextAlign.center,
      childBuilder: (BuildContext context, QueueItem item, int index) {
        final String provider = item.mediaItem?.provider.trim() ?? '';
        final String providerLabel = _providerLabelFromId(provider);
        return Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: <Widget>[
            MediaProviderIcon(
              providerLabel: providerLabel,
              iconSvg: ProviderManifestRegistry.iconSvgForProvider(provider),
              size: 16,
            ),
          ],
        );
      },
    ),
  ];
}

String _queueIndexValue(QueueItem item, int index) => '${index + 1}';

String _queueTitleValue(QueueItem item, int index) {
  if (item.mediaItem?.name.trim().isNotEmpty == true) {
    return item.mediaItem!.name.trim();
  }
  if (item.name.trim().isNotEmpty) {
    return item.name.trim();
  }
  return 'Unbekannter Titel';
}

String _queueArtistValue(QueueItem item, int index) {
  final String artists = item.mediaItem?.artists
          .map((artist) => artist.name.trim())
          .where((name) => name.isNotEmpty)
          .join(', ') ??
      '';
  return artists.isEmpty ? '-' : artists;
}

String _queueDurationValue(QueueItem item, int index) {
  return _formatOverlaySeconds((item.duration ?? 0).toDouble());
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

String _formatOverlaySeconds(double seconds) {
  if (!seconds.isFinite || seconds <= 0) {
    return '0:00';
  }

  final int totalSeconds = seconds.round();
  final int minutes = totalSeconds ~/ 60;
  final int remainingSeconds = totalSeconds % 60;
  return '$minutes:${remainingSeconds.toString().padLeft(2, '0')}';
}
