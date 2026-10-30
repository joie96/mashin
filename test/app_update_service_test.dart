import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:mashin/app/updates/update_channel.dart';
import 'package:mashin/app/updates/app_update_service.dart';
import 'package:mashin/features/core/core_bridge.dart';
import 'package:pub_semver/pub_semver.dart';

Map<String, dynamic> release(
  String version, {
  String target = 'windows-x64.msi',
  bool isDraft = false,
  bool hasChecksum = true,
}) {
  final name = 'Mashin-$version-$target';
  return {
    'tag_name': 'v$version',
    'draft': isDraft,
    'prerelease': version.contains('-'),
    'body': "## What's Changed\n* A fix",
    'assets': [
      for (final suffix in ['', if (hasChecksum) '.sha256'])
        {
          'name': '$name$suffix',
          'size': 3,
          'browser_download_url':
              'https://github.com/joie96/mashin/releases/download/v$version/$name$suffix',
        },
    ],
  };
}

void main() {
  group('Rust-owned channel preferences', () {
    test('derives initial defaults from the installed version', () {
      for (final entry in {
        '1.0.0': UpdateChannel.stable,
        '1.0.0-beta.1': UpdateChannel.beta,
        '1.0.0-dev.1': UpdateChannel.dev,
        '1.0.0-rc.1': UpdateChannel.stable,
      }.entries) {
        expect(
          UpdateChannel.fromVersion(Version.parse(entry.key)),
          entry.value,
        );
      }
    });

    test('loads and saves exclusively through the core bridge', () async {
      final bridge = _UpdateChannelBridge();
      final service = _InstalledUpdateService(coreBridge: bridge);
      expect(await service.getUpdateChannel(), UpdateChannel.stable);
      expect(bridge.defaultChannel, UpdateChannel.stable);
      await service.saveUpdateChannel(UpdateChannel.beta);
      expect(bridge.channel, UpdateChannel.beta);
      expect(
        await _InstalledUpdateService(coreBridge: bridge).getUpdateChannel(),
        UpdateChannel.beta,
      );
    });

    test('Rust persistence errors propagate without a Dart fallback', () async {
      final bridge = _UpdateChannelBridge()..shouldFail = true;
      final service = _InstalledUpdateService(coreBridge: bridge);
      await expectLater(service.getUpdateChannel(), throwsStateError);
      await expectLater(
        service.saveUpdateChannel(UpdateChannel.beta),
        throwsStateError,
      );
      expect(bridge.channel, isNull);
    });

    test(
      'startup and manual services share the persisted channel',
      () async {
        final target = UpdateTarget.current!;
        final releases = [
          release('1.0.0', target: target.assetSuffix),
          release('1.1.0-beta.1', target: target.assetSuffix),
          release('1.2.0-dev.1', target: target.assetSuffix),
        ];
        final bridge = _UpdateChannelBridge();
        AppUpdateService service() => _InstalledUpdateService(
          coreBridge: bridge,
          createClient: () =>
              MockClient((_) async => http.Response(jsonEncode(releases), 200)),
        );
        final startup = service();
        final manual = service();
        await startup.saveUpdateChannel(UpdateChannel.beta);
        expect(await manual.getUpdateChannel(), UpdateChannel.beta);
        expect(
          (await startup.checkForUpdate())?.version.toString(),
          '1.1.0-beta.1',
        );
        expect(
          (await manual.checkForUpdate())?.version.toString(),
          '1.1.0-beta.1',
        );
        await manual.saveUpdateChannel(UpdateChannel.stable);
        expect((await startup.checkForUpdate())?.version.toString(), '1.0.0');
        expect((await service().checkForUpdate())?.version.toString(), '1.0.0');
      },
      skip: UpdateTarget.current == null,
    );
  });

  Future<AppUpdate?> find(
    List<Map<String, dynamic>> releases, {
    String current = '0.0.1-dev.5',
    UpdateTarget target = UpdateTarget.windowsX64,
    UpdateChannel? channel,
  }) =>
      AppUpdateService(
        createClient: () =>
            MockClient((_) async => http.Response(jsonEncode(releases), 200)),
      ).findUpdate(
        currentVersion: Version.parse(current),
        target: target,
        channel: channel ?? UpdateChannel.fromVersion(Version.parse(current)),
      );

  test(
    'filters releases by selected channel independently of installed version',
    () async {
      final releases = [
        release('1.0.0'),
        release('1.1.0-beta.1'),
        release('1.2.0-dev.1'),
        release('2.0.0-alpha.1'),
        release('2.0.0-rc.1'),
      ];
      for (final entry in {
        UpdateChannel.stable: '1.0.0',
        UpdateChannel.beta: '1.1.0-beta.1',
        UpdateChannel.dev: '1.2.0-dev.1',
      }.entries) {
        expect(
          (await find(
            releases,
            current: '0.9.0',
            channel: entry.key,
          ))?.version.toString(),
          entry.value,
        );
      }
      expect(
        await find([release('1.1.0-dev.1')], current: '1.0.0-beta.2'),
        isNull,
      );
      expect(
        await find(
          [release('1.0.0')],
          current: '1.1.0-dev.1',
          channel: UpdateChannel.stable,
        ),
        isNull,
      );
    },
  );

  test(
    'selects semantic newest version rather than GitHub order or text order',
    () async {
      final update = await find([
        release('0.0.1-dev.9'),
        release('0.0.1-dev.10'),
      ]);
      expect(update?.version.toString(), '0.0.1-dev.10');
      expect(update?.notes, "## What's Changed\n* A fix");
    },
  );

  test(
    'does not offer same, older, draft, wrong architecture or incomplete releases',
    () async {
      expect(
        await find([
          release('0.0.1-dev.5'),
          release('0.0.1-dev.4'),
          release('0.0.1-dev.6', isDraft: true),
          release('0.0.1-dev.7', target: 'windows-arm64.msi'),
          release('0.0.1-dev.8', hasChecksum: false),
        ]),
        isNull,
      );
    },
  );

  test(
    'stable installs ignore prereleases; prereleases can move to stable',
    () async {
      expect(await find([release('0.0.2-beta.1')], current: '0.0.1'), isNull);
      expect((await find([release('0.0.1')]))?.version.toString(), '0.0.1');
    },
  );

  test('selects exact Android and Windows ARM64 assets', () async {
    for (final target in [
      UpdateTarget.androidArm64,
      UpdateTarget.windowsArm64,
    ]) {
      final update = await find([
        release('0.0.1-dev.6', target: target.assetSuffix),
      ], target: target);
      expect(update?.fileName, 'Mashin-0.0.1-dev.6-${target.assetSuffix}');
    }
  });

  test('matches installer channel order dev then beta then stable', () async {
    expect(
      (await find([release('0.0.1-beta.1')]))?.version.toString(),
      '0.0.1-beta.1',
    );
    expect(
      await find([release('0.0.1-dev.20')], current: '0.0.1-beta.1'),
      isNull,
    );
    expect(
      (await find([
        release('0.0.1-beta.2'),
        release('0.0.1'),
        release('0.0.1-dev.30'),
      ]))?.version.toString(),
      '0.0.1',
    );
  });

  test('rejects installer URLs outside the release repository', () async {
    final data = release('0.0.1-dev.6');
    (data['assets'] as List).first['browser_download_url'] =
        'https://example.com/installer.msi';
    expect(await find([data]), isNull);
  });

  test('GitHub failure is surfaced to the startup caller', () async {
    final service = AppUpdateService(
      createClient: () => MockClient((_) async => http.Response('', 403)),
    );
    await expectLater(
      service.findUpdate(
        currentVersion: Version.parse('0.0.1'),
        target: UpdateTarget.windowsX64,
        channel: UpdateChannel.stable,
      ),
      throwsA(isA<HttpException>()),
    );
  });

  group('download', () {
    late Directory temporary;
    late AppUpdate update;
    setUp(() async {
      temporary = await Directory.systemTemp.createTemp('mashin-update-test-');
      update = (await find([release('0.0.1-dev.6')]))!;
    });
    tearDown(() async => temporary.delete(recursive: true));

    AppUpdateService service({
      String contents = 'abc',
      String? checksum,
    }) => AppUpdateService(
      downloadDirectory: () async => temporary,
      createClient: () => MockClient(
        (request) async => http.Response(
          request.url.path.endsWith('.sha256')
              ? '${checksum ?? sha256.convert(utf8.encode('abc'))}  ${update.fileName}\n'
              : contents,
          200,
        ),
      ),
    );

    test('streams, verifies and renames a complete installer', () async {
      final progress = <double>[];
      final file = await service().download(update, onProgress: progress.add);
      expect(await file.readAsString(), 'abc');
      expect(file.path.endsWith('.msi'), isTrue);
      expect(progress.last, 1);
    });

    test(
      'rejects corrupt and truncated downloads and removes partial files',
      () async {
        for (final contents in ['bad', 'ab', 'abcd']) {
          await expectLater(
            service(contents: contents).download(update, onProgress: (_) {}),
            throwsA(isA<FormatException>()),
          );
          expect(
            await Directory('${temporary.path}/mashin-updates').list().toList(),
            isEmpty,
          );
        }
      },
    );

    test(
      'cancel prevents installer handoff and removes the download',
      () async {
        final downloader = service();
        await expectLater(
          downloader.download(
            update,
            onProgress: (_) => downloader.cancelDownload(),
          ),
          throwsA(isA<UpdateCancelled>()),
        );
        expect(
          await Directory('${temporary.path}/mashin-updates').list().toList(),
          isEmpty,
        );
      },
    );
  });
}

class _InstalledUpdateService extends AppUpdateService {
  _InstalledUpdateService({required super.coreBridge, super.createClient});

  @override
  Future<Version> getInstalledVersion() async => Version.parse('0.9.0');
}

class _UpdateChannelBridge implements MashinCoreBridgeApi {
  UpdateChannel? channel;
  UpdateChannel? defaultChannel;
  bool shouldFail = false;

  @override
  Future<UpdateChannel> getUpdateChannel({
    required UpdateChannel defaultChannel,
  }) async {
    if (shouldFail) throw StateError('Rust configuration unavailable');
    this.defaultChannel = defaultChannel;
    return channel ??= defaultChannel;
  }

  @override
  Future<void> saveUpdateChannel(UpdateChannel channel) async {
    if (shouldFail) throw StateError('Rust configuration unavailable');
    this.channel = channel;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
