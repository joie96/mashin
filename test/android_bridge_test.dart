import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/features/core/android_bridge.dart';
import 'package:mashin_flutter/features/core/core_models.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('mashin_flutter/playback');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  tearDown(() => messenger.setMockMethodCallHandler(channel, null));

  test('playback awaits the service acknowledgement', () async {
    final acknowledgement = Completer<Object?>();
    MethodCall? receivedCall;
    messenger.setMockMethodCallHandler(channel, (call) {
      receivedCall = call;
      return acknowledgement.future;
    });
    var hasCompleted = false;
    final completion = AndroidBridge()
        .execute(AndroidCoreCommand.transportPlay, const <String, Object?>{})
        .then((_) => hasCompleted = true);
    await Future<void>.delayed(Duration.zero);
    expect(hasCompleted, isFalse);
    expect(receivedCall?.method, 'execute');
    expect(receivedCall?.arguments, <String, Object?>{
      'command': 'transportPlay',
      'arguments': <String, Object?>{},
    });
    acknowledgement.complete(null);
    await completion;
    expect(hasCompleted, isTrue);
  });

  test('session restoration returns the service result', () async {
    messenger.setMockMethodCallHandler(channel, (_) async => true);
    expect(
      await AndroidBridge().execute(
        AndroidCoreCommand.restorePersistedSession,
        const <String, Object?>{},
      ),
      isTrue,
    );
  });

  test(
    'queue mutations forward deletion batches and MA move parameters',
    () async {
      final calls = <MethodCall>[];
      messenger.setMockMethodCallHandler(channel, (call) async {
        calls.add(call);
        return null;
      });
      final bridge = AndroidBridge();
      await bridge.execute(AndroidCoreCommand.moveQueueItem, const {
        'itemId': 'stable',
        'positionShift': 0,
      });
      await bridge.execute(AndroidCoreCommand.moveQueueItem, const {
        'itemId': 'stable',
        'positionShift': -2,
      });
      await bridge.execute(AndroidCoreCommand.moveQueueItemToEnd, const {
        'itemId': 'stable',
      });
      await bridge.execute(AndroidCoreCommand.deleteQueueItems, const {
        'itemIds': ['stable', 'second'],
      });
      expect(calls.map((call) => call.arguments), <Object?>[
        {
          'command': 'moveQueueItem',
          'arguments': {'itemId': 'stable', 'positionShift': 0},
        },
        {
          'command': 'moveQueueItem',
          'arguments': {'itemId': 'stable', 'positionShift': -2},
        },
        {
          'command': 'moveQueueItemToEnd',
          'arguments': {'itemId': 'stable'},
        },
        {
          'command': 'deleteQueueItems',
          'arguments': {
            'itemIds': ['stable', 'second'],
          },
        },
      ]);
      expect(
        AndroidCoreCommand.values.map((command) => command.name),
        isNot(
          anyOf(
            contains('insertQueueItems'),
            contains('appendQueueItems'),
            contains('moveQueueItems'),
            contains('deleteQueueItem'),
          ),
        ),
      );
    },
  );

  test('settings are submitted in one complete Android command', () async {
    MethodCall? receivedCall;
    messenger.setMockMethodCallHandler(channel, (call) async {
      receivedCall = call;
      return null;
    });
    const update = SessionSettingsUpdate(
      localCacheSizeBytes: 524288000,
      isDarkTheme: false,
      logLevel: LogLevelPreference.warn,
      shouldShowMultiDeviceButton: false,
    );
    await AndroidBridge().execute(
      AndroidCoreCommand.saveSessionSettings,
      update.toJson(),
    );
    expect(receivedCall?.arguments, <String, Object?>{
      'command': 'saveSessionSettings',
      'arguments': <String, Object?>{
        'localCacheSizeBytes': 524288000,
        'theme': 'light',
        'logLevel': 'warn',
        'shouldShowMultiDeviceButton': false,
      },
    });
  });

  test(
    'preferred provider forwards typed settings and automatic selection',
    () async {
      final calls = <MethodCall>[];
      messenger.setMockMethodCallHandler(channel, (call) async {
        calls.add(call);
        return null;
      });
      const preferred = PreferredProvider(
        providerInstance: 'tidal-instance',
        providerDomain: 'tidal',
      );
      for (final value in <PreferredProvider?>[preferred, null]) {
        await AndroidBridge().execute(AndroidCoreCommand.setPreferredProvider, {
          'preferredProvider': value?.toJson(),
        });
      }
      expect(calls.map((call) => call.arguments), <Object?>[
        {
          'command': 'setPreferredProvider',
          'arguments': {'preferredProvider': preferred.toJson()},
        },
        {
          'command': 'setPreferredProvider',
          'arguments': {'preferredProvider': null},
        },
      ]);
    },
  );

  test('settings commit is acknowledged by the Android service', () async {
    MethodCall? receivedCall;
    messenger.setMockMethodCallHandler(channel, (call) async {
      receivedCall = call;
      return null;
    });
    await AndroidBridge().execute(
      AndroidCoreCommand.saveSessionSettings,
      const SessionSettingsUpdate(
        localCacheSizeBytes: 262144000,
        isDarkTheme: true,
        logLevel: LogLevelPreference.info,
      ).toJson(),
    );
    expect(receivedCall?.arguments, <String, Object?>{
      'command': 'saveSessionSettings',
      'arguments': <String, Object?>{
        'localCacheSizeBytes': 262144000,
        'theme': 'dark',
        'logLevel': 'info',
        'shouldShowMultiDeviceButton': true,
      },
    });
  });

  test('service failure is propagated without fallback', () async {
    messenger.setMockMethodCallHandler(channel, (_) async {
      throw PlatformException(code: 'service_unavailable');
    });
    await expectLater(
      AndroidBridge().execute(AndroidCoreCommand.transportPlay, const {}),
      throwsA(isA<PlatformException>()),
    );
  });

  test('disposing disconnects the UI without a shutdown command', () async {
    final methods = <String>[];
    messenger.setMockMethodCallHandler(channel, (call) async {
      methods.add(call.method);
      return null;
    });
    await AndroidBridge().dispose();
    expect(methods, <String>['disconnect']);
  });

  test(
    'dispose tolerates an already detached engine, execute does not',
    () async {
      messenger.setMockMethodCallHandler(
        channel,
        (_) async => throw MissingPluginException(),
      );
      await AndroidBridge().dispose();
      await expectLater(
        AndroidBridge().execute(AndroidCoreCommand.transportPlay, const {}),
        throwsA(isA<MissingPluginException>()),
      );
    },
  );
}
