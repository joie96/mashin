import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mashin_flutter/features/core/android_bridge.dart';

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
    'ListenBrainz credentials are acknowledged by the Android service',
    () async {
      MethodCall? receivedCall;
      messenger.setMockMethodCallHandler(channel, (call) async {
        receivedCall = call;
        return null;
      });
      await AndroidBridge().execute(
        AndroidCoreCommand.setListenBrainzCredentials,
        const <String, Object?>{'username': 'user', 'token': 'secret'},
      );
      expect(receivedCall?.arguments, <String, Object?>{
        'command': 'setListenBrainzCredentials',
        'arguments': <String, Object?>{'username': 'user', 'token': 'secret'},
      });
    },
  );

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
