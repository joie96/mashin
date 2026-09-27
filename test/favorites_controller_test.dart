import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/features/core/core_bridge.dart';
import 'package:mashin_flutter/features/core/core_models.dart';
import 'package:mashin_flutter/features/core/core_userdata.dart';
import 'package:mashin_flutter/features/favorites/favorites_controller.dart';

void main() {
  test(
    'passes the artist to the radio command without separate playback',
    () async {
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

      expect(coreBridge.playedItems, isEmpty);
      expect(coreBridge.radioSeeds, <MediaItem>[artist]);
    },
  );
}

class _FakeFavoritesCoreBridge extends Fake implements MashinCoreBridgeApi {
  List<MediaItem> playedItems = const <MediaItem>[];
  List<MediaItem> radioSeeds = const <MediaItem>[];

  @override
  Future<void> playMedia(List<MediaItem> items) async {
    playedItems = items;
  }

  @override
  Future<void> startSendspinRadio(List<MediaItem> items) async {
    radioSeeds = items;
  }
}

class _FakeUserDataController extends Fake implements UserData {
  @override
  void addListener(VoidCallback listener) {}

  @override
  void removeListener(VoidCallback listener) {}
}
