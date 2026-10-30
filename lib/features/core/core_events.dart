import 'dart:async';

import 'package:flutter/foundation.dart';

import 'core_bridge.dart';
import 'core_models.dart';

/// Projects immutable core events into the Flutter media-session render state.
class MashinCoreBridgeEvents extends ChangeNotifier {
  MashinCoreBridgeEvents({required this.coreBridge});

  final MashinCoreBridgeApi coreBridge;
  StreamSubscription<Queue>? _queueSubscription;
  StreamSubscription<PlayerState>? _playerStateSubscription;
  MediaSessionState _mediaSessionState = const MediaSessionState();
  bool _isStarted = false;
  bool _isDisposed = false;

  bool get isStarted => _isStarted;
  MediaSessionState get mediaSessionState => _mediaSessionState;

  Future<void> start() async {
    if (_isStarted) {
      return;
    }

    _queueSubscription = coreBridge.queueChanges.listen(_handleQueueChanged);
    _playerStateSubscription = coreBridge.playerStateChanges.listen(
      _handlePlayerStateChanged,
    );
    _isStarted = true;
    await Future.wait(<Future<void>>[
      coreBridge.refreshQueue(),
      coreBridge.refreshPlayerState(),
    ]);
  }

  // A new login can restart subscriptions without retaining the previous account's state.
  Future<void> reset() async {
    _isStarted = false;
    final queueSubscription = _queueSubscription;
    final playerStateSubscription = _playerStateSubscription;
    _queueSubscription = null;
    _playerStateSubscription = null;
    await queueSubscription?.cancel();
    await playerStateSubscription?.cancel();
    _mediaSessionState = const MediaSessionState();
    _notifyListeners();
  }

  void _handleQueueChanged(Queue queue) {
    _mediaSessionState = MediaSessionState(
      queue: queue,
      playerState: _mediaSessionState.playerState,
    );
    _notifyListeners();
  }

  void _handlePlayerStateChanged(PlayerState playerState) {
    _mediaSessionState = MediaSessionState(
      queue: _mediaSessionState.queue,
      playerState: playerState,
    );
    _notifyListeners();
  }

  void _notifyListeners() {
    if (!_isDisposed) {
      notifyListeners();
    }
  }

  @override
  void dispose() {
    _isDisposed = true;
    unawaited(_queueSubscription?.cancel());
    unawaited(_playerStateSubscription?.cancel());
    _queueSubscription = null;
    _playerStateSubscription = null;
    super.dispose();
  }
}
