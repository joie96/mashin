import 'dart:async';

import 'package:flutter/material.dart';

import '../core/core_models.dart';
import 'artist_detail_controller.dart';

class ArtistDetailSection extends StatelessWidget {
  const ArtistDetailSection({
    super.key,
    required this.controller,
    required this.kind,
    required this.title,
    required this.builder,
    this.isSliver = false,
  });

  final ArtistDetailController controller;
  final CatalogQueryKind kind;
  final String title;
  final WidgetBuilder builder;
  final bool isSliver;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: switch (kind) {
      CatalogQueryKind.artistAlbums => controller.albumsListenable,
      CatalogQueryKind.artistTopTracks => controller.topTracksListenable,
      CatalogQueryKind.similarArtists => controller.similarArtistsListenable,
      _ => throw ArgumentError.value(kind, 'kind', 'Not an artist section'),
    },
    builder: (context, _) {
      if (!controller.hasSectionError(kind)) return builder(context);
      final hasObserved = switch (kind) {
        CatalogQueryKind.artistAlbums => controller.hasObservedAlbums,
        CatalogQueryKind.artistTopTracks => controller.hasObservedTopTracks,
        CatalogQueryKind.similarArtists => controller.hasObservedSimilarArtists,
        _ => throw ArgumentError.value(kind, 'kind', 'Not an artist section'),
      };
      final error = Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (!hasObserved)
            Text(title, style: Theme.of(context).textTheme.titleLarge),
          Row(
            children: [
              Expanded(
                child: Text(
                  hasObserved
                      ? 'Aktualisieren fehlgeschlagen.'
                      : 'Laden fehlgeschlagen.',
                ),
              ),
              IconButton(
                tooltip: '$title erneut laden',
                icon: const Icon(Icons.refresh),
                onPressed: () => unawaited(controller.retrySection(kind)),
              ),
            ],
          ),
        ],
      );
      if (isSliver) {
        return SliverMainAxisGroup(
          slivers: [
            SliverToBoxAdapter(child: error),
            if (hasObserved) builder(context),
          ],
        );
      }
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [error, if (hasObserved) builder(context)],
      );
    },
  );
}
