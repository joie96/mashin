import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/features/core/core_models.dart';

void main() {
  tearDown(() => MediaItemImage.configureProxyBaseUrl(null));

  test('resolves a relative generic provider icon against the MA URL', () {
    MediaItemImage.configureProxyBaseUrl('http://music-assistant.local:8095');
    ProviderManifestRegistry.registerOne(
      ProviderManifest.fromJson(<String, dynamic>{
        'domain': 'new-provider-icon',
        'icon': 'providers/new-provider-icon/icon.png',
      }),
    );

    expect(
      ProviderManifestRegistry.iconSvgForProvider('new-provider-icon--user'),
      'http://music-assistant.local:8095/providers/new-provider-icon/icon.png',
    );
  });

  test('prefers icon_svg when a manifest provides both icon formats', () {
    ProviderManifestRegistry.registerOne(
      ProviderManifest.fromJson(<String, dynamic>{
        'domain': 'provider-icon-priority',
        'icon': 'providers/provider-icon-priority/icon.png',
        'icon_svg': '<svg viewBox="0 0 1 1"></svg>',
      }),
    );

    expect(
      ProviderManifestRegistry.iconSvgForProvider('provider-icon-priority'),
      '<svg viewBox="0 0 1 1"></svg>',
    );
  });

  test('prefers icon data loaded from the provider icon endpoint', () {
    ProviderManifestRegistry.registerOne(
      ProviderManifest.fromJson(<String, dynamic>{
        'domain': 'provider-icon-endpoint',
        'icon_svg': '<svg viewBox="0 0 1 1"></svg>',
        'icon_images': <String>['default'],
      }),
    );
    ProviderManifestRegistry.registerIconData(
      'provider-icon-endpoint',
      'data:image/png;base64,aWNvbg==',
    );

    expect(
      ProviderManifestRegistry.iconSvgForProvider(
        'provider-icon-endpoint--user',
      ),
      'data:image/png;base64,aWNvbg==',
    );
  });
}