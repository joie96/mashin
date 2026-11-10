import 'dart:async';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:mashin/app/theme/app_theme.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/core/core_query_scope.dart';
import 'package:mashin/features/shared/widgets/media_image.dart';
import 'package:mashin/features/shared/widgets/media_table_section_desktop.dart';

void main() {
  Future<Uint8List> bitmap(Color color) async {
    final recorder = ui.PictureRecorder();
    Canvas(
      recorder,
    ).drawRect(const Rect.fromLTWH(0, 0, 2, 2), Paint()..color = color);
    final picture = recorder.endRecording();
    final image = await picture.toImage(2, 2);
    final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
    image.dispose();
    picture.dispose();
    return bytes!.buffer.asUint8List();
  }

  tearDown(() {
    MediaImage.configureArtworkLoader(null);
    MediaImage.configureImageLoader(null);
    ProviderManifestRegistry.clear();
    MediaItemImage.configureProxyBaseUrl(null);
  });

  testWidgets('unsupported server paths render a placeholder', (tester) async {
    MediaItemImage.configureProxyBaseUrl('https://music.example');
    for (final source in [
      '/server/cover.jpg',
      'https://cdn.example/cover.jpg',
    ]) {
      await tester.pumpWidget(
        MaterialApp(
          home: MediaImage(
            source: source,
            fit: BoxFit.cover,
            fallback: const Text('fallback'),
          ),
        ),
      );
      expect(find.text('fallback'), findsOneWidget);
      expect(find.byType(Image), findsNothing);
    }
  });

  testWidgets('persisted inline references retain their native model slot', (
    tester,
  ) async {
    final bytes = (await tester.runAsync(() => bitmap(Colors.red)))!;
    final requests = <ImageRequest>[];
    MediaImage.configureImageLoader((request, refresh) async {
      requests.add(request);
      expect(refresh, isFalse);
      return ImageReadSnapshot(bytes: bytes);
    });
    final image = MediaItemImage.fromJson({
      'type': 'thumb',
      'provider': 'builtin',
      'cached_inline': {'owner_uri': 'library://playlist/42', 'position': 1},
    });
    expect(image.path, isEmpty);
    expect(image.proxyId, isNull);
    final source = image.getProxyPath(
      size: 160,
      ownerUri: 'library://playlist/other',
    );
    expect(source, isNotNull);
    await tester.pumpWidget(
      MaterialApp(
        home: MediaImage(
          source: source!,
          fit: BoxFit.cover,
          fallback: const Text('fallback'),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(
      (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
      bytes,
    );
    expect(requests, hasLength(1));
    expect(
      requests.single.image.cachedInline!.ownerUri,
      'library://playlist/42',
    );
    expect(requests.single.image.cachedInline!.position, 1);
    expect((requests.single.toJson()['image'] as Map)['cached_inline'], {
      'owner_uri': 'library://playlist/42',
      'position': 1,
    });
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets(
    'image cache clearing reloads the same source and rejects late refresh',
    (tester) async {
      final original = (await tester.runAsync(() => bitmap(Colors.red)))!;
      final replacement = (await tester.runAsync(() => bitmap(Colors.green)))!;
      final pending = Completer<ImageReadSnapshot>();
      final invalidations = StreamController<void>.broadcast(sync: true);
      final requests = <bool>[];
      MediaImage.configureImageLoader((request, refresh) async {
        requests.add(refresh);
        if (refresh) return pending.future;
        return requests.length == 1
            ? ImageReadSnapshot(bytes: original, hasRefresh: true)
            : ImageReadSnapshot(bytes: replacement);
      }, invalidations: invalidations.stream);
      final source = const MediaItemImage(
        type: 'thumb',
        path: '',
        provider: 'builtin',
        remotelyAccessible: false,
        proxyId:
            'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      ).getProxyPath()!;
      await tester.pumpWidget(
        MaterialApp(
          home: MediaImage(
            source: source,
            fit: BoxFit.cover,
            fallback: const Text('fallback'),
          ),
        ),
      );
      await tester.pump();
      expect(requests, [false, true]);
      invalidations.add(null);
      await tester.pumpAndSettle();
      expect(requests, [false, true, false]);
      expect(
        (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
        replacement,
      );
      pending.complete(ImageReadSnapshot(bytes: original));
      await tester.pumpAndSettle();
      expect(
        (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
        replacement,
      );
      await tester.pumpWidget(const SizedBox.shrink());
      await invalidations.close();
    },
  );

  testWidgets(
    'proxy source notifications reload only matching visible images',
    (tester) async {
      final original = (await tester.runAsync(() => bitmap(Colors.red)))!;
      final replacement = (await tester.runAsync(() => bitmap(Colors.green)))!;
      final changes = StreamController<String>.broadcast(sync: true);
      final pending = Completer<ImageReadSnapshot>();
      final reads = <String>[];
      MediaImage.configureImageLoader((request, refresh) async {
        final proxyId = request.image.proxyId!;
        reads.add(proxyId);
        if (reads.length > 2) return pending.future;
        return ImageReadSnapshot(bytes: original);
      }, sourceChanges: changes.stream);
      String source(String proxyId) => MediaItemImage(
        type: 'thumb',
        path: '',
        provider: 'builtin',
        remotelyAccessible: false,
        proxyId: proxyId,
      ).getProxyPath()!;
      final first = 'a' * 64;
      final other = 'b' * 64;
      await tester.pumpWidget(
        MaterialApp(
          home: Row(
            children: [
              MediaImage(
                source: source(first),
                fit: BoxFit.cover,
                fallback: const Text('first'),
              ),
              MediaImage(
                source: source(other),
                fit: BoxFit.cover,
                fallback: const Text('other'),
              ),
            ],
          ),
        ),
      );
      await tester.pumpAndSettle();
      changes.add(first);
      await tester.pump();
      expect(reads, [first, other, first]);
      expect(
        (tester.widgetList<Image>(find.byType(Image)).first.image
                as MemoryImage)
            .bytes,
        original,
      );
      pending.complete(ImageReadSnapshot(bytes: replacement));
      await tester.pumpAndSettle();
      final displayed = tester.widgetList<Image>(find.byType(Image)).toList();
      expect((displayed.first.image as MemoryImage).bytes, replacement);
      expect((displayed.last.image as MemoryImage).bytes, original);
      await tester.pumpWidget(const SizedBox.shrink());
      await changes.close();
    },
  );

  testWidgets('same-slot content changes reload visible native images', (
    tester,
  ) async {
    final original = (await tester.runAsync(() => bitmap(Colors.red)))!;
    final replacement = (await tester.runAsync(() => bitmap(Colors.green)))!;
    final pending = Completer<ImageReadSnapshot>();
    final hashes = <String?>[];
    MediaImage.configureImageLoader((request, refresh) async {
      expect(refresh, isFalse);
      final hash = request.image.cachedInline!.contentHash;
      hashes.add(hash);
      if (hash == 'failed-content') {
        return const ImageReadSnapshot(cacheError: 'synthetic read failure');
      }
      return hash == 'new-content'
          ? pending.future
          : ImageReadSnapshot(bytes: original);
    });
    String source(String hash) => MediaItemImage.fromJson({
      'type': 'thumb',
      'provider': 'builtin',
      'proxy_id': 'a' * 64,
      'cached_inline': {
        'owner_uri': 'library://playlist/42',
        'position': 0,
        'content_hash': hash,
      },
    }).getProxyPath(size: 160, ownerUri: 'library://playlist/42')!;
    Widget image(String hash) => MaterialApp(
      home: MediaImage(
        source: source(hash),
        fit: BoxFit.cover,
        fallback: const Text('fallback'),
      ),
    );
    await tester.pumpWidget(image('old-content'));
    await tester.pumpAndSettle();
    await tester.pumpWidget(image('old-content'));
    await tester.pumpAndSettle();
    expect(hashes, ['old-content']);
    await tester.pumpWidget(image('new-content'));
    await tester.pump();
    expect(hashes, ['old-content', 'new-content']);
    expect(
      (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
      original,
    );
    pending.complete(ImageReadSnapshot(bytes: replacement));
    await tester.pumpAndSettle();
    expect(
      (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
      replacement,
    );
    await tester.pumpWidget(image('failed-content'));
    await tester.pumpAndSettle();
    expect(hashes, ['old-content', 'new-content', 'failed-content']);
    expect(
      (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
      replacement,
    );
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('local native bytes precede refresh and retain their variant', (
    tester,
  ) async {
    MediaItemImage.configureProxyBaseUrl('https://music.example');
    final bytes = (await tester.runAsync(() => bitmap(Colors.red)))!;
    final updated = (await tester.runAsync(() => bitmap(Colors.green)))!;
    final pending = Completer<ImageReadSnapshot>();
    final requests = <ImageRequest>[];
    MediaImage.configureImageLoader((request, refresh) async {
      requests.add(request);
      return refresh
          ? pending.future
          : ImageReadSnapshot(bytes: bytes, hasRefresh: true);
    });
    final source = const MediaItemImage(
      type: 'thumb',
      path: 'server/path',
      provider: 'builtin',
      remotelyAccessible: false,
      proxyId:
          'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    ).getProxyPath(size: 160, ownerUri: 'library://album/7')!;
    await tester.pumpWidget(
      MaterialApp(
        home: MediaImage(
          source: source,
          fit: BoxFit.cover,
          fallback: const Text('fallback'),
        ),
      ),
    );
    await tester.pump();
    expect(
      (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
      bytes,
    );
    expect(requests, hasLength(2));
    expect(requests.first.size, 160);
    expect(requests.first.ownerUri, 'library://album/7');
    pending.complete(ImageReadSnapshot(bytes: updated));
    await tester.pumpAndSettle();
    expect(
      (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
      updated,
    );
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('inline SVG and data URI icons remain renderable', (
    tester,
  ) async {
    const svg =
        '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="red"/></svg>';
    for (final source in [
      svg,
      Uri.dataFromString(svg, mimeType: 'image/svg+xml').toString(),
    ]) {
      await tester.pumpWidget(
        MaterialApp(
          home: MediaImage(
            source: source,
            fit: BoxFit.contain,
            fallback: const Text('fallback'),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.byType(SvgPicture), findsOneWidget);
      expect(find.text('fallback'), findsNothing);
      expect(tester.takeException(), isNull);
    }
  });

  Artist artist() => Artist.fromJson({
    'item_id': 'one',
    'provider': 'library',
    'name': 'Artist',
    'provider_mappings': [
      {
        'provider_domain': 'source',
        'provider_instance': 'source--one',
        'item_id': 'other',
      },
    ],
  });

  testWidgets('only a rendered image requests missing artwork', (tester) async {
    MediaItemImage.configureProxyBaseUrl('https://music.example');
    final pending = Completer<MediaItemImage?>();
    final bytes = (await tester.runAsync(() => bitmap(Colors.red)))!;
    MediaImage.configureImageLoader(
      (_, _) async => ImageReadSnapshot(bytes: bytes),
    );
    var requests = 0;
    MediaImage.configureArtworkLoader((_) {
      requests++;
      return pending.future;
    });
    final source = artist().imageUriForSize(256)!;
    expect(requests, 0);
    await tester.pumpWidget(
      MaterialApp(
        home: MediaImage(
          source: source,
          fit: BoxFit.cover,
          fallback: const Text('fallback'),
        ),
      ),
    );
    expect(requests, 1);
    expect(find.text('fallback'), findsOneWidget);
    pending.complete(
      const MediaItemImage(
        type: 'thumb',
        path: 'https://example.test/cover.png',
        provider: 'source',
        remotelyAccessible: true,
        proxyId: 'observed-cover',
      ),
    );
    await tester.pump();
    final image = tester.widget<Image>(find.byType(Image));
    expect((image.image as MemoryImage).bytes, bytes);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('cached mapping artwork starts bytes before metadata refresh', (
    tester,
  ) async {
    final bytes = (await tester.runAsync(() => bitmap(Colors.red)))!;
    final metadata = Completer<MediaItemImage?>();
    final updates = StreamController<CatalogUpdate>.broadcast(sync: true);
    final requests = <ImageRequest>[];
    MediaImage.configureArtworkLoader(
      (_) => metadata.future,
      catalogUpdates: updates.stream,
    );
    MediaImage.configureImageLoader((request, _) async {
      requests.add(request);
      return ImageReadSnapshot(bytes: bytes);
    });
    await tester.pumpWidget(
      MaterialApp(
        home: MediaImage(
          source: artist().imageUriForSize(256)!,
          fit: BoxFit.cover,
          fallback: const Text('fallback'),
        ),
      ),
    );
    final mapped = Artist.fromJson({
      'item_id': 'other',
      'provider': 'source--one',
      'name': 'Mapped',
      'metadata': {
        'images': [
          {
            'type': 'thumb',
            'provider': 'source',
            'path': 'cached',
            'proxy_id': 'a' * 64,
          },
        ],
      },
    });
    updates.add(
      CatalogUpdate(
        kind: CatalogQueryKind.providerItem,
        itemId: 'other',
        provider: 'unrelated',
        mediaType: MediaType.artist,
        item: mapped,
      ),
    );
    await tester.pump();
    expect(requests, isEmpty);
    updates.add(
      CatalogUpdate(
        kind: CatalogQueryKind.providerItem,
        itemId: 'other',
        provider: 'source--one',
        mediaType: MediaType.artist,
        item: mapped,
      ),
    );
    await tester.pump();
    expect(metadata.isCompleted, isFalse);
    expect(requests, hasLength(1));
    expect(
      (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
      bytes,
    );
    metadata.completeError(StateError('offline refresh'));
    await tester.pump();
    expect(
      (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
      bytes,
    );
    await tester.pumpWidget(const SizedBox.shrink());
    await updates.close();
  });

  testWidgets('disposed image rejects late artwork', (tester) async {
    final pending = Completer<MediaItemImage?>();
    MediaImage.configureArtworkLoader((_) => pending.future);
    final source = artist().imageUriForSize(256)!;
    await tester.pumpWidget(
      MaterialApp(
        home: MediaImage(
          source: source,
          fit: BoxFit.cover,
          fallback: const SizedBox.shrink(),
        ),
      ),
    );
    await tester.pumpWidget(const SizedBox.shrink());
    pending.complete(
      const MediaItemImage(
        type: 'thumb',
        path: 'https://example.test/late.png',
        provider: 'source',
        remotelyAccessible: true,
      ),
    );
    await tester.pump();
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'mapping revisions cancel old bytes and confirmed emptiness removes artwork',
    (tester) async {
      final red = (await tester.runAsync(() => bitmap(Colors.red)))!;
      final green = (await tester.runAsync(() => bitmap(Colors.green)))!;
      final metadata = Completer<MediaItemImage?>();
      final updates = StreamController<CatalogUpdate>.broadcast(sync: true);
      final changes = StreamController<CatalogReadChange>.broadcast(sync: true);
      MediaImage.configureArtworkLoader(
        (_) => metadata.future,
        catalogUpdates: updates.stream,
        catalogReadChanges: changes.stream,
      );
      final reads = <String>[];
      var canceled = 0;
      MediaImage.configureImageLoader((request, refresh) {
        if (refresh) {
          return CoreQueryScope.current!.execute(
            () => Completer<ImageReadSnapshot>().future,
            onCancel: () {
              canceled++;
            },
          );
        }
        reads.add(request.image.path);
        return Future.value(
          ImageReadSnapshot(
            bytes: request.image.path == 'first' ? red : green,
            hasRefresh: true,
          ),
        );
      });
      final item = Artist.fromJson({
        'item_id': 'one',
        'provider': 'library',
        'name': 'Artist',
        'provider_mappings': [
          {
            'provider_domain': 'source',
            'provider_instance': 'source--one',
            'item_id': 'first',
          },
          {
            'provider_domain': 'source',
            'provider_instance': 'source--one',
            'item_id': 'second',
          },
        ],
      });
      void publish(String id, {bool hasImage = true}) => updates.add(
        CatalogUpdate(
          kind: CatalogQueryKind.providerItem,
          itemId: id,
          provider: 'source--one',
          mediaType: MediaType.artist,
          item: Artist.fromJson({
            'item_id': id,
            'provider': 'source--one',
            'name': id,
            'metadata': {
              'images': [
                if (hasImage)
                  {
                    'type': 'thumb',
                    'provider': 'source',
                    'path': id,
                    'proxy_id': (id == 'first' ? 'a' : 'b') * 64,
                  },
              ],
            },
          }),
        ),
      );
      await tester.pumpWidget(
        MaterialApp(
          home: MediaImage(
            source: item.imageUriForSize(256)!,
            fit: BoxFit.cover,
            fallback: const Text('fallback'),
          ),
        ),
      );
      publish('second');
      await tester.pump();
      expect(reads, ['second']);
      publish('first');
      await tester.pump();
      expect(reads, ['second', 'first']);
      expect(canceled, 1);
      expect(
        (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
        red,
      );
      publish('first');
      await tester.pump();
      expect(reads, hasLength(2));
      publish('first', hasImage: false);
      await tester.pump();
      expect(reads, hasLength(2));
      expect(
        (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
        red,
      );
      void confirm(String id, CatalogReadKnowledge knowledge) => changes.add(
        CatalogReadChange(
          kind: CatalogQueryKind.providerItem,
          phase: CatalogLoadPhase.refreshing,
          knowledge: knowledge,
          freshness: CatalogFreshness.stale,
          itemId: id,
          provider: 'source--one',
          mediaType: MediaType.artist,
        ),
      );
      confirm('first', CatalogReadKnowledge.partial);
      await tester.pump();
      expect(reads, hasLength(2));
      confirm('first', CatalogReadKnowledge.observedResponse);
      await tester.pump();
      expect(reads, ['second', 'first', 'second']);
      expect(canceled, 2);
      expect(
        (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
        green,
      );
      publish('second', hasImage: false);
      confirm('second', CatalogReadKnowledge.observedResponse);
      await tester.pump();
      expect(canceled, 3);
      expect(find.byType(Image), findsNothing);
      expect(find.text('fallback'), findsOneWidget);
      metadata.complete(null);
      await tester.pump();
      expect(find.byType(Image), findsNothing);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await updates.close();
      await changes.close();
    },
  );

  testWidgets(
    'image-less early mapping does not hide a successful terminal artwork',
    (tester) async {
      final bytes = (await tester.runAsync(() => bitmap(Colors.red)))!;
      final metadata = Completer<MediaItemImage?>();
      final updates = StreamController<CatalogUpdate>.broadcast(sync: true);
      MediaImage.configureArtworkLoader(
        (_) => metadata.future,
        catalogUpdates: updates.stream,
      );
      MediaImage.configureImageLoader(
        (_, _) async => ImageReadSnapshot(bytes: bytes),
      );
      await tester.pumpWidget(
        MaterialApp(
          home: MediaImage(
            source: artist().imageUriForSize(256)!,
            fit: BoxFit.cover,
            fallback: const Text('fallback'),
          ),
        ),
      );
      updates.add(
        CatalogUpdate(
          kind: CatalogQueryKind.providerItem,
          itemId: 'other',
          provider: 'source--one',
          mediaType: MediaType.artist,
          item: Artist.fromJson({
            'item_id': 'other',
            'provider': 'source--one',
            'name': 'Empty',
          }),
        ),
      );
      await tester.pump();
      expect(find.text('fallback'), findsOneWidget);
      metadata.complete(
        const MediaItemImage(
          type: 'thumb',
          provider: 'source',
          path: 'cover',
          remotelyAccessible: false,
        ),
      );
      await tester.pumpAndSettle();
      expect(
        (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
        bytes,
      );
      await tester.pumpWidget(const SizedBox.shrink());
      await updates.close();
    },
  );

  testWidgets('hidden images do not subscribe until visible', (tester) async {
    var requests = 0;
    MediaImage.configureArtworkLoader((_) async {
      requests++;
      return null;
    });
    final source = artist().imageUriForSize(256)!;
    Widget image(bool isVisible) => MaterialApp(
      home: TickerMode(
        enabled: isVisible,
        child: MediaImage(
          source: source,
          fit: BoxFit.cover,
          fallback: const SizedBox.shrink(),
        ),
      ),
    );
    await tester.pumpWidget(image(false));
    expect(requests, 0);
    await tester.pumpWidget(image(true));
    await tester.pump();
    expect(requests, 1);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  for (final boundary in ['hide', 'dispose', 'invalidate']) {
    testWidgets('early artwork and byte owners retire at $boundary', (
      tester,
    ) async {
      final red = (await tester.runAsync(() => bitmap(Colors.red)))!;
      final green = (await tester.runAsync(() => bitmap(Colors.green)))!;
      final updates = StreamController<CatalogUpdate>.broadcast(sync: true);
      final changes = StreamController<CatalogReadChange>.broadcast(sync: true);
      final metadata = <Completer<MediaItemImage?>>[];
      final lateBytes = Completer<ImageReadSnapshot>();
      var metadataCanceled = 0;
      var imageCanceled = 0;
      MediaImage.configureArtworkLoader(
        (_) {
          final pending = Completer<MediaItemImage?>();
          metadata.add(pending);
          return CoreQueryScope.current!.execute(
            () => pending.future,
            onCancel: () {
              metadataCanceled++;
            },
          );
        },
        catalogUpdates: updates.stream,
        catalogReadChanges: changes.stream,
      );
      MediaImage.configureImageLoader((request, refresh) {
        if (refresh) {
          return CoreQueryScope.current!.execute(
            () => lateBytes.future,
            onCancel: () {
              imageCanceled++;
            },
          );
        }
        return Future.value(
          ImageReadSnapshot(
            bytes: request.image.path == 'cached' ? red : green,
            hasRefresh: request.image.path == 'cached',
          ),
        );
      });
      final source = artist().imageUriForSize(256)!;
      Widget image(bool isVisible) => MaterialApp(
        home: TickerMode(
          enabled: isVisible,
          child: MediaImage(
            source: source,
            fit: BoxFit.cover,
            fallback: const Text('fallback'),
          ),
        ),
      );
      void publish(String path) => updates.add(
        CatalogUpdate(
          kind: CatalogQueryKind.providerItem,
          itemId: 'other',
          provider: 'source--one',
          mediaType: MediaType.artist,
          item: Artist.fromJson({
            'item_id': 'other',
            'provider': 'source--one',
            'name': 'Artist',
            'metadata': {
              'images': [
                {'type': 'thumb', 'provider': 'source', 'path': path},
              ],
            },
          }),
        ),
      );
      await tester.pumpWidget(image(true));
      publish('cached');
      await tester.pumpAndSettle();
      expect(
        (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
        red,
      );
      if (boundary == 'hide') {
        await tester.pumpWidget(image(false));
      } else if (boundary == 'dispose') {
        await tester.pumpWidget(const SizedBox.shrink());
      } else {
        changes.add(
          const CatalogReadChange(
            kind: CatalogQueryKind.providerItem,
            phase: CatalogLoadPhase.invalidated,
            knowledge: CatalogReadKnowledge.unknown,
            freshness: CatalogFreshness.unknown,
            itemId: 'other',
            provider: 'source--one',
            mediaType: MediaType.artist,
          ),
        );
      }
      await tester.pump();
      expect(metadataCanceled, 1);
      expect(imageCanceled, 1);
      if (boundary != 'invalidate') {
        expect(updates.hasListener, isFalse);
        expect(changes.hasListener, isFalse);
      }
      if (boundary == 'hide') {
        await tester.pumpWidget(image(true));
        await tester.pump();
      }
      if (boundary != 'dispose') {
        expect(metadata, hasLength(2));
        publish('fresh');
        await tester.pumpAndSettle();
        expect(
          (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
          green,
        );
      }
      metadata.first.complete(
        const MediaItemImage(
          type: 'thumb',
          provider: 'source',
          path: 'late',
          remotelyAccessible: false,
        ),
      );
      lateBytes.complete(ImageReadSnapshot(bytes: red));
      await tester.pumpAndSettle();
      if (boundary != 'dispose') {
        expect(
          (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
          green,
        );
      }
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
      expect(updates.hasListener, isFalse);
      expect(changes.hasListener, isFalse);
      expect(tester.takeException(), isNull);
      await updates.close();
      await changes.close();
    });
  }

  testWidgets(
    'refresh failure preserves local bytes and old sessions cannot replace them',
    (tester) async {
      final bytes = (await tester.runAsync(() => bitmap(Colors.red)))!;
      final pending = Completer<ImageReadSnapshot>();
      MediaImage.configureImageLoader(
        (_, refresh) async => refresh
            ? pending.future
            : ImageReadSnapshot(bytes: bytes, hasRefresh: true),
      );
      final source = const MediaItemImage(
        type: 'thumb',
        path: 'data:image/png;base64,unused',
        provider: 'builtin',
        remotelyAccessible: false,
      ).getProxyPath(ownerUri: 'library://album/7')!;
      await tester.pumpWidget(
        MaterialApp(
          home: MediaImage(
            source: source,
            fit: BoxFit.cover,
            fallback: const Text('fallback'),
          ),
        ),
      );
      await tester.pump();
      pending.completeError(StateError('refresh failed'));
      await tester.pump();
      expect(
        (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
        bytes,
      );
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());

      final late = Completer<ImageReadSnapshot>();
      MediaImage.configureImageLoader((_, _) => late.future);
      final oldSource = const MediaItemImage(
        type: 'thumb',
        path: '<svg/>',
        provider: 'builtin',
        remotelyAccessible: false,
      ).getProxyPath()!;
      await tester.pumpWidget(
        MaterialApp(
          home: MediaImage(
            source: oldSource,
            fit: BoxFit.cover,
            fallback: const Text('fallback'),
          ),
        ),
      );
      MediaImage.configureImageLoader(
        (_, _) async => const ImageReadSnapshot(),
      );
      late.complete(ImageReadSnapshot(bytes: bytes));
      await tester.pump();
      expect(find.byType(Image), findsNothing);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  testWidgets(
    'hidden refresh cancels only its subscriber and resumes on visibility',
    (tester) async {
      final bytes = (await tester.runAsync(() => bitmap(Colors.red)))!;
      var started = 0;
      var canceled = 0;
      MediaImage.configureImageLoader((_, refresh) {
        if (!refresh) {
          return Future.value(
            ImageReadSnapshot(bytes: bytes, hasRefresh: true),
          );
        }
        started++;
        return CoreQueryScope.current!.execute(
          () => Completer<ImageReadSnapshot>().future,
          onCancel: () {
            canceled++;
          },
        );
      });
      final source = const MediaItemImage(
        type: 'thumb',
        path: '<svg/>',
        provider: 'builtin',
        remotelyAccessible: false,
      ).getProxyPath(ownerUri: 'library://album/7')!;
      Widget image(bool visible) => MaterialApp(
        home: TickerMode(
          enabled: visible,
          child: MediaImage(
            source: source,
            fit: BoxFit.cover,
            fallback: const Text('fallback'),
          ),
        ),
      );
      await tester.pumpWidget(image(true));
      await tester.pump();
      expect(started, 1);
      await tester.pumpWidget(image(false));
      expect(canceled, 1);
      await tester.pumpWidget(image(true));
      await tester.pump();
      expect(started, 2);
      await tester.pumpWidget(const SizedBox.shrink());
      expect(canceled, 2);
      expect(tester.takeException(), isNull);
    },
  );

  test(
    'percent-encoded inline content crosses the native boundary as base64',
    () {
      const svg = '<svg xmlns="http://www.w3.org/2000/svg"/>';
      final request = ImageRequest(
        image: MediaItemImage(
          type: 'thumb',
          provider: 'builtin',
          path: Uri.dataFromString(svg, mimeType: 'image/svg+xml').toString(),
          remotelyAccessible: false,
        ),
      );
      final wire = request.toJson()['image'] as Map<String, Object?>;
      final uri = UriData.parse(wire['path']! as String);
      expect(uri.isBase64, isTrue);
      expect(uri.contentAsString(), svg);
      expect(
        ImageReadSnapshot.fromJson({
          'bytes': null,
          'cacheError': 'read-only',
          'hasRefresh': false,
        }).cacheError,
        'read-only',
      );
    },
  );

  testWidgets('provider icons use the native loader with their manifest slot', (
    tester,
  ) async {
    final bytes = (await tester.runAsync(() => bitmap(Colors.red)))!;
    final requests = <ImageRequest>[];
    MediaImage.configureImageLoader((request, _) async {
      requests.add(request);
      return ImageReadSnapshot(bytes: bytes);
    });
    ProviderManifestRegistry.registerOne(
      const ProviderManifest(domain: 'spotify', iconSvg: '<svg/>', raw: {}),
    );
    final source = await ProviderManifestRegistry.getProviderIconData(
      'spotify--one',
    );
    await tester.pumpWidget(
      MaterialApp(
        home: MediaImage(
          source: source!,
          fit: BoxFit.contain,
          fallback: const Text('fallback'),
        ),
      ),
    );
    await tester.pump();
    expect(requests.single.providerIconRole, 'icon_svg');
    expect(requests.single.image.provider, 'spotify');
    expect(requests.single.ownerUri, isNull);
    expect(tester.widget<Image>(find.byType(Image)).image, isA<MemoryImage>());
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('default provider icons open local bytes before icon metadata', (
    tester,
  ) async {
    final bytes = (await tester.runAsync(() => bitmap(Colors.red)))!;
    final metadata = Completer<String?>();
    var metadataReads = 0;
    final requests = <ImageRequest>[];
    MediaImage.configureImageLoader((request, _) async {
      requests.add(request);
      return ImageReadSnapshot(bytes: bytes);
    });
    ProviderManifestRegistry.configureIconLoader((_) {
      metadataReads++;
      return metadata.future;
    });
    ProviderManifestRegistry.registerOne(
      const ProviderManifest(
        domain: 'spotify',
        iconImages: ['default'],
        raw: {},
      ),
    );
    String? source;
    unawaited(
      ProviderManifestRegistry.getProviderIconData('spotify--one').then((
        value,
      ) {
        source = value;
      }),
    );
    await tester.pump();
    expect(source, isNotNull);
    expect(metadataReads, 0);
    await tester.pumpWidget(
      MaterialApp(
        home: MediaImage(
          source: source!,
          fit: BoxFit.contain,
          fallback: const Text('fallback'),
        ),
      ),
    );
    await tester.pump();
    expect(requests.single.providerIconRole, 'default');
    expect(requests.single.image.provider, 'spotify');
    expect(tester.widget<Image>(find.byType(Image)).image, isA<MemoryImage>());
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets(
    'table default icon shows native bytes without metadata and cancels hidden refresh',
    (tester) async {
      final bytes = (await tester.runAsync(() => bitmap(Colors.red)))!;
      var metadataReads = 0;
      var reads = 0;
      var canceled = 0;
      ProviderManifestRegistry.configureIconLoader((_) {
        metadataReads++;
        return Completer<String?>().future;
      });
      ProviderManifestRegistry.registerOne(
        const ProviderManifest(
          domain: 'spotify',
          iconImages: ['default'],
          raw: {},
        ),
      );
      MediaImage.configureImageLoader((request, refresh) {
        expect(request.providerIconRole, 'default');
        expect(request.image.provider, 'spotify');
        if (refresh) {
          return CoreQueryScope.current!.execute(
            () => Completer<ImageReadSnapshot>().future,
            onCancel: () {
              canceled++;
            },
          );
        }
        reads++;
        return Future.value(ImageReadSnapshot(bytes: bytes, hasRefresh: true));
      });
      Widget icon(bool isVisible) => MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: TickerMode(
            enabled: isVisible,
            child: const MediaProviderIcon(
              providerId: 'spotify--one',
              providerLabel: 'Spotify',
            ),
          ),
        ),
      );
      await tester.pumpWidget(icon(true));
      await tester.pumpAndSettle();
      expect(metadataReads, 0);
      expect(reads, 1);
      expect(
        (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
        bytes,
      );
      expect(tester.getSize(find.byType(MediaImage)), const Size(16, 16));
      await tester.pumpWidget(icon(false));
      expect(canceled, 1);
      await tester.pumpWidget(icon(true));
      await tester.pump();
      expect(reads, 2);
      expect(metadataReads, 0);
      await tester.pumpWidget(const SizedBox.shrink());
      expect(canceled, 2);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'table icon manifest revisions retain bytes until the new local read completes',
    (tester) async {
      final red = (await tester.runAsync(() => bitmap(Colors.red)))!;
      final green = (await tester.runAsync(() => bitmap(Colors.green)))!;
      final pending = Completer<ImageReadSnapshot>();
      var reads = 0;
      MediaImage.configureImageLoader((_, _) async {
        reads++;
        return reads == 1 ? ImageReadSnapshot(bytes: red) : pending.future;
      });
      ProviderManifestRegistry.registerOne(
        const ProviderManifest(domain: 'spotify', iconSvg: '<svg/>', raw: {}),
      );
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.dark(),
          home: const Scaffold(
            body: MediaProviderIcon(
              providerId: 'spotify--one',
              providerLabel: 'Spotify',
            ),
          ),
        ),
      );
      await tester.pump();
      expect(
        (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
        red,
      );
      ProviderManifestRegistry.registerOne(
        const ProviderManifest(
          domain: 'spotify',
          iconSvg: '<svg>new</svg>',
          raw: {},
        ),
      );
      await tester.pump();
      expect(reads, 2);
      expect(
        (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
        red,
      );
      pending.complete(ImageReadSnapshot(bytes: green));
      await tester.pumpAndSettle();
      expect(
        (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
        green,
      );
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );
}
