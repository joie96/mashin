import 'dart:async';
import 'dart:convert';

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
  String? _requestedMediaPresentation;
  final ChangeNotifier _playbackProgressChanges = ChangeNotifier();
  final ChangeNotifier _queueChanges = ChangeNotifier();
  final ChangeNotifier _currentMediaChanges = ChangeNotifier();

  bool get isStarted => _isStarted;
  MediaSessionState get mediaSessionState => _mediaSessionState;
  Listenable get playbackProgressChanges => _playbackProgressChanges;
  Listenable get queueChanges => _queueChanges;
  Listenable get currentMediaChanges => _currentMediaChanges;

  // Subscription lifecycle and account-state reset.
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
    await Future.wait<void>([
      if (queueSubscription != null) queueSubscription.cancel(),
      if (playerStateSubscription != null) playerStateSubscription.cancel(),
    ]);
    if (_isDisposed) return;
    _mediaSessionState = const MediaSessionState();
    _requestedMediaPresentation = null;
    _playbackProgressChanges.notifyListeners();
    _queueChanges.notifyListeners();
    _currentMediaChanges.notifyListeners();
    _notifyListeners();
  }

  @override
  void dispose() {
    _isDisposed = true;
    unawaited(_queueSubscription?.cancel());
    unawaited(_playerStateSubscription?.cancel());
    _queueSubscription = null;
    _playerStateSubscription = null;
    _playbackProgressChanges.dispose();
    _queueChanges.dispose();
    _currentMediaChanges.dispose();
    super.dispose();
  }

  // Snapshots advance before narrowly scoped render notifications are delivered.
  void _handleQueueChanged(Queue queue) {
    if (_isDisposed) return;
    final previousMedia = _mediaSessionState.displayedMedia;
    _mediaSessionState = MediaSessionState(
      queue: queue,
      playerState: _mediaSessionState.playerState,
    );
    _queueChanges.notifyListeners();
    if (!_hasSameMediaIdentity(
      previousMedia,
      _mediaSessionState.displayedMedia,
    )) {
      _currentMediaChanges.notifyListeners();
    }
    _notifyListeners();
  }

  void _handlePlayerStateChanged(PlayerState playerState) {
    if (_isDisposed) return;
    final previousState = _mediaSessionState.playerState;
    final previousMedia = _mediaSessionState.displayedMedia;
    final requestedMedia = playerState.requestedMedia;
    final requestedPresentation = requestedMedia == null
        ? null
        : jsonEncode(createMusicAssistantMediaItemPayload(requestedMedia));
    final hasPresentationChanged =
        (
          previousState.activeMode,
          previousState.connectionState,
          previousState.playbackState,
          previousState.currentQueueItemId,
          previousState.currentIndex,
          previousState.durationMillis,
          previousState.volume,
          previousState.muted,
          _requestedMediaPresentation,
        ) !=
        (
          playerState.activeMode,
          playerState.connectionState,
          playerState.playbackState,
          playerState.currentQueueItemId,
          playerState.currentIndex,
          playerState.durationMillis,
          playerState.volume,
          playerState.muted,
          requestedPresentation,
        );
    final hasProgressChanged =
        (
          previousState.positionMillis,
          previousState.bufferedPositionMillis,
          previousState.durationMillis,
        ) !=
        (
          playerState.positionMillis,
          playerState.bufferedPositionMillis,
          playerState.durationMillis,
        );
    _requestedMediaPresentation = requestedPresentation;
    _mediaSessionState = MediaSessionState(
      queue: _mediaSessionState.queue,
      playerState: playerState,
    );
    if (!_hasSameMediaIdentity(
      previousMedia,
      _mediaSessionState.displayedMedia,
    )) {
      _currentMediaChanges.notifyListeners();
    }
    if (hasPresentationChanged) _notifyListeners();
    if (hasProgressChanged) _playbackProgressChanges.notifyListeners();
  }

  // Media identity includes exact provider mappings used by playback row matching.
  bool _hasSameMediaIdentity(MediaItem? previous, MediaItem? current) {
    if (identical(previous, current)) return true;
    if (previous == null || current == null) return false;
    return (
              previous.mediaType,
              previous.provider,
              previous.itemId,
              previous.uri,
            ) ==
            (
              current.mediaType,
              current.provider,
              current.itemId,
              current.uri,
            ) &&
        setEquals(
          previous.providerMappings
              .map((mapping) => (mapping.providerInstance, mapping.itemId))
              .toSet(),
          current.providerMappings
              .map((mapping) => (mapping.providerInstance, mapping.itemId))
              .toSet(),
        );
  }

  void _notifyListeners() {
    if (!_isDisposed) {
      notifyListeners();
    }
  }
}
