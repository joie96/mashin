import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:mashin/features/core/core_models.dart';
import 'package:mashin/features/shared/widgets/media_image.dart';

void main() {
  tearDown(() {
    MediaImage.configureArtworkLoader(null);
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

  testWidgets('proxy artwork uses Flutter network rendering', (tester) async {
    MediaItemImage.configureProxyBaseUrl('https://music.example');
    const source = 'https://music.example/imageproxy/observed-cover?size=160';
    await tester.pumpWidget(
      const MaterialApp(
        home: MediaImage(
          source: source,
          fit: BoxFit.cover,
          fallback: Text('fallback'),
        ),
      ),
    );
    final image = tester.widget<Image>(find.byType(Image));
    expect(image.image, isA<NetworkImage>());
    expect((image.image as NetworkImage).url, source);
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
    expect(
      (image.image as NetworkImage).url,
      'https://music.example/imageproxy/observed-cover?size=256',
    );
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
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
}
