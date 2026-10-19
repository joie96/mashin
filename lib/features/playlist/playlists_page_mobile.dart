import 'dart:async';

import 'package:flutter/material.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_events.dart';
import '../shared/context_menu/context_menu_controller.dart';
import '../core/core_userdata.dart';
import '../shared/widgets/media_table_section_mobile.dart';
import 'playlist_name_dialog.dart';
import 'playlists_controller.dart';

class PlaylistsPageMobile extends StatefulWidget {
  const PlaylistsPageMobile({
    required this.coreBridge,
    required this.coreEvents,
    required this.userDataController,
    required this.onOpenPlaylist,
    super.key,
  });

  final MashinCoreBridgeApi coreBridge;
  final MashinCoreBridgeEvents coreEvents;
  final UserData userDataController;
  final ValueChanged<Playlist> onOpenPlaylist;

  @override
  State<PlaylistsPageMobile> createState() => _PlaylistsPageMobileState();
}

class _PlaylistsPageMobileState extends State<PlaylistsPageMobile> {
  late final PlaylistsController _controller;

  @override
  void initState() {
    super.initState();
    _controller = PlaylistsController(
      coreBridge: widget.coreBridge,
      userDataController: widget.userDataController,
    )..addListener(_onControllerChanged);
  }

  @override
  void dispose() {
    _controller
      ..removeListener(_onControllerChanged)
      ..dispose();
    super.dispose();
  }

  void _onControllerChanged() {
    if (mounted) {
      setState(() {});
    }
  }

  List<ContextMenuAction> _buildMenuActions(List<Playlist> playlists) {
    final List<Track> tracks = _controller.tracksFor(playlists);
    final bool isSingleEditable =
        playlists.length == 1 && playlists.single.isEditable;
    final bool areAllEditable = playlists.every(
      (Playlist playlist) => playlist.isEditable,
    );
    return <ContextMenuAction>[
      ContextMenuAction(
        label: 'Abspielen',
        icon: Icons.play_arrow,
        isEnabled: tracks.isNotEmpty,
        onSelected: () => _controller.play(tracks),
      ),
      ContextMenuAction(
        label: 'Zufällig abspielen',
        icon: Icons.shuffle_outlined,
        isEnabled: tracks.isNotEmpty,
        onSelected: () => _controller.shuffle(tracks),
      ),
      ContextMenuAction(
        label: 'Radio starten',
        icon: Icons.radio_outlined,
        isEnabled: tracks.isNotEmpty,
        onSelected: () => widget.coreBridge.startRadio(playlists),
      ),
      ContextMenuAction(
        label: 'Als Nächstes spielen',
        icon: Icons.keyboard_return,
        iconQuarterTurns: 2,
        isEnabled: tracks.isNotEmpty,
        onSelected: () => _controller.playNext(tracks),
      ),
      ContextMenuAction(
        label: 'Als Letztes spielen',
        icon: Icons.last_page,
        isEnabled: tracks.isNotEmpty,
        onSelected: () => _controller.playLast(tracks),
      ),
      const ContextMenuAction.separator(),
      ContextMenuAction(
        label: 'Umbenennen',
        icon: Icons.edit_outlined,
        isEnabled: isSingleEditable,
        onSelected: () => _renamePlaylist(playlists.single),
      ),
      ContextMenuAction(
        label: 'Cover aktualisieren',
        icon: Icons.image_outlined,
        isEnabled: isSingleEditable,
        onSelected: () =>
            _controller.refreshPrivatePlaylistCover(playlists.single),
      ),
      ContextMenuAction(
        label: 'Löschen',
        icon: Icons.delete_outline,
        isEnabled: areAllEditable,
        isDestructive: true,
        onSelected: () => _deletePlaylists(playlists),
      ),
    ];
  }

  Future<void> _showSelectionMenu(List<Playlist> playlists) {
    return const AppContextMenuController().showContextMenu(
      context: context,
      actions: _buildMenuActions(playlists),
    );
  }

  Future<void> _createPlaylist() async {
    final String? name = await showPlaylistNameDialog(context: context);
    if (name != null) {
      await _controller.createPlaylist(name);
    }
  }

  Future<void> _renamePlaylist(Playlist playlist) async {
    final String? name = await showPlaylistNameDialog(
      context: context,
      playlist: playlist,
    );
    if (name != null) {
      await _controller.renamePlaylist(playlist, name);
    }
  }

  Future<void> _deletePlaylists(List<Playlist> playlists) async {
    final String playlistName = playlists.length == 1
        ? playlists.single.name
        : '${playlists.length} Wiedergabelisten';
    final bool confirmed = await showDeletePlaylistDialog(
      context: context,
      playlistName: playlistName,
    );
    if (confirmed) {
      await _controller.deletePlaylists(playlists);
    }
  }

  @override
  Widget build(BuildContext context) {
    return CustomScrollView(
      slivers: <Widget>[
        SliverPadding(
          padding: const EdgeInsets.fromLTRB(18, 20, 8, 20),
          sliver: SliverToBoxAdapter(
            child: Row(
              children: <Widget>[
                Expanded(
                  child: Text(
                    'Playlists',
                    style: Theme.of(context).textTheme.headlineSmall?.copyWith(
                      fontWeight: FontWeight.w700,
                    ),
                  ),
                ),
                IconButton(
                  tooltip: 'Wiedergabeliste erstellen',
                  onPressed: _createPlaylist,
                  padding: EdgeInsets.zero,
                  constraints: const BoxConstraints(
                    minWidth: 36,
                    minHeight: 36,
                  ),
                  visualDensity: VisualDensity.compact,
                  iconSize: 20,
                  icon: const Icon(Icons.add_outlined),
                ),
              ],
            ),
          ),
        ),
        SliverPadding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          sliver: MediaTableSectionMobile<Playlist>(
            title: ' ',
            items: _controller.playlists,
            isLoading: _controller.isLoading,
            emptyText: 'Keine Wiedergabelisten vorhanden.',
            maxItemsPerPage: 0,
            coreBridge: widget.coreBridge,
            coreEvents: widget.coreEvents,
            playlistSubtitleBuilder: (Playlist playlist) {
              final List<Track> tracks = _controller.tracksFor(<Playlist>[
                playlist,
              ]);
              final int totalSeconds = tracks.fold<int>(
                0,
                (int sum, Track track) => sum + track.duration,
              );
              return '${tracks.length} Titel • ${_formatPlaylistDuration(totalSeconds)}';
            },
            onRowTap: (Playlist playlist, int _) =>
                widget.onOpenPlaylist(playlist),
            onSelectionMenuTap: (List<Playlist> playlists) =>
                unawaited(_showSelectionMenu(playlists)),
          ),
        ),
        const SliverToBoxAdapter(child: SizedBox(height: 24)),
      ],
    );
  }
}

String _formatPlaylistDuration(int seconds) {
  if (seconds <= 0) {
    return '0m';
  }

  final Duration duration = Duration(seconds: seconds);
  final int hours = duration.inHours;
  final int minutes = duration.inMinutes;
  final int remainingMinutes = minutes % 60;
  if (hours > 0) {
    return '${hours}h ${remainingMinutes}m';
  }
  return '${minutes}m';
}
