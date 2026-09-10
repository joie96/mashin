import 'dart:io' show Platform;

import 'package:shared_preferences/shared_preferences.dart';
import 'package:flutter/widgets.dart';

class AppConfig {
  const AppConfig._();

  static const String defaultMusicAssistantUrl = 'http://192.168.1.2:8095';
  static const String defaultSendspinUrl = 'ws://192.168.1.2:8927/sendspin';

  static const int singleSendspinBufferCapacityBytes = 50 * 1024 * 1024;
  static const int cacheCapacityBytes = 250 * 1024 * 1024;
  static const String sendspinPreferredCodec = 'pcm';
  static const int defaultLocalVolume = 100;

  static const String _musicAssistantUrlKey = 'ma.base_url';
  static const String _sendspinUrlKey = 'ma.sendspin_url';
  static const String _sendspinPreferredCodecKey =
      'ma.sendspin_preferred_codec';
  static const String _localVolumeKey = 'ma.local_volume';

  static bool isMobile() {
    // Temporary: decide mobile/desktop by viewport width.
    const double mobileWidthBreakpoint = 900;
    final views = WidgetsBinding.instance.platformDispatcher.views;
    if (views.isEmpty) {
      return false;
    }

    final view = views.first;
    final logicalWidth = view.physicalSize.width / view.devicePixelRatio;
    return logicalWidth < mobileWidthBreakpoint;

    // Previous platform-based detection kept for later reactivation.
    // if (kIsWeb) {
    //   return false;
    // }

    // switch (defaultTargetPlatform) {
    //   case TargetPlatform.android:
    //   case TargetPlatform.iOS:
    //   case TargetPlatform.fuchsia:
    //     return true;
    //   case TargetPlatform.linux:
    //   case TargetPlatform.macOS:
    //   case TargetPlatform.windows:
    //     return false;
    // }
  }

  static Future<String> getSendspinUrl() async {
    final prefs = await SharedPreferences.getInstance();
    final configured = prefs.getString(_sendspinUrlKey)?.trim() ?? '';
    if (configured.isNotEmpty) {
      return configured;
    }

    final musicAssistantUrl =
        prefs.getString(_musicAssistantUrlKey)?.trim() ??
        defaultMusicAssistantUrl;
    final derived = _deriveSendspinUrlFromMusicAssistantUrl(musicAssistantUrl);
    await prefs.setString(_sendspinUrlKey, derived);
    return derived;
  }

  static Future<String> getSendspinPreferredCodec() async {
    final prefs = await SharedPreferences.getInstance();
    final configured =
        prefs.getString(_sendspinPreferredCodecKey)?.trim().toLowerCase() ?? '';
    return _isSupportedSendspinCodec(configured)
        ? configured
        : sendspinPreferredCodec;
  }

  static Future<void> setSendspinPreferredCodec(String codec) async {
    final normalized = codec.trim().toLowerCase();
    if (!_isSupportedSendspinCodec(normalized)) {
      throw ArgumentError.value(codec, 'codec', 'Unsupported Sendspin codec.');
    }

    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_sendspinPreferredCodecKey, normalized);
  }

  static Future<int> getLocalVolume() async {
    if (isMobile()) {
      return defaultLocalVolume;
    }

    final prefs = await SharedPreferences.getInstance();
    return (prefs.getInt(_localVolumeKey) ?? defaultLocalVolume).clamp(
      0,
      100,
    );
  }

  static Future<void> setLocalVolume(int volume) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setInt(_localVolumeKey, volume.clamp(0, 100));
  }

  static bool _isSupportedSendspinCodec(String codec) {
    return codec == 'opus' || codec == 'flac' || codec == 'pcm';
  }

  static String _deriveSendspinUrlFromMusicAssistantUrl(
    String musicAssistantUrl,
  ) {
    final fallback = Uri.parse(defaultMusicAssistantUrl);
    final source = Uri.tryParse(musicAssistantUrl.trim());

    final host = (source != null && source.host.trim().isNotEmpty)
        ? source.host.trim()
        : fallback.host;
    final scheme = (source?.scheme.toLowerCase() == 'https') ? 'wss' : 'ws';

    return Uri(
      scheme: scheme,
      host: host,
      port: 8927,
      path: '/sendspin',
    ).toString();
  }

  // Device and App Identification
  static String get deviceName {
    final host = Platform.localHostname.trim();
    if (host.isEmpty) {
      return 'Device';
    }
    return host;
  }

  static String get appName => 'Mashin ($deviceName)';

  static String get appId {
    final normalizedDevice = _normalizeForId(deviceName, preserveHyphen: true);
    final suffix = normalizedDevice.isEmpty ? 'device' : normalizedDevice;
    return 'mashin-$suffix';
  }

  static String deriveMaAppId(String sourceAppId) {
    final normalized = _normalizeForId(sourceAppId, preserveHyphen: false);
    if (normalized.isEmpty) {
      return '';
    }

    if (normalized.toLowerCase().startsWith('up')) {
      return normalized.toLowerCase();
    }

    return 'up$normalized';
  }

  static String get maAppId => deriveMaAppId(appId);

  static String sendspinDownloadSlotId(int slot) {
    if (slot != 1 && slot != 2) {
      throw ArgumentError.value(slot, 'slot', 'Expected download slot 1 or 2.');
    }
    return '$appId-$slot';
  }

  static String maDownloadSlotPlayerId(int slot) =>
      deriveMaAppId(sendspinDownloadSlotId(slot));

  static String _normalizeForId(String raw, {required bool preserveHyphen}) {
    final lower = raw.trim().toLowerCase();
    if (lower.isEmpty) {
      return '';
    }

    final pattern = preserveHyphen
        ? RegExp(r'[^a-z0-9-]')
        : RegExp(r'[^a-z0-9]');
    return lower.replaceAll(pattern, '');
  }
}
