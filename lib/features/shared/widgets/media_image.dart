import 'dart:convert';
import 'dart:typed_data';
import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:http/http.dart' as http;
import 'package:image/image.dart' as img;

/// Blurred artwork background with a dominant-color tint.
class MediaArtworkBackground extends StatefulWidget {
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
  State<MediaArtworkBackground> createState() => _MediaArtworkBackgroundState();
}

class _MediaArtworkBackgroundState extends State<MediaArtworkBackground> {
  // Avoids recalculating colors while browsing the same covers.
  static final Map<String, Color> _dominantColorCache = <String, Color>{};
  Color? _dominantColor;

  @override
  void initState() {
    super.initState();
    _loadDominantColor();
  }

  @override
  void didUpdateWidget(covariant MediaArtworkBackground oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.source != widget.source) {
      _dominantColor = null;
      _loadDominantColor();
    }
  }

  Future<void> _loadDominantColor() async {
    final String source = widget.source.trim();
    if (source.isEmpty) {
      return;
    }

    final Color? cached = _dominantColorCache[source];
    if (cached != null) {
      if (mounted) {
        setState(() => _dominantColor = cached);
      }
      return;
    }

    final Color? color = await _resolveDominantColor(source);
    if (color == null) {
      return;
    }

    _dominantColorCache[source] = color;
    if (mounted && widget.source.trim() == source) {
      setState(() => _dominantColor = color);
    }
  }

  @override
  Widget build(BuildContext context) {
    final Color color = _dominantColor ?? Colors.black;
    final HSLColor hslColor = HSLColor.fromColor(color);
    final Color gradientStart = hslColor
        .withLightness((hslColor.lightness * 0.75).clamp(0.18, 0.48))
        .toColor();
    final Color gradientEnd = hslColor
        .withLightness((hslColor.lightness * 0.45).clamp(0.10, 0.30))
        .toColor();

    return ClipRRect(
      borderRadius: widget.borderRadius,
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
                  source: widget.source,
                  fit: BoxFit.cover,
                  fallback: const SizedBox.shrink(),
                ),
              ),
            ),
          ),
          DecoratedBox(
            decoration: BoxDecoration(
              gradient: LinearGradient(
                begin: Alignment.topCenter,
                end: Alignment.bottomCenter,
                colors: <Color>[
                  gradientStart.withValues(alpha: 0.7),
                  gradientEnd.withValues(alpha: 0.7),
                ],
              ),
            ),
          ),
          widget.child,
        ],
      ),
    );
  }

  // Extracts a stable dominant color from the cover image.
  static Future<Color?> _resolveDominantColor(String source) async {
    final Uint8List? bytes = await _loadBytes(source);
    if (bytes == null || bytes.isEmpty) {
      return null;
    }

    final img.Image? image = img.decodeImage(bytes);
    if (image == null || image.width <= 0 || image.height <= 0) {
      return null;
    }

    final Map<int, _ColorBucket> buckets = <int, _ColorBucket>{};
    final int step = (image.width * image.height / 4096).sqrtCeil();

    for (int y = 0; y < image.height; y += step) {
      for (int x = 0; x < image.width; x += step) {
        final img.Pixel pixel = image.getPixel(x, y);
        final int alpha = pixel.a.round();
        final int red = pixel.r.round();
        final int green = pixel.g.round();
        final int blue = pixel.b.round();
        if (_shouldIgnoreColor(red, green, blue, alpha)) {
          continue;
        }

        final int key =
            ((red ~/ 32) << 10) | ((green ~/ 32) << 5) | (blue ~/ 32);
        final _ColorBucket bucket = buckets.putIfAbsent(key, _ColorBucket.new);
        bucket.red += red;
        bucket.green += green;
        bucket.blue += blue;
        bucket.count += 1;
      }
    }

    if (buckets.isEmpty) {
      return null;
    }

    final _ColorBucket dominant = buckets.values.reduce(
      (_ColorBucket current, _ColorBucket candidate) =>
          candidate.count > current.count ? candidate : current,
    );

    return Color.fromARGB(
      255,
      dominant.red ~/ dominant.count,
      dominant.green ~/ dominant.count,
      dominant.blue ~/ dominant.count,
    );
  }

  // Supports the same image source types as MediaImage.
  static Future<Uint8List?> _loadBytes(String source) async {
    final Uint8List? dataBytes = _decodeDataUri(source);
    if (dataBytes != null) {
      return dataBytes;
    }

    final Uri? uri = Uri.tryParse(source);
    if (uri == null || (uri.scheme != 'http' && uri.scheme != 'https')) {
      return null;
    }

    try {
      final http.Response response = await http.get(uri);
      if (response.statusCode < 200 || response.statusCode >= 300) {
        return null;
      }
      return response.bodyBytes;
    } catch (_) {
      return null;
    }
  }

  static Uint8List? _decodeDataUri(String value) {
    final int separator = value.indexOf(',');
    if (separator < 0) {
      return null;
    }

    final String header = value.substring(0, separator).toLowerCase();
    if (!header.startsWith('data:image/') || !header.contains(';base64')) {
      return null;
    }

    try {
      return base64Decode(value.substring(separator + 1));
    } on FormatException {
      return null;
    }
  }

  // Ignores neutral extremes so artwork color is not washed out.
  static bool _shouldIgnoreColor(int red, int green, int blue, int alpha) {
    if (alpha < 40) {
      return true;
    }
    if (red < 30 && green < 30 && blue < 30) {
      return true;
    }
    return red > 235 && green > 235 && blue > 235;
  }
}

class _ColorBucket {
  int red = 0;
  int green = 0;
  int blue = 0;
  int count = 0;
}

extension on double {
  int sqrtCeil() {
    if (this <= 1) {
      return 1;
    }
    int value = 1;
    while (value * value < this) {
      value++;
    }
    return value;
  }
}

/// Displays either a remote image URL or an inline data URI.
class MediaImage extends StatelessWidget {
  static const int _dataUriCacheLimit = 8;
  static final Map<String, Uint8List> _dataUriCache = <String, Uint8List>{};

  const MediaImage({
    super.key,
    required this.source,
    required this.fit,
    required this.fallback,
  });

  final String source;
  final BoxFit fit;
  final Widget fallback;

  Uint8List? _decodeDataUri(String value) {
    final Uint8List? cached = _dataUriCache[value];
    if (cached != null) {
      return cached;
    }

    final int separator = value.indexOf(',');
    if (separator < 0) {
      return null;
    }

    final String header = value.substring(0, separator).toLowerCase();
    if (!header.startsWith('data:image/') || !header.contains(';base64')) {
      return null;
    }

    try {
      final Uint8List decoded = base64Decode(value.substring(separator + 1));
      if (decoded.isEmpty) {
        return null;
      }

      if (_dataUriCache.length >= _dataUriCacheLimit) {
        _dataUriCache.remove(_dataUriCache.keys.first);
      }
      _dataUriCache[value] = decoded;
      return decoded;
    } on FormatException {
      return null;
    }
  }

  @override
  Widget build(BuildContext context) {
    final String trimmedSource = source.trim();
    final Uint8List? bytes = _decodeDataUri(trimmedSource);
    if (bytes != null && bytes.isNotEmpty) {
      return Image.memory(
        bytes,
        fit: fit,
        gaplessPlayback: true,
        errorBuilder: (_, _, _) => fallback,
      );
    }

    if (trimmedSource.isEmpty ||
        trimmedSource.toLowerCase().startsWith('data:')) {
      return fallback;
    }

    return Image.network(
      trimmedSource,
      fit: fit,
      gaplessPlayback: true,
      errorBuilder: (_, _, _) => fallback,
    );
  }
}
