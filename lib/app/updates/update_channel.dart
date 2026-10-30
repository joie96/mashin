import 'package:pub_semver/pub_semver.dart';

enum UpdateChannel {
  stable,
  beta,
  dev;

  static UpdateChannel fromVersion(Version version) =>
      switch (version.preRelease.firstOrNull) {
        'dev' => dev,
        'beta' => beta,
        _ => stable,
      };

  bool allows(Version version, {required bool isPrerelease}) {
    if (!version.isPreRelease) return !isPrerelease;
    return switch (version.preRelease.firstOrNull) {
      'dev' => this == dev,
      'beta' => this == beta || this == dev,
      _ => false,
    };
  }
}
