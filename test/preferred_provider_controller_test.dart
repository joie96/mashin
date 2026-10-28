import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:mashin/app/session/preferred_provider_controller.dart';
import 'package:mashin/features/core/core_models.dart';

void main() {
  const preferred = PreferredProvider(
    providerInstance: 'tidal-instance',
    providerDomain: 'tidal',
  );

  test('preference contract uses named instance and domain fields', () {
    final decoded = PreferredProvider.fromJson(preferred.toJson());
    expect(decoded.providerInstance, 'tidal-instance');
    expect(decoded.providerDomain, 'tidal');
    final settings = SessionSettings.fromJson(<String, dynamic>{
      'preferredProvider': preferred.toJson(),
    });
    expect(settings.preferredProvider?.providerInstance, 'tidal-instance');
    expect(
      SessionSettings.fromJson(<String, dynamic>{}).preferredProvider,
      isNull,
    );
  });

  test('snapshot changes only after Rust persistence succeeds', () async {
    final controller = PreferredProviderController();
    final pending = Completer<void>();
    PreferredProvider? submitted;
    controller.initialize(
      preferredProvider: null,
      persist: (value) {
        submitted = value;
        return pending.future;
      },
    );
    final saving = controller.setPreferredProvider(preferred);
    expect(submitted, same(preferred));
    expect(controller.preferredProvider, isNull);
    pending.complete();
    await saving;
    expect(controller.preferredProvider, same(preferred));
    controller.initialize(preferredProvider: preferred, persist: (_) async {});
    await controller.setPreferredProvider(null);
    expect(controller.preferredProvider, isNull);
    controller.dispose();
  });

  test('failed writes preserve the current preference', () async {
    final controller = PreferredProviderController();
    controller.initialize(
      preferredProvider: preferred,
      persist: (_) async {
        throw StateError('storage unavailable');
      },
    );
    await expectLater(controller.setPreferredProvider(null), throwsStateError);
    expect(controller.preferredProvider, same(preferred));
    controller.reset();
    await expectLater(
      controller.setPreferredProvider(preferred),
      throwsStateError,
    );
    expect(controller.preferredProvider, isNull);
    controller.dispose();
  });

  test('completed writes cannot overwrite a newer session snapshot', () async {
    final controller = PreferredProviderController();
    final pending = Completer<void>();
    controller.initialize(
      preferredProvider: null,
      persist: (_) => pending.future,
    );
    final saving = controller.setPreferredProvider(preferred);
    controller.reset();
    controller.initialize(preferredProvider: null, persist: (_) async {});
    pending.complete();
    await saving;
    expect(controller.preferredProvider, isNull);
    controller.dispose();
  });
}
