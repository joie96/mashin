import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

class AppConfig {
  const AppConfig._();

  static const String defaultMusicAssistantUrl = 'http://192.168.1.2:8095';
  static const String sendspinClientId = 'mashin_flutter';
  static const String sendspinClientName = 'Mashin Flutter';
  static const int sendspinBufferCapacityBytes = 52428800;
  static const String sendspinPreferredCodec = 'pcm';
  static const int sendspinInitialVolume = 50;
  static const bool sendspinInitialMuted = false;

  // Helper to determine if the app is running on a mobile platform (Android, iOS, Fuchsia) or not.
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

  // Helper to derive the Sendspin WebSocket URL from the Music Assistant base URL.
  static String deriveSendspinUrl(String musicAssistantBaseUrl) {
    final fallback = Uri.parse(defaultMusicAssistantUrl);
    final source = Uri.tryParse(musicAssistantBaseUrl.trim());

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
}
