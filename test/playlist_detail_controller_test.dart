import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/playlist/playlist_detail_controller.dart';

void main() {
  test(
    'loads and edits an online playlist through core bridge methods',
    () async {
      final coreBridge = _FakePlaylistCoreBridge();
      final controller = PlaylistDetailController(
        coreBridge: coreBridge,
        itemId: 'playlist-1',
        providerInstanceIdOrDomain: 'library',
      );
      addTearDown(controller.dispose);

      await controller.loadPlaylist();
      await controller.addTracksToPlaylist('playlist-1', <MediaItem>[
        _track('track-2', uri: 'ma://track/2'),
      ]);
      await controller.removeTrack(controller.tracks.single);

      expect(controller.playlist?.name, 'Online Playlist');
      expect(controller.tracks, isEmpty);
      expect(coreBridge.operations, <String>[
        'getPlaylist:playlist-1:library',
        'getPlaylistTracks:playlist-1:library:true:false',
        'addPlaylistTracks:playlist-1:ma://track/2',
        'removePlaylistTracks:playlist-1:1',
      ]);
    },
  );
}

class _FakePlaylistCoreBridge extends Fake implements MashinCoreBridgeApi {
  final List<String> operations = <String>[];

  @override
  Future<Playlist> getPlaylist(String itemId, String provider) async {
    operations.add('getPlaylist:$itemId:$provider');
    return Playlist(
      itemId: itemId,
      provider: provider,
      name: 'Online Playlist',
      isEditable: true,
    );
  }

  @override
  Future<List<Track>> getPlaylistTracks(
    String itemId,
    String provider, {
    bool forceRefresh = false,
    bool allowDynamicTracks = false,
  }) async {
    operations.add(
      'getPlaylistTracks:$itemId:$provider:$forceRefresh:$allowDynamicTracks',
    );
    return <Track>[_track('track-1', uri: 'ma://track/1')];
  }

  @override
  Future<void> addPlaylistTracks(String playlistId, List<String> uris) async {
    operations.add('addPlaylistTracks:$playlistId:${uris.join(',')}');
  }

  @override
  Future<void> removePlaylistTracks(
    String playlistId,
    List<int> positionsToRemove,
  ) async {
    operations.add(
      'removePlaylistTracks:$playlistId:${positionsToRemove.join(',')}',
    );
  }
}

Track _track(String itemId, {required String uri}) => Track(
  itemId: itemId,
  provider: 'library',
  name: itemId,
  duration: 180,
  uri: uri,
  artists: const <ArtistReference>[],
);
