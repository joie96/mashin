import 'dart:async';

import 'package:flutter/foundation.dart';

import '../../data/music_assistant/ma_client.dart';
import '../../data/music_assistant/ma_models.dart';
import '../shared/userdata_controller.dart';

class ExploreSimilarArtistSection {
  const ExploreSimilarArtistSection({
    required this.title,
    required this.artists,
  });

  final String title;
  final List<Artist> artists;
}

class ExploreController extends ChangeNotifier {
  ExploreController({required this.client, this.userDataController});

  final MusicAssistantClient client;
  final UserDataController? userDataController;

  bool _isDisposed = false;
  bool _isLoadingSimilarArtists = true;
  List<ExploreSimilarArtistSection> _similarArtistSections =
      const <ExploreSimilarArtistSection>[];

  bool get isLoadingSimilarArtists => _isLoadingSimilarArtists;
  List<ExploreSimilarArtistSection> get similarArtistSections =>
      _similarArtistSections;

  @override
  void notifyListeners() {
    if (!_isDisposed) {
      super.notifyListeners();
    }
  }

  Future<void> load() async {
    _isLoadingSimilarArtists = true;
    notifyListeners();

    try {
      await client.getProviderManifests();
      final List<RecommendationFolder> folders = await client
          .getRecommendations();
      final List<RecommendationFolder> listenBrainzFolders = folders
          .where(_isListenBrainzFolder)
          .toList(growable: false);
      _similarArtistSections = _similarArtistSectionsFromFolders(
        listenBrainzFolders,
      );
    } catch (_) {
      _similarArtistSections = const <ExploreSimilarArtistSection>[];
    } finally {
      _isLoadingSimilarArtists = false;
      notifyListeners();
    }
  }

  void refreshFavoriteStatuses() {
    _similarArtistSections = _similarArtistSections
        .map(
          (ExploreSimilarArtistSection section) => ExploreSimilarArtistSection(
            title: section.title,
            artists: _withFavoriteStatuses(section.artists),
          ),
        )
        .toList(growable: false);
    notifyListeners();
  }

  bool isItemFavorite(MediaItem item) => item.favorite;

  Future<void> setFavorite(MediaItem item, bool favorite) async {
    await userDataController?.setFavorite(<MediaItem>[item], favorite);
  }

  List<T> _withFavoriteStatuses<T extends MediaItem>(List<T> items) {
    final UserDataController? controller = userDataController;
    if (controller == null) {
      return items;
    }
    return items
        .map((T item) => controller.withFavoriteStatus(item))
        .toList(growable: false);
  }

  bool _isListenBrainzFolder(RecommendationFolder folder) {
    return folder.provider.startsWith('listenbrainz_recommendations--') &&
        folder.itemId.trim().isNotEmpty;
  }

  bool _isValidPlayable(MediaItem item) {
    return item.itemId.trim().isNotEmpty && item.provider.trim().isNotEmpty;
  }

  List<ExploreSimilarArtistSection> _similarArtistSectionsFromFolders(
    List<RecommendationFolder> folders,
  ) {
    return folders
        .where(
          (RecommendationFolder folder) =>
              folder.itemId.toLowerCase().startsWith('similar_artists_'),
        )
        .map(
          (RecommendationFolder folder) => ExploreSimilarArtistSection(
            title: _normalizeSimilarArtistSectionTitle(folder.title),
            artists: _withFavoriteStatuses(
              folder.items
                  .whereType<Artist>()
                  .where(_isValidPlayable)
                  .toList(growable: false),
            ),
          ),
        )
        .where(
          (ExploreSimilarArtistSection section) => section.artists.isNotEmpty,
        )
        .take(5)
        .toList(growable: false);
  }

  String _normalizeSimilarArtistSectionTitle(String title) {
    return title.replaceFirst(
      RegExp(r'^Similar Artists for\s*', caseSensitive: false),
      'Ähnlich zu ',
    );
  }

  @override
  void dispose() {
    _isDisposed = true;
    super.dispose();
  }
}
