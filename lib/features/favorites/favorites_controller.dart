import 'dart:async';
import 'dart:math';

import 'package:flutter/foundation.dart';

import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../playback/playback_coordinator.dart';
import '../shared/userdata_controller.dart';

/// Exposes the favorited tracks/albums/playlists/artists held by [UserDataController]
/// plus the radio/favorite-toggle actions used by the desktop and mobile favorites pages.
/// favorites come from the already loaded user preferences snapshot instead of a dedicated network call.
class FavoritesController extends ChangeNotifier {
  FavoritesController({
    required this.client,
    required this.playbackCoordinator,
    required this.userDataController,
  }) {
    userDataController.addListener(_onUserDataChanged);
  }

  final MusicAssistantClient client;
  final PlaybackCoordinator playbackCoordinator;
  final UserDataController userDataController;

  final Random _shuffleRandom = Random();
  bool _isDisposed = false;

  @override
  void notifyListeners() {
    if (!_isDisposed) {
      super.notifyListeners();
    }
  }

  void _onUserDataChanged() {
    notifyListeners();
  }

  @override
  void dispose() {
    _isDisposed = true;
    userDataController.removeListener(_onUserDataChanged);
    super.dispose();
  }

  bool get isLoading => userDataController.isLoadingPreferences;

  List<Track> get tracks => userDataController.favoriteTracks;
  List<Album> get albums => userDataController.favoriteAlbums;
  List<Playlist> get playlists => userDataController.favoritePlaylists;
  List<Artist> get artists => userDataController.favoriteArtists;

  List<Playlist> get playlistsForAdding => userDataController.playlists;

  // Radio
  Future<void> startTrackRadio(Track track) async {
    if (track.itemId.trim().isEmpty || track.provider.trim().isEmpty) {
      return;
    }

    try {
      await playbackCoordinator.playMedia(<MediaItem>[track]);
      await playbackCoordinator.playMediaRadioNext(<MediaItem>[track]);
      await _removeDuplicateRadioQueueItem();
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }

  Future<void> startItemRadio(MediaItem item) async {
    if (item.itemId.trim().isEmpty || item.provider.trim().isEmpty) {
      return;
    }

    try {
      await playbackCoordinator.playMedia(<MediaItem>[item]);
      await playbackCoordinator.playMediaRadioNext(<MediaItem>[item]);
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }

  Future<void> startArtistRadio(Artist artist) async {
    if (artist.itemId.trim().isEmpty || artist.provider.trim().isEmpty) {
      return;
    }

    try {
      final List<Track> topTracks = await client.getArtistTopTracks(
        artist.itemId,
        artist.provider,
      );
      if (topTracks.isEmpty) {
        return;
      }

      final Track startTrack = topTracks[_shuffleRandom.nextInt(topTracks.length)];
      await playbackCoordinator.playMedia(<MediaItem>[startTrack]);
      await playbackCoordinator.playMediaRadioNext(<MediaItem>[artist]);
    } catch (_) {
      // Ignore playback failures to keep the UI responsive.
    }
  }

  /// The radio command inserts a duplicate of the seed track directly after
  /// the played one; remove it once it shows up in the queue.
  Future<void> _removeDuplicateRadioQueueItem() async {
    const int duplicateIndex = 1;
    for (int attempt = 0; attempt < 10; attempt++) {
      final List<QueueItem> queueItems = playbackCoordinator.queueItems;
      if (queueItems.length > duplicateIndex) {
        final String queueItemId = queueItems[duplicateIndex].queueItemId;
        if (queueItemId.trim().isNotEmpty) {
          await playbackCoordinator.deleteQueueItem(queueItemId);
          return;
        }
      }
      await Future<void>.delayed(const Duration(milliseconds: 500));
    }
  }

  // Favorites
  bool isItemFavorite(MediaItem item) => item.favorite;

  Future<void> setFavorite(MediaItem item, bool favorite) async {
    await userDataController.setFavorite(<MediaItem>[item], favorite);
  }

  Future<void> addTracksToPlaylist(String playlistId, List<Track> tracksToAdd) async {
    if (playlistId.trim().isEmpty || tracksToAdd.isEmpty) {
      return;
    }
    await userDataController.addPlaylistTracks(playlistId, tracksToAdd);
  }
}
