import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';

import '../../core/core_models.dart';
import '../../core/core_query_scope.dart';
import '../detail_page_lifecycle.dart';

/// Blurred online artwork background with a neutral readability overlay.
class MediaArtworkBackground extends StatelessWidget {
  final String source;
  final BorderRadius borderRadius;
  final Widget child;

  const MediaArtworkBackground({
    super.key,
    required this.source,
    required this.borderRadius,
    required this.child,
  });

  @override
  Widget build(BuildContext context) {
    return ClipRRect(
      borderRadius: borderRadius,
      child: Stack(
        fit: StackFit.expand,
        children: <Widget>[
          Opacity(
            opacity: 0.8,
            child: ImageFiltered(
              imageFilter: ImageFilter.blur(sigmaX: 48, sigmaY: 48),
              child: Transform.scale(
                scale: 1.16,
                child: MediaImage(
                  source: source,
                  fit: BoxFit.cover,
                  fallback: const SizedBox.shrink(),
                ),
              ),
            ),
          ),
          const DecoratedBox(
            decoration: BoxDecoration(
              gradient: LinearGradient(
                begin: Alignment.topCenter,
                end: Alignment.bottomCenter,
                colors: <Color>[Color(0x70404040), Color(0xB0000000)],
              ),
            ),
          ),
          child,
        ],
      ),
    );
  }
}

/// Displays server proxy artwork or inline image content without an app-owned cache.
class MediaImage extends StatefulWidget {
  static Future<MediaItemImage?> Function(MediaItem)? _loadArtwork;
  static int _artworkGeneration = 0;

  // Model getters create requests only; visible widgets own the online reads.
  static void configureArtworkLoader(
    Future<MediaItemImage?> Function(MediaItem)? loader,
  ) {
    _artworkGeneration++;
    _loadArtwork = loader;
    MediaItemImage.missingArtworkSource = loader == null
        ? null
        : _createArtworkSource;
  }

  static String? _createArtworkSource(MediaItem item, int size) {
    if (item.itemId.isEmpty ||
        !item.providerMappings.any(
          (mapping) =>
              mapping.providerInstance != item.provider ||
              mapping.itemId != item.itemId,
        )) {
      return null;
    }
    return Uri(
      scheme: 'mashin-artwork',
      host: item.mediaType.name,
      pathSegments: [item.provider, item.itemId],
      queryParameters: {
        'size': '$size',
        'session': '$_artworkGeneration',
        'item': jsonEncode(createMusicAssistantMediaItemPayload(item)),
      },
    ).toString();
  }

  const MediaImage({
    super.key,
    required this.source,
    required this.fit,
    required this.fallback,
  });

  final String source;
  final BoxFit fit;
  final Widget fallback;

  @override
  State<MediaImage> createState() => _MediaImageState();
}

class _MediaImageState extends State<MediaImage>
    with DetailPageLifecycle<MediaImage> {
  CoreQueryScope _queryScope = CoreQueryScope();
  String? _resolvedSource;
  bool _isVisible = true;
  bool _isLoadingArtwork = false;
  bool _hasLoadedArtwork = false;
  int _loadGeneration = 0;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final isVisible =
        TickerMode.valuesOf(context).enabled &&
        (ModalRoute.of(context)?.isCurrent ?? true);
    onDetailPageVisibilityChanged(isVisible);
    if (isVisible) {
      unawaited(_loadMissingArtwork());
    }
  }

  @override
  void didUpdateWidget(covariant MediaImage oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.source != widget.source) {
      _cancelArtwork();
      _queryScope = CoreQueryScope();
      _resolvedSource = null;
      _hasLoadedArtwork = false;
      unawaited(_loadMissingArtwork());
    }
  }

  @override
  void onDetailPageVisibilityChanged(bool isVisible) {
    if (_isVisible == isVisible) return;
    _isVisible = isVisible;
    if (!isVisible) {
      _cancelArtwork();
    } else {
      _queryScope = CoreQueryScope();
      unawaited(_loadMissingArtwork());
    }
  }

  @override
  void dispose() {
    _cancelArtwork();
    super.dispose();
  }

  void _cancelArtwork() {
    _loadGeneration++;
    _queryScope.cancel();
    _isLoadingArtwork = false;
  }

  // Every image subscribes separately, so hiding one cannot cancel another visible consumer.
  Future<void> _loadMissingArtwork() async {
    final source = widget.source;
    final loader = MediaImage._loadArtwork;
    if (!_isVisible ||
        _isLoadingArtwork ||
        _hasLoadedArtwork ||
        !source.startsWith('mashin-artwork:') ||
        loader == null) {
      return;
    }
    _isLoadingArtwork = true;
    final generation = ++_loadGeneration;
    final sessionGeneration = MediaImage._artworkGeneration;
    try {
      final request = Uri.parse(source);
      if (request.queryParameters['session'] != '$sessionGeneration') return;
      final item = mediaItemFromJson(
        jsonDecode(request.queryParameters['item']!) as Map<String, dynamic>,
      );
      final image = await _queryScope.run(
        () => _queryScope.execute(() => loader(item), onCancel: () {}),
      );
      if (!mounted ||
          generation != _loadGeneration ||
          sessionGeneration != MediaImage._artworkGeneration) {
        return;
      }
      final size = int.tryParse(request.queryParameters['size'] ?? '');
      final resolved = image?.getProxyPath(size: size);
      setState(() {
        _resolvedSource = resolved;
        _hasLoadedArtwork = true;
      });
    } on CoreQueryCanceled {
      return;
    } catch (_) {
      return;
    } finally {
      if (generation == _loadGeneration) _isLoadingArtwork = false;
    }
  }

  Uint8List? _decodeDataUri(String value) {
    try {
      if (!value.toLowerCase().startsWith('data:image/')) return null;
      final decoded = UriData.parse(value).contentAsBytes();
      return decoded.isEmpty ? null : decoded;
    } on FormatException {
      return null;
    }
  }

  @override
  Widget build(BuildContext context) {
    final String trimmedSource =
        (widget.source.startsWith('mashin-artwork:')
                ? _resolvedSource ?? ''
                : widget.source)
            .trim();
    if (trimmedSource.startsWith('<svg') || trimmedSource.startsWith('<?xml')) {
      return SvgPicture.string(
        trimmedSource,
        fit: widget.fit,
        errorBuilder: (_, _, _) => widget.fallback,
      );
    }
    final Uint8List? bytes = _decodeDataUri(trimmedSource);
    if (bytes != null && bytes.isNotEmpty) {
      if (trimmedSource.toLowerCase().startsWith('data:image/svg+xml')) {
        return SvgPicture.memory(
          bytes,
          fit: widget.fit,
          errorBuilder: (_, _, _) => widget.fallback,
        );
      }
      return Image.memory(
        bytes,
        fit: widget.fit,
        gaplessPlayback: true,
        errorBuilder: (_, _, _) => widget.fallback,
      );
    }

    if (!_isVisible || !MediaItemImage.isProxyUrl(trimmedSource)) {
      return widget.fallback;
    }

    return Image.network(
      trimmedSource,
      fit: widget.fit,
      gaplessPlayback: true,
      errorBuilder: (_, _, _) => widget.fallback,
    );
  }
}
