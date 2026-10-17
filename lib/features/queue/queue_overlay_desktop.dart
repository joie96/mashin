import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/app_theme.dart';
import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_table_section_desktop.dart';
import '../shared/widgets/media_image.dart';

class QueueOverlayDesktop extends StatefulWidget {
  final String trackTitle;
  final String trackSubtitle;
  final String? trackCoverUrl;
  final String queueTrackCountText;
  final String queueTotalDurationText;
  final bool dontStopTheMusicEnabled;
  final List<QueueItem> queueItems;
  final int? currentQueueIndex;
  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData? userDataController;
  final VoidCallback onClose;
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
    required this.coreBridge,
    required this.coreEvents,
    this.userDataController,
    required this.onClose,
    required this.onToggleDontStopTheMusic,
    this.onOpenArtist,
    this.onOpenAlbum,
    super.key,
  });

  @override
  State<QueueOverlayDesktop> createState() => _QueueOverlayDesktopState();
}

class _QueueOverlayDesktopState extends State<QueueOverlayDesktop> {
  static const double _queueRowExtent = 64;
  List<QueueItem> _selectedItems = const <QueueItem>[];
  final ScrollController _queueScrollController = ScrollController();
  int? _lastCurrentQueueIndex;

  String get trackTitle => widget.trackTitle;
  String get trackSubtitle => widget.trackSubtitle;
  String? get trackCoverUrl => widget.trackCoverUrl;
  String get queueTrackCountText => widget.queueTrackCountText;
  String get queueTotalDurationText => widget.queueTotalDurationText;
  bool get dontStopTheMusicEnabled => widget.dontStopTheMusicEnabled;
  List<QueueItem> get queueItems => widget.queueItems;
  int? get currentQueueIndex => widget.currentQueueIndex;
  MashinCoreBridgeEvents get coreEvents => widget.coreEvents;
  UserData? get userDataController => widget.userDataController;
  VoidCallback get onClose => widget.onClose;
  Future<void> Function() get onToggleDontStopTheMusic =>
      widget.onToggleDontStopTheMusic;
  ValueChanged<Artist>? get onOpenArtist => widget.onOpenArtist;
  ValueChanged<Album>? get onOpenAlbum => widget.onOpenAlbum;

  @override
  void initState() {
    super.initState();
    _scrollToCurrentItem();
  }

  @override
  void didUpdateWidget(covariant QueueOverlayDesktop oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.currentQueueIndex != widget.currentQueueIndex ||
        oldWidget.queueItems.length != widget.queueItems.length) {
      _scrollToCurrentItem();
    }
  }

  @override
  void dispose() {
    _queueScrollController.dispose();
    super.dispose();
  }

  void _scrollToCurrentItem() {
    final int? index = widget.currentQueueIndex;
    if (index == null ||
        index < 0 ||
        index >= widget.queueItems.length ||
        index == _lastCurrentQueueIndex) {
      return;
    }
    _lastCurrentQueueIndex = index;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || !_queueScrollController.hasClients) {
        return;
      }
      final double target = (index * _queueRowExtent - _queueRowExtent)
          .clamp(0, _queueScrollController.position.maxScrollExtent)
          .toDouble();
      _queueScrollController.animateTo(
        target,
        duration: const Duration(milliseconds: 220),
        curve: Curves.easeOutCubic,
      );
    });
  }

  Track? get _currentTrack {
    final int? index = currentQueueIndex;
    if (index == null || index < 0 || index >= queueItems.length) {
      return null;
    }
    final MediaItem mediaItem = queueItems[index].mediaItem;
    return mediaItem is Track ? mediaItem : null;
  }

  Future<void> _showAddToPlaylistMenu(BuildContext context, Track track) {
    final UserData? controller = userDataController;
    if (controller == null) {
      return Future<void>.value();
    }
    return const AppContextMenuController().showContextMenu(
      context: context,
      actions: controller.playlists
          .map(
            (Playlist playlist) => ContextMenuAction(
              label: playlist.name,
              icon: Icons.playlist_play_outlined,
              onSelected: () =>
                  controller.addPlaylistTracks(playlist.itemId, <Track>[track]),
            ),
          )
          .toList(growable: false),
    );
  }

  List<QueueItem> _targetItems(QueueItem item) =>
      _selectedItems.isEmpty ? <QueueItem>[item] : _selectedItems;

  Future<void> _moveTargetItemsNext(List<QueueItem> targets) async {
    final Set<String> targetIds = targets
        .map((QueueItem item) => item.queueItemId)
        .where((String id) => id.isNotEmpty)
        .toSet();
    final List<String> orderedTargetIds = queueItems
        .map((QueueItem item) => item.queueItemId)
        .where(targetIds.contains)
        .toList(growable: false);
    for (final String itemId in orderedTargetIds.reversed) {
      await widget.coreBridge.moveQueueItem(itemId, 0);
    }
  }

  Future<void> _moveTargetItemsLast(List<QueueItem> targets) async {
    final Set<String> targetIds = targets
        .map((QueueItem item) => item.queueItemId)
        .where((String id) => id.isNotEmpty)
        .toSet();
    final List<String> orderedTargetIds = queueItems
        .map((QueueItem item) => item.queueItemId)
        .where(targetIds.contains)
        .toList(growable: false);
    for (final String itemId in orderedTargetIds) {
      await widget.coreBridge.moveQueueItemToEnd(itemId);
    }
  }

  List<ContextMenuAction> _buildMenuActions(QueueItem item) {
    final List<QueueItem> targets = _targetItems(item);
    final List<Track> tracks = targets
        .map((QueueItem queueItem) => queueItem.mediaItem)
        .whereType<Track>()
        .toList(growable: false);
    final Track? track = tracks.length == 1 ? tracks.single : null;
    final UserData? controller = userDataController;
    final bool areAllFavorites =
        tracks.isNotEmpty &&
        tracks.every(
          (Track queueTrack) =>
              controller?.isFavorite(queueTrack) ?? queueTrack.favorite,
        );

    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        icon: Icons.play_arrow,
        onSelected: () =>
            widget.coreBridge.selectQueueItem(targets.first.queueItemId),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        onSelected: () => _moveTargetItemsNext(targets),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        onSelected: () => _moveTargetItemsLast(targets),
      ),
      ContextMenuAction(
        label: 'Aus Queue entfernen',
        icon: Icons.remove_circle_outline,
        onSelected: () async {
          for (final String itemId
              in targets
                  .map((QueueItem target) => target.queueItemId)
                  .where((String id) => id.isNotEmpty)) {
            await widget.coreBridge.deleteQueueItem(itemId);
          }
        },
      ),
      if (track != null &&
          (onOpenArtist != null || onOpenAlbum != null)) ...<ContextMenuAction>[
        const ContextMenuAction.separator(),
        if (track.artists.isNotEmpty && onOpenArtist != null)
          ContextMenuAction(
            label: 'Künstler:in öffnen',
            icon: Icons.person_outline,
            onSelected: () => onOpenArtist!(track.artists.first),
          ),
        if (track.album != null && onOpenAlbum != null)
          ContextMenuAction(
            label: 'Album öffnen',
            icon: Icons.album_outlined,
            onSelected: () => onOpenAlbum!(track.album!),
          ),
      ],
      if (tracks.isNotEmpty &&
          controller != null &&
          controller.playlists.isNotEmpty) ...<ContextMenuAction>[
        const ContextMenuAction.separator(),
        ContextMenuAction.submenu(
          label: 'Zu Wiedergabeliste hinzufügen',
          icon: Icons.playlist_add_outlined,
          children: controller.playlists
              .map(
                (Playlist playlist) => ContextMenuAction(
                  label: playlist.name,
                  icon: Icons.playlist_play_outlined,
                  onSelected: () =>
                      controller.addPlaylistTracks(playlist.itemId, tracks),
                ),
              )
              .toList(growable: false),
        ),
      ],
      if (tracks.isNotEmpty && controller != null) ...<ContextMenuAction>[
        const ContextMenuAction.separator(),
        ContextMenuAction(
          label: areAllFavorites
              ? 'Aus Favoriten entfernen'
              : 'Zu Favoriten hinzufügen',
          icon: areAllFavorites
              ? Icons.favorite_outlined
              : Icons.favorite_border_outlined,
          onSelected: () => controller.setFavorite(tracks, !areAllFavorites),
        ),
      ],
    ];
  }

  Future<void> _showMenu(
    BuildContext context,
    QueueItem item, {
    BuildContext? anchorContext,
    Offset? globalPosition,
  }) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      anchorContext: anchorContext,
      globalPosition: globalPosition,
      actions: _buildMenuActions(item),
    );
  }

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
                      Builder(
                        builder: (BuildContext context) {
                          final Track? track = _currentTrack;
                          final bool isFavorite =
                              track != null &&
                              (userDataController?.isFavorite(track) ??
                                  track.favorite);
                          return _TrackActionIcon(
                            icon: isFavorite
                                ? Icons.favorite_outlined
                                : Icons.favorite_border_outlined,
                            onPressed:
                                track == null || userDataController == null
                                ? null
                                : () => userDataController!.setFavorite(<Track>[
                                    track,
                                  ], !isFavorite),
                          );
                        },
                      ),
                      const SizedBox(width: 22),
                      Builder(
                        builder: (BuildContext context) {
                          final Track? track = _currentTrack;
                          final bool hasPlaylists =
                              userDataController?.playlists.isNotEmpty ?? false;
                          return _TrackActionIcon(
                            icon: Icons.add_outlined,
                            onPressed: track == null || !hasPlaylists
                                ? null
                                : () => _showAddToPlaylistMenu(context, track),
                          );
                        },
                      ),
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
                    const SizedBox(width: 8),
                    _TrackActionIcon(
                      icon: Icons.close_outlined,
                      onPressed: onClose,
                    ),
                  ],
                ),
                const SizedBox(height: 16),
                Expanded(
                  child: CustomScrollView(
                    controller: _queueScrollController,
                    slivers: <Widget>[
                      MediaTableSectionDesktop<QueueItem>(
                        title: ' ',
                        items: queueItems,
                        isLoading: false,
                        emptyText: 'Queue ist leer',
                        maxItemsPerPage: 0,
                        enableSelection: true,
                        coreBridge: widget.coreBridge,
                        coreEvents: coreEvents,
                        onSelectionChanged: (List<QueueItem> items) {
                          setState(() {
                            _selectedItems = items;
                          });
                        },
                        onRowMoreAnchorTap:
                            (
                              QueueItem item,
                              int _,
                              BuildContext anchorContext,
                            ) => _showMenu(
                              context,
                              item,
                              anchorContext: anchorContext,
                            ),
                        onRowContextMenu:
                            (QueueItem item, int _, Offset globalPosition) =>
                                _showMenu(
                                  context,
                                  item,
                                  globalPosition: globalPosition,
                                ),
                        onArtistTap: onOpenArtist,
                        onAlbumTap: onOpenAlbum,
                      ),
                    ],
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
  final VoidCallback? onPressed;
  final Color? color;

  const _TrackActionIcon({required this.icon, this.onPressed}) : color = null;

  @override
  Widget build(BuildContext context) {
    final colors = context.mashinColors;
    final Color effectiveColor = color ?? colors.textSecondary;
    return SizedBox(
      width: 36,
      height: 36,
      child: IconButton(
        onPressed: onPressed,
        splashRadius: 16,
        visualDensity: VisualDensity.compact,
        icon: Icon(icon, size: 20, color: effectiveColor),
        style: IconButton.styleFrom(
          backgroundColor: Colors.transparent,
          foregroundColor: effectiveColor,
        ),
      ),
    );
  }
}
