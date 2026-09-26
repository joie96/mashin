import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/core/core_userdata.dart';
import 'package:mashin_flutter/features/favorites/favorites_controller.dart';

void main() {
  test('starts artist radio with top tracks from the core bridge', () async {
    final coreBridge = _FakeFavoritesCoreBridge();
    final controller = FavoritesController(
      coreBridge: coreBridge,
      userDataController: _FakeUserDataController(),
    );
    addTearDown(controller.dispose);

    final artist = Artist(
      itemId: 'artist-1',
      provider: 'library',
      name: 'Artist',
    );
    await controller.startArtistRadio(artist);

    expect(coreBridge.queries, <String>['artist-1:library']);
    expect(coreBridge.playedItems, <MediaItem>[coreBridge.topTrack]);
    expect(coreBridge.radioSeeds, <MediaItem>[artist]);
  });
}

class _FakeFavoritesCoreBridge extends Fake implements MashinCoreBridgeApi {
  final List<String> queries = <String>[];
  List<MediaItem> playedItems = const <MediaItem>[];
  List<MediaItem> radioSeeds = const <MediaItem>[];
  final Track topTrack = Track(
    itemId: 'track-1',
    provider: 'library',
    name: 'Top Track',
    duration: 180,
    artists: const <Artist>[],
  );

  @override
  Future<List<Track>> getArtistTopTracks(String itemId, String provider) async {
    queries.add('$itemId:$provider');
    return <Track>[topTrack];
  }

  @override
  Future<void> playMedia(List<MediaItem> items) async {
    playedItems = items;
  }

  @override
  Future<void> playSendspinRadioNext(List<MediaItem> items) async {
    radioSeeds = items;
  }
}

class _FakeUserDataController extends Fake implements UserData {
  @override
  void addListener(VoidCallback listener) {}

  @override
  void removeListener(VoidCallback listener) {}
}
