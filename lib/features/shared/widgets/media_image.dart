import 'dart:async';
import 'dart:convert';
import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:flutter/foundation.dart';
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

/// Visible subscribers own local image reads and their optional native refresh.
class MediaImage extends StatefulWidget {
  static const _maximumRetainedBytes = 128 * 1024 * 1024;
  static final _retainedImages = <String, _RetainedImage>{};
  static int _retainedByteCount = 0;
  static Future<MediaItemImage?> Function(MediaItem)? _loadArtwork;
  static Stream<CatalogUpdate>? _artworkUpdates;
  static Stream<CatalogReadChange>? _artworkReadChanges;
  static int _artworkGeneration = 0;
  static Future<ImageReadSnapshot> Function(ImageRequest, bool)? _loadImage;
  static final ValueNotifier<int> _imageChanges = ValueNotifier<int>(0);
  static final ValueNotifier<int> _cacheChanges = ValueNotifier<int>(0);
  static StreamSubscription<void>? _imageInvalidationSubscription;
  static StreamSubscription<String>? _sourceChangeSubscription;
  static final ValueNotifier<(int, String)> _sourceChanges = ValueNotifier((
    0,
    '',
  ));
  static int get _imageGeneration => _imageChanges.value;

  static void configureImageLoader(
    Future<ImageReadSnapshot> Function(ImageRequest, bool)? loader, {
    Stream<void>? invalidations,
    Stream<String>? sourceChanges,
  }) {
    unawaited(_imageInvalidationSubscription?.cancel());
    _imageInvalidationSubscription = null;
    unawaited(_sourceChangeSubscription?.cancel());
    _sourceChangeSubscription = null;
    _clearRetainedImages();
    _loadImage = loader;
    MediaItemImage.cachedImageSource = loader == null
        ? null
        : _createImageSource;
    ProviderManifestRegistry.cachedIconSource = loader == null
        ? null
        : _createImageSource;
    _imageChanges.value++;
    final generation = _imageGeneration;
    if (loader != null && invalidations != null) {
      _imageInvalidationSubscription = invalidations.listen((_) {
        if (generation == _imageGeneration) {
          _clearRetainedImages();
          _cacheChanges.value++;
        }
      });
    }
    if (loader != null && sourceChanges != null) {
      _sourceChangeSubscription = sourceChanges.listen((proxyId) {
        if (generation == _imageGeneration) {
          for (final source in _retainedImages.keys.toList()) {
            if (_retainedImages[source]!.proxyId == proxyId) {
              _retainedByteCount -= _retainedImages
                  .remove(source)!
                  .bytes
                  .length;
            }
          }
          _sourceChanges.value = (_sourceChanges.value.$1 + 1, proxyId);
        }
      });
    }
  }

  // Retained bytes are presentation-only; every new subscriber still validates natively.
  static Uint8List? _readRetainedImage(String source) {
    final image = _retainedImages.remove(source);
    if (image == null) return null;
    _retainedImages[source] = image;
    return image.bytes;
  }

  static Uint8List _retainImage(
    String source,
    String? proxyId,
    Uint8List bytes,
  ) {
    final previous = _retainedImages.remove(source);
    if (previous != null) _retainedByteCount -= previous.bytes.length;
    final retained = previous != null && listEquals(previous.bytes, bytes)
        ? previous.bytes
        : bytes;
    if (retained.length > _maximumRetainedBytes) return retained;
    while (_retainedImages.isNotEmpty &&
        _retainedByteCount + retained.length > _maximumRetainedBytes) {
      _retainedByteCount -= _retainedImages
          .remove(_retainedImages.keys.first)!
          .bytes
          .length;
    }
    _retainedImages[source] = _RetainedImage(retained, proxyId);
    _retainedByteCount += retained.length;
    return retained;
  }

  static void _clearRetainedImages() {
    _retainedImages.clear();
    _retainedByteCount = 0;
  }

  static String _createImageSource(ImageRequest request) => Uri(
    scheme: 'mashin-image',
    host: 'model',
    queryParameters: {
      'session': '$_imageGeneration',
      'request': jsonEncode(request.toJson()),
    },
  ).toString();

  static String _retainedImageKey(String source, ImageRequest request) {
    final proxyId = request.image.proxyId?.trim();
    if (proxyId == null || !RegExp(r'^[a-f0-9]{64}$').hasMatch(proxyId)) {
      return source;
    }
    return jsonEncode([_imageGeneration, proxyId, request.size]);
  }

  static bool _hasSameImageIdentity(String previous, String current) {
    try {
      final previousUri = Uri.parse(previous);
      final currentUri = Uri.parse(current);
      if (previousUri.scheme != 'mashin-image' ||
          currentUri.scheme != 'mashin-image' ||
          previousUri.queryParameters['session'] !=
              currentUri.queryParameters['session']) {
        return false;
      }
      final previousRequest =
          jsonDecode(previousUri.queryParameters['request']!)
              as Map<String, dynamic>;
      final currentRequest =
          jsonDecode(currentUri.queryParameters['request']!)
              as Map<String, dynamic>;
      final previousImage =
          previousRequest.remove('image') as Map<String, dynamic>;
      final currentImage =
          currentRequest.remove('image') as Map<String, dynamic>;
      if (previousRequest['providerIconRole'] != null &&
          previousImage['proxy_id'] == null &&
          currentImage['proxy_id'] == null) {
        previousImage.remove('path');
        currentImage.remove('path');
        return mapEquals(previousRequest, currentRequest) &&
            mapEquals(previousImage, currentImage);
      }
      final previousReference =
          previousImage.remove('cached_inline') as Map<String, dynamic>?;
      final currentReference =
          currentImage.remove('cached_inline') as Map<String, dynamic>?;
      if (previousReference == null || currentReference == null) return false;
      previousReference.remove('content_hash');
      currentReference.remove('content_hash');
      return mapEquals(previousRequest, currentRequest) &&
          mapEquals(previousImage, currentImage) &&
          mapEquals(previousReference, currentReference);
    } catch (_) {
      return false;
    }
  }

  // Model getters create requests only; visible widgets own the online reads.
  static void configureArtworkLoader(
    Future<MediaItemImage?> Function(MediaItem)? loader, {
    Stream<CatalogUpdate>? catalogUpdates,
    Stream<CatalogReadChange>? catalogReadChanges,
  }) {
    _artworkGeneration++;
    _loadArtwork = loader;
    _artworkUpdates = catalogUpdates;
    _artworkReadChanges = catalogReadChanges;
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
    this.cacheWidth,
    this.cacheHeight,
    this.isBackground = false,
  });

  final String source;
  final BoxFit fit;
  final Widget fallback;
  final int? cacheWidth;
  final int? cacheHeight;
  final bool isBackground;

  @override
  State<MediaImage> createState() => _MediaImageState();
}

class _RetainedImage {
  const _RetainedImage(this.bytes, this.proxyId);

  final Uint8List bytes;
  final String? proxyId;
}

/// Owns background image reads for the two pages beside the displayed page.
class MediaPageImagePreload extends StatelessWidget {
  const MediaPageImagePreload({
    super.key,
    required this.items,
    required this.pageStart,
    required this.pageSize,
    required this.imageSize,
    required this.child,
    this.isEnabled = true,
  });

  final List<Object?> items;
  final int pageStart;
  final int pageSize;
  final int imageSize;
  final bool isEnabled;
  final Widget child;

  @override
  Widget build(BuildContext context) => Stack(
    children: [
      child,
      if (isEnabled && pageSize > 0)
        Positioned(
          top: 0,
          left: 0,
          child: ExcludeSemantics(
            child: IgnorePointer(
              child: Opacity(
                opacity: 0,
                child: ValueListenableBuilder<int>(
                  valueListenable: ProviderManifestRegistry.changes,
                  builder: (context, _, _) {
                    final sources = <String>{};
                    for (final start in [
                      pageStart - pageSize,
                      pageStart + pageSize,
                    ]) {
                      if (start < 0 || start >= items.length) continue;
                      for (final item in items.skip(start).take(pageSize)) {
                        final mediaItem = switch (item) {
                          QueueItem() => item.mediaItem,
                          MediaItem() => item,
                          _ => null,
                        };
                        if (mediaItem == null) continue;
                        final image = item is QueueItem
                            ? item.imageUriForSize(imageSize)
                            : mediaItem.imageUriForSize(imageSize);
                        if (image != null && image.isNotEmpty) {
                          sources.add(image);
                        }
                        final providers = {
                          mediaItem.provider,
                          for (final mapping in mediaItem.providerMappings)
                            if (mapping.available) mapping.providerDomain,
                        };
                        for (final provider in providers) {
                          final icon =
                              ProviderManifestRegistry.iconSourceForProvider(
                                provider,
                              );
                          if (icon != null && icon.isNotEmpty) {
                            sources.add(icon);
                          }
                        }
                      }
                    }
                    return SizedBox.square(
                      dimension: 1,
                      child: Stack(
                        children: [
                          for (final source in sources)
                            Positioned.fill(
                              child: MediaImage(
                                key: ValueKey(source),
                                source: source,
                                fit: BoxFit.contain,
                                fallback: const SizedBox.shrink(),
                                isBackground: true,
                              ),
                            ),
                        ],
                      ),
                    );
                  },
                ),
              ),
            ),
          ),
        ),
    ],
  );
}

class _MediaImageState extends State<MediaImage>
    with DetailPageLifecycle<MediaImage> {
  CoreQueryScope _queryScope = CoreQueryScope();
  late CoreQueryScope _imageScope = _queryScope.createChild();
  String? _resolvedSource;
  bool _isVisible = true;
  bool _isLoadingArtwork = false;
  bool _hasLoadedArtwork = false;
  bool _isLoadingImage = false;
  bool _hasLoadedImage = false;
  Uint8List? _bytes;
  String? _proxyId;
  bool _hasPendingSourceChange = false;
  int _loadGeneration = 0;
  int _imageLoadGeneration = 0;
  List<ScrollPosition> _scrollPositions = [];
  bool _isInViewport = true;
  bool _isNearViewport = true;
  bool _isBackgroundImageRead = false;
  bool _isViewportCheckQueued = false;
  Completer<void>? _initialLayout;

  @override
  void initState() {
    super.initState();
    MediaImage._imageChanges.addListener(_onImageSessionChanged);
    MediaImage._cacheChanges.addListener(_onImageSessionChanged);
    MediaImage._sourceChanges.addListener(_onImageSourceChanged);
  }

  void _onImageSourceChanged() {
    if (_proxyId != MediaImage._sourceChanges.value.$2) return;
    _hasPendingSourceChange = true;
    if (_isLoadingImage) return;
    _hasPendingSourceChange = false;
    _hasLoadedImage = false;
    if (_isVisible) unawaited(_loadMissingArtwork());
  }

  void _onImageSessionChanged() {
    _cancelArtwork();
    _queryScope = CoreQueryScope();
    _imageScope = _queryScope.createChild();
    setState(() {
      _bytes = null;
      _proxyId = null;
      _hasPendingSourceChange = false;
      _resolvedSource = null;
      _hasLoadedImage = false;
      _hasLoadedArtwork = false;
    });
    if (_isVisible) unawaited(_loadMissingArtwork());
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    Scrollable.maybeOf(context, axis: Axis.vertical);
    Scrollable.maybeOf(context, axis: Axis.horizontal);
    final positions = <ScrollPosition>[];
    context.visitAncestorElements((element) {
      if (element is StatefulElement) {
        final state = element.state;
        if (state is ScrollableState) positions.add(state.position);
      }
      return true;
    });
    if (!listEquals(positions, _scrollPositions)) {
      for (final position in _scrollPositions) {
        position.removeListener(_scheduleViewportCheck);
      }
      _scrollPositions = positions;
      for (final position in positions) {
        position.addListener(_scheduleViewportCheck);
      }
    }
    if (positions.isNotEmpty && _initialLayout == null) {
      _initialLayout = Completer<void>();
    }
    final isVisible =
        TickerMode.valuesOf(context).enabled &&
        (ModalRoute.of(context)?.isCurrent ?? true);
    onDetailPageVisibilityChanged(isVisible);
    if (isVisible) {
      _scheduleViewportCheck();
      unawaited(_loadMissingArtwork());
    }
  }

  // Layout, not cache-extent construction, determines the interactive artwork lane.
  void _scheduleViewportCheck() {
    if (_isViewportCheckQueued) return;
    _isViewportCheckQueued = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _isViewportCheckQueued = false;
      if (!mounted || !_isVisible) return;
      final render = context.findRenderObject();
      _isInViewport = true;
      _isNearViewport = true;
      if (render is RenderBox && render.hasSize) {
        var visibleBounds = render.localToGlobal(Offset.zero) & render.size;
        var prefetchBounds = visibleBounds;
        for (final position in _scrollPositions) {
          final viewport = position.context.notificationContext
              ?.findRenderObject();
          if (viewport is! RenderBox || !viewport.hasSize) continue;
          final bounds = viewport.localToGlobal(Offset.zero) & viewport.size;
          visibleBounds = visibleBounds.intersect(bounds);
          prefetchBounds = prefetchBounds.intersect(
            Rect.fromLTRB(
              bounds.left - bounds.width,
              bounds.top - bounds.height,
              bounds.right + bounds.width,
              bounds.bottom + bounds.height,
            ),
          );
        }
        _isInViewport = !visibleBounds.isEmpty;
        _isNearViewport = !prefetchBounds.isEmpty;
      }
      if (_initialLayout != null && !_initialLayout!.isCompleted) {
        _initialLayout!.complete();
      }
      if (!widget.isBackground &&
          _isInViewport &&
          _isLoadingImage &&
          _isBackgroundImageRead) {
        _imageLoadGeneration++;
        _imageScope.cancel();
        _imageScope = _queryScope.createChild();
        _isLoadingImage = false;
      }
      unawaited(_loadMissingArtwork());
    });
  }

  @override
  void didUpdateWidget(covariant MediaImage oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.source != widget.source) {
      final shouldRetainBytes = MediaImage._hasSameImageIdentity(
        oldWidget.source,
        widget.source,
      );
      _cancelArtwork();
      _queryScope = CoreQueryScope();
      _imageScope = _queryScope.createChild();
      _resolvedSource = null;
      _proxyId = null;
      _hasLoadedArtwork = false;
      _hasLoadedImage = false;
      if (!shouldRetainBytes) _bytes = null;
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
      _imageScope = _queryScope.createChild();
      unawaited(_loadMissingArtwork());
    }
  }

  @override
  void dispose() {
    for (final position in _scrollPositions) {
      position.removeListener(_scheduleViewportCheck);
    }
    if (_initialLayout != null && !_initialLayout!.isCompleted) {
      _initialLayout!.complete();
    }
    MediaImage._imageChanges.removeListener(_onImageSessionChanged);
    MediaImage._cacheChanges.removeListener(_onImageSessionChanged);
    MediaImage._sourceChanges.removeListener(_onImageSourceChanged);
    _cancelArtwork();
    super.dispose();
  }

  void _cancelArtwork() {
    _loadGeneration++;
    _imageLoadGeneration++;
    _hasPendingSourceChange = false;
    if (_isLoadingArtwork) _hasLoadedArtwork = false;
    _queryScope.cancel();
    _isLoadingArtwork = false;
    _isLoadingImage = false;
  }

  // Every image subscribes separately, so hiding one cannot cancel another visible consumer.
  Future<void> _loadMissingArtwork() async {
    final source = widget.source;
    if (!source.startsWith('mashin-artwork:')) {
      await _loadImageSource(source);
      return;
    }
    final retained = MediaImage._readRetainedImage(source);
    if (retained != null && !listEquals(_bytes, retained)) {
      setState(() => _bytes = retained);
    }
    if (_initialLayout != null) await _initialLayout!.future;
    if (!mounted || !_isVisible || !_isNearViewport) return;
    if (_resolvedSource != null) {
      unawaited(_loadImageSource(_resolvedSource!));
      if (_hasLoadedArtwork) return;
    }
    final loader = MediaImage._loadArtwork;
    if (!_isVisible ||
        _isLoadingArtwork ||
        _hasLoadedArtwork ||
        !source.startsWith('mashin-artwork:') ||
        loader == null) {
      return;
    }
    _isLoadingArtwork = true;
    final generation = _loadGeneration;
    final sessionGeneration = MediaImage._artworkGeneration;
    StreamSubscription<CatalogUpdate>? updates;
    StreamSubscription<CatalogReadChange>? readChanges;
    bool isCurrent() =>
        mounted &&
        _isVisible &&
        generation == _loadGeneration &&
        sessionGeneration == MediaImage._artworkGeneration;
    try {
      final request = Uri.parse(source);
      if (request.queryParameters['session'] != '$sessionGeneration') return;
      final item = mediaItemFromJson(
        jsonDecode(request.queryParameters['item']!) as Map<String, dynamic>,
      );
      final observed = <(String, String), MediaItemImage?>{};
      final pendingEmpty = <(String, String)>{};
      bool matches(String? itemId, String? provider, MediaType? mediaType) =>
          mediaType == item.mediaType &&
          item.providerMappings.any(
            (mapping) =>
                mapping.itemId == itemId &&
                mapping.providerInstance == provider,
          );
      void applyArtwork(MediaItemImage? image) {
        if (!isCurrent()) return;
        final size = int.tryParse(request.queryParameters['size'] ?? '');
        final resolved = image?.getProxyPath(size: size, ownerUri: item.uri);
        if (_hasLoadedArtwork && resolved == _resolvedSource) return;
        _imageScope.cancel();
        _imageScope = _queryScope.createChild();
        _imageLoadGeneration++;
        _isLoadingImage = false;
        _hasLoadedImage = false;
        _proxyId = null;
        setState(() {
          _resolvedSource = resolved;
          _hasLoadedArtwork = true;
          if (resolved == null) _bytes = null;
        });
        if (resolved != null) unawaited(_loadImageSource(resolved));
      }

      void applyObservedArtwork() {
        final image = item.providerMappings
            .map(
              (mapping) => observed[(mapping.itemId, mapping.providerInstance)],
            )
            .whereType<MediaItemImage>()
            .firstOrNull;
        if (image != null ||
            item.providerMappings.every(
              (mapping) => observed.containsKey((
                mapping.itemId,
                mapping.providerInstance,
              )),
            )) {
          applyArtwork(image);
        }
      }

      updates = MediaImage._artworkUpdates?.listen((update) {
        if (!isCurrent() ||
            update.kind != CatalogQueryKind.providerItem ||
            !matches(update.itemId, update.provider, update.mediaType) ||
            update.item == null) {
          return;
        }
        final mappedItem = update.item!;
        final candidates = <MediaItemImage>[
          ...?mappedItem.metadata?.images,
          if (mappedItem is Track && mappedItem.image != null)
            mappedItem.image!,
        ];
        final image = candidates
            .where(
              (image) =>
                  image.cachedInline != null ||
                  image.path.trim().isNotEmpty ||
                  (image.proxyId?.trim().isNotEmpty ?? false),
            )
            .firstOrNull;
        final key = (update.itemId!, update.provider!);
        if (image == null) {
          pendingEmpty.add(key);
        } else {
          pendingEmpty.remove(key);
          observed[key] = image;
          applyObservedArtwork();
        }
      });
      readChanges = MediaImage._artworkReadChanges?.listen((change) {
        if (isCurrent() &&
            change.kind == CatalogQueryKind.providerItem &&
            change.phase == CatalogLoadPhase.invalidated &&
            matches(change.itemId, change.provider, change.mediaType)) {
          _onImageSessionChanged();
        } else if (isCurrent() &&
            change.kind == CatalogQueryKind.providerItem &&
            change.knowledge == CatalogReadKnowledge.observedResponse &&
            matches(change.itemId, change.provider, change.mediaType) &&
            pendingEmpty.remove((change.itemId!, change.provider!))) {
          observed[(change.itemId!, change.provider!)] = null;
          applyObservedArtwork();
        }
      });
      final image = await _queryScope.run(
        () => _queryScope.execute(() => loader(item), onCancel: () {}),
      );
      if (!isCurrent()) return;
      if (!observed.values.any((value) => value != null)) applyArtwork(image);
    } on CoreQueryCanceled {
      return;
    } catch (_) {
      return;
    } finally {
      await Future.wait<void>([
        if (updates != null) updates.cancel(),
        if (readChanges != null) readChanges.cancel(),
      ]);
      if (generation == _loadGeneration) _isLoadingArtwork = false;
    }
  }

  // Follow-up reads share the widget's native cancellation scope; late bytes never cross sessions.
  Future<void> _loadImageSource(String source) async {
    final loader = MediaImage._loadImage;
    if (loader == null || !_isVisible || _isLoadingImage || _hasLoadedImage) {
      return;
    }
    _isLoadingImage = true;
    final generation = ++_imageLoadGeneration;
    final sessionGeneration = MediaImage._imageGeneration;
    bool isCurrent() =>
        mounted &&
        _isVisible &&
        generation == _imageLoadGeneration &&
        sessionGeneration == MediaImage._imageGeneration;
    try {
      final ImageRequest request;
      if (source.startsWith('mashin-image:')) {
        final uri = Uri.parse(source);
        if (uri.queryParameters['session'] != '$sessionGeneration') return;
        request = ImageRequest.fromJson(
          jsonDecode(uri.queryParameters['request']!) as Map<String, dynamic>,
        );
      } else if (MediaItemImage.isProxyUrl(source)) {
        final uri = Uri.parse(source);
        request = ImageRequest(
          image: MediaItemImage(
            type: 'thumb',
            path: '',
            provider: 'builtin',
            remotelyAccessible: false,
            proxyId: uri.pathSegments.last,
          ),
          size: MediaItemImage.mapToAllowedSize(
            int.tryParse(uri.queryParameters['size'] ?? '') ?? 512,
          ),
        );
      } else if (source.startsWith('data:image/') ||
          source.trimLeft().startsWith('<')) {
        request = ImageRequest(
          image: MediaItemImage(
            type: 'thumb',
            path: source,
            provider: 'builtin',
            remotelyAccessible: false,
          ),
        );
      } else {
        return;
      }
      _proxyId = request.image.proxyId;
      final retainedKey = widget.source.startsWith('mashin-artwork:')
          ? widget.source
          : MediaImage._retainedImageKey(source, request);
      final retained = MediaImage._readRetainedImage(retainedKey);
      if (retained != null && !listEquals(_bytes, retained)) {
        setState(() => _bytes = retained);
      }
      if (_initialLayout != null) await _initialLayout!.future;
      if (!isCurrent() || !_isNearViewport) return;
      _isBackgroundImageRead = widget.isBackground || !_isInViewport;
      final scheduledRequest = ImageRequest(
        image: request.image,
        ownerUri: request.ownerUri,
        position: request.position,
        size: request.size,
        providerIconRole: request.providerIconRole,
        isBackground: _isBackgroundImageRead,
      );
      var snapshot = await _imageScope.run(
        () => loader(scheduledRequest, false),
      );
      if (!isCurrent()) return;
      if (snapshot.bytes != null) {
        final bytes = MediaImage._retainImage(
          retainedKey,
          _proxyId,
          snapshot.bytes!,
        );
        if (!listEquals(_bytes, bytes)) setState(() => _bytes = bytes);
      }
      if (snapshot.hasRefresh) {
        snapshot = await _imageScope.run(() => loader(scheduledRequest, true));
        if (!isCurrent()) return;
        if (snapshot.bytes != null) {
          final bytes = MediaImage._retainImage(
            retainedKey,
            _proxyId,
            snapshot.bytes!,
          );
          if (!listEquals(_bytes, bytes)) setState(() => _bytes = bytes);
        }
      }
      _hasLoadedImage = true;
    } on CoreQueryCanceled {
      return;
    } catch (_) {
      return;
    } finally {
      if (generation == _imageLoadGeneration) {
        _isLoadingImage = false;
        if (_hasPendingSourceChange) {
          _hasPendingSourceChange = false;
          _hasLoadedImage = false;
          if (_isVisible) unawaited(_loadMissingArtwork());
        }
      }
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
    final cachedBytes = _bytes;
    if (cachedBytes != null) {
      return Image.memory(
        cachedBytes,
        fit: widget.fit,
        cacheWidth: widget.cacheWidth,
        cacheHeight: widget.cacheHeight,
        gaplessPlayback: true,
        errorBuilder: (_, _, _) => widget.fallback,
      );
    }
    if (MediaImage._loadImage != null ||
        widget.source.startsWith('mashin-image:')) {
      return widget.fallback;
    }
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
        cacheWidth: widget.cacheWidth,
        cacheHeight: widget.cacheHeight,
        gaplessPlayback: true,
        errorBuilder: (_, _, _) => widget.fallback,
      );
    }

    if (!_isVisible || !MediaItemImage.isProxyUrl(trimmedSource)) {
      return widget.fallback;
    }

    return widget.fallback;
  }
}
