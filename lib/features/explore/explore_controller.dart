import 'dart:async';

import 'package:flutter/foundation.dart';

import '../core/core_bridge.dart';
import '../core/core_models.dart';
import '../core/core_userdata.dart';

class ExploreSimilarArtistSection {
  const ExploreSimilarArtistSection({
    required this.title,
    required this.artists,
  });

  final String title;
  final List<Artist> artists;
}

class ExploreController extends ChangeNotifier {
  ExploreController({required this.coreBridge, this.userDataController});

  final MashinCoreBridgeApi coreBridge;
  final UserData? userDataController;

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
      final List<RecommendationFolder> folders = await coreBridge
          .getRecommendations();
      _similarArtistSections = _similarArtistSectionsFromFolders(folders);
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
    final UserData? controller = userDataController;
    if (controller == null) {
      return items;
    }
    return items
        .map((T item) => controller.withFavoriteStatus(item))
        .toList(growable: false);
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
