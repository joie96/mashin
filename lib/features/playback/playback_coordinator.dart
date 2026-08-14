import 'dart:async';

import 'package:flutter/foundation.dart';

import 'playback_backend.dart';
import 'playback_models.dart';

class PlaybackCoordinator extends ChangeNotifier {
  PlaybackCoordinator({
    required List<PlaybackBackend> backends,
    this.initialOutputMode = PlaybackOutputMode.localSendspin,
  }) : _backends = <PlaybackOutputMode, PlaybackBackend>{
          for (final backend in backends) backend.outputMode: backend,
        };

  final Map<PlaybackOutputMode, PlaybackBackend> _backends;
  final PlaybackOutputMode initialOutputMode;

  PlaybackBackend? _activeBackend;
  StreamSubscription<PlaybackState>? _stateSubscription;
  PlaybackOutputMode? _outputMode;
  PlaybackState _state = const PlaybackState();
  bool _initialized = false;

  bool get isInitialized => _initialized;

  PlaybackOutputMode? get outputMode => _outputMode;

  PlaybackState get state => _state;

  Future<void> initialize() async {
    if (_initialized) {
      return;
    }

    final preferredMode = _backends.containsKey(initialOutputMode)
        ? initialOutputMode
        : (_backends.keys.isNotEmpty ? _backends.keys.first : null);

    if (preferredMode == null) {
      throw StateError('Kein PlaybackBackend registriert.');
    }

    await switchOutputMode(preferredMode);
    await _activeBackend!.connect();
    _state = await _activeBackend!.getState();
    _initialized = true;
    notifyListeners();
  }

  Future<void> switchOutputMode(
    PlaybackOutputMode mode, {
    String? targetPlayerId,
  }) async {
    final nextBackend = _backends[mode];
    if (nextBackend == null) {
      throw StateError('Kein Backend fuer den Modus $mode vorhanden.');
    }

    final hadPreviousBackend = _activeBackend != null;
    final backendChanged = !identical(nextBackend, _activeBackend);

    if (hadPreviousBackend && backendChanged) {
      await _activeBackend!.deactivate();
    }

    if (!hadPreviousBackend || backendChanged) {
      await nextBackend.activate(targetPlayerId: targetPlayerId);
    }

    _activeBackend = nextBackend;
    _outputMode = mode;

    await _bindStateStream(nextBackend);
    _state = await nextBackend.getState();
    notifyListeners();
  }

  Future<void> togglePlayPause() async {
    await _route((PlaybackBackend backend) async {
      final current = _state.playbackState;
      if (current == PlaybackStateType.playing) {
        await backend.transportPause();
      } else {
        await backend.transportPlay();
      }
    });
  }

  Future<void> next() => _route((PlaybackBackend backend) => backend.transportNext());

  Future<void> previous() => _route((PlaybackBackend backend) => backend.transportPrevious());

  Future<void> seek(double seconds) =>
      _route((PlaybackBackend backend) => backend.transportSeek(seconds));

  Future<void> setVolume(int volume) =>
      _route((PlaybackBackend backend) => backend.setVolume(volume));

  Future<void> setMuted(bool muted) =>
      _route((PlaybackBackend backend) => backend.setMuted(muted));

  Future<void> shutdown() async {
    await _stateSubscription?.cancel();
    _stateSubscription = null;

    final distinctBackends = _backends.values.toSet();
    for (final backend in distinctBackends) {
      await backend.terminate();
    }

    _activeBackend = null;
    _outputMode = null;
    _initialized = false;
    _state = const PlaybackState();
    notifyListeners();
  }

  Future<void> _bindStateStream(PlaybackBackend backend) async {
    await _stateSubscription?.cancel();
    _stateSubscription = backend.states.listen((PlaybackState next) {
      _state = next;
      notifyListeners();
    });
  }

  Future<void> _route(Future<void> Function(PlaybackBackend backend) action) async {
    final backend = _activeBackend;
    if (backend == null) {
      throw StateError('PlaybackCoordinator ist nicht initialisiert.');
    }

    await action(backend);
    _state = await backend.getState();
    notifyListeners();
  }

  @override
  void dispose() {
    unawaited(_stateSubscription?.cancel());
    _stateSubscription = null;
    super.dispose();
  }
}
