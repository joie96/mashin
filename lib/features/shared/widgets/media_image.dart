import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';

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
