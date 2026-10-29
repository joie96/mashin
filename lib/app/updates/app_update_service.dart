import 'dart:async';
import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:http/http.dart' as http;
import 'package:package_info_plus/package_info_plus.dart';
import 'package:path_provider/path_provider.dart';
import 'package:pub_semver/pub_semver.dart';

enum UpdateTarget {
  windowsX64('windows-x64.msi'),
  windowsArm64('windows-arm64.msi'),
  androidArm64('android-arm64.apk');

  const UpdateTarget(this.assetSuffix);

  final String assetSuffix;

  static UpdateTarget? get current => switch (Abi.current()) {
    Abi.windowsX64 => windowsX64,
    Abi.windowsArm64 => windowsArm64,
    Abi.androidArm64 => androidArm64,
    _ => null,
  };
}

class AppUpdate {
  const AppUpdate({
    required this.version,
    required this.notes,
    required this.fileName,
    required this.downloadUrl,
    required this.checksumUrl,
    required this.size,
  });

  final Version version;
  final String notes;
  final String fileName;
  final Uri downloadUrl;
  final Uri checksumUrl;
  final int size;
}

class UpdateCancelled implements Exception {}

/// Owns release requests and at most one cancellable installer download.
class AppUpdateService {
  AppUpdateService({
    http.Client Function()? createClient,
    Future<Directory> Function()? downloadDirectory,
  }) : _createClient = createClient ?? http.Client.new,
       _downloadDirectory = downloadDirectory ?? getTemporaryDirectory;

  final http.Client Function() _createClient;
  final Future<Directory> Function() _downloadDirectory;
  http.Client? _downloadClient;
  bool _isCancelled = false;

  // Release discovery and version selection.
  Future<AppUpdate?> checkAtStartup() async {
    final target = UpdateTarget.current;
    if (target == null) return null;
    String version;
    if (Platform.isWindows) {
      final metadata = File.fromUri(
        File(
          Platform.resolvedExecutable,
        ).parent.uri.resolve('data/release.json'),
      );
      if (await metadata.exists()) {
        final data =
            jsonDecode(await metadata.readAsString()) as Map<String, dynamic>;
        version = data['version'] as String;
      } else {
        version = (await PackageInfo.fromPlatform()).version;
      }
    } else {
      version = (await PackageInfo.fromPlatform()).version;
    }
    return findUpdate(currentVersion: Version.parse(version), target: target);
  }

  Future<AppUpdate?> findUpdate({
    required Version currentVersion,
    required UpdateTarget target,
  }) async {
    final client = _createClient();
    try {
      AppUpdate? newest;
      for (var page = 1; page <= 10; page++) {
        final response = await client
            .get(
              Uri.https('api.github.com', '/repos/joie96/mashin/releases', {
                'per_page': '100',
                'page': '$page',
              }),
              headers: {
                'Accept': 'application/vnd.github+json',
                'X-GitHub-Api-Version': '2022-11-28',
              },
            )
            .timeout(const Duration(seconds: 15));
        _requireSuccess(response.statusCode);
        final releases = jsonDecode(response.body) as List<dynamic>;
        for (final value in releases) {
          try {
            final release = value as Map<String, dynamic>;
            if (release['draft'] == true) continue;
            final tag = release['tag_name'] as String;
            final version = Version.parse(
              tag.startsWith('v') ? tag.substring(1) : tag,
            );
            if (_compareVersions(version, currentVersion) <= 0 ||
                (currentVersion.isPreRelease == false &&
                    (version.isPreRelease || release['prerelease'] == true)) ||
                (newest != null &&
                    _compareVersions(version, newest.version) <= 0)) {
              continue;
            }
            final name = 'Mashin-$version-${target.assetSuffix}';
            final assets = (release['assets'] as List<dynamic>)
                .cast<Map<String, dynamic>>();
            final installers = assets.where((asset) => asset['name'] == name);
            final checksums = assets.where(
              (asset) => asset['name'] == '$name.sha256',
            );
            if (installers.length != 1 || checksums.length != 1) continue;
            final installer = installers.single;
            final downloadUrl = Uri.parse(
              installer['browser_download_url'] as String,
            );
            final checksumUrl = Uri.parse(
              checksums.single['browser_download_url'] as String,
            );
            if (!_isReleaseAsset(downloadUrl, tag, name) ||
                !_isReleaseAsset(checksumUrl, tag, '$name.sha256')) {
              continue;
            }
            final size = installer['size'] as int;
            if (size <= 0) continue;
            newest = AppUpdate(
              version: version,
              notes: release['body'] as String? ?? '',
              fileName: name,
              downloadUrl: downloadUrl,
              checksumUrl: checksumUrl,
              size: size,
            );
          } on FormatException {
            continue;
          } on TypeError {
            continue;
          }
        }
        if (releases.length < 100) break;
      }
      return newest;
    } finally {
      client.close();
    }
  }

  // Installer download and integrity verification.
  Future<File> download(
    AppUpdate update, {
    required void Function(double progress) onProgress,
  }) async {
    if (_downloadClient != null) {
      throw StateError('Ein Update-Download ist bereits aktiv.');
    }
    final client = _createClient();
    _downloadClient = client;
    _isCancelled = false;
    Directory? staging;
    try {
      final cache = await _downloadDirectory();
      final updates = await Directory(
        '${cache.path}/mashin-updates',
      ).create(recursive: true);
      staging = await updates.createTemp('installer-');
      final checksum = await client
          .get(update.checksumUrl)
          .timeout(const Duration(seconds: 20));
      _requireSuccess(checksum.statusCode);
      final checksumMatch = RegExp(
        r'^([a-fA-F0-9]{64})\s+\*?([^\r\n]+)\s*$',
      ).firstMatch(checksum.body.trim());
      if (checksumMatch == null || checksumMatch[2] != update.fileName) {
        throw const FormatException('Ungültige SHA-256-Prüfsumme.');
      }
      final response = await client
          .send(http.Request('GET', update.downloadUrl))
          .timeout(const Duration(seconds: 30));
      _requireSuccess(response.statusCode);
      final partial = File('${staging.path}/${update.fileName}.part');
      final sink = partial.openWrite();
      var received = 0;
      try {
        await for (final chunk in response.stream.timeout(
          const Duration(seconds: 30),
        )) {
          if (_isCancelled) throw UpdateCancelled();
          received += chunk.length;
          if (received > update.size) {
            throw const FormatException('Ungültige Downloadgröße.');
          }
          sink.add(chunk);
          onProgress(received / update.size);
        }
      } finally {
        await sink.close();
      }
      if (_isCancelled) throw UpdateCancelled();
      if (received != update.size) {
        throw const FormatException('Der Download ist unvollständig.');
      }
      final digest = await sha256.bind(partial.openRead()).first;
      if (digest.toString() != checksumMatch[1]!.toLowerCase()) {
        throw const FormatException('Die SHA-256-Prüfung ist fehlgeschlagen.');
      }
      if (_isCancelled) throw UpdateCancelled();
      return await partial.rename('${staging.path}/${update.fileName}');
    } catch (error) {
      if (staging != null && await staging.exists()) {
        await staging.delete(recursive: true);
      }
      if (_isCancelled) throw UpdateCancelled();
      rethrow;
    } finally {
      client.close();
      _downloadClient = null;
    }
  }

  void cancelDownload() {
    _isCancelled = true;
    _downloadClient?.close();
  }

  // Match the installer version ordering in build_release.ps1: dev, beta, stable.
  static int _compareVersions(Version left, Version right) {
    final baseOrder = Version(
      left.major,
      left.minor,
      left.patch,
    ).compareTo(Version(right.major, right.minor, right.patch));
    if (baseOrder != 0) return baseOrder;
    int channelOrder(Version version) =>
        switch (version.preRelease.firstOrNull) {
          'dev' => 0,
          'beta' => 1,
          null => 2,
          _ => -1,
        };
    final channelComparison = channelOrder(left).compareTo(channelOrder(right));
    return channelComparison == 0 ? left.compareTo(right) : channelComparison;
  }

  // GitHub boundary validation.
  static bool _isReleaseAsset(Uri uri, String tag, String name) =>
      uri.scheme == 'https' &&
      uri.host == 'github.com' &&
      uri.userInfo.isEmpty &&
      !uri.hasPort &&
      uri.query.isEmpty &&
      uri.fragment.isEmpty &&
      uri.path == '/joie96/mashin/releases/download/$tag/$name';

  static void _requireSuccess(int status) {
    if (status != 200) {
      throw HttpException('GitHub-Anfrage fehlgeschlagen (HTTP $status).');
    }
  }
}
