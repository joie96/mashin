import 'dart:async';

import 'package:flutter/foundation.dart';

import 'playback_models.dart';
import 'sendspin/sendspin_player.dart';

class PlaybackCoordinator extends ChangeNotifier {
  PlaybackCoordinator({
    required List<SendspinPlayer> players,
    this.initialOutputMode = PlaybackOutputMode.localSendspin,
  }) : _players = <PlaybackOutputMode, SendspinPlayer>{
          for (final player in players) player.outputMode: player,
        };

  final Map<PlaybackOutputMode, SendspinPlayer> _players;
  final PlaybackOutputMode initialOutputMode;

  SendspinPlayer? _activePlayer;
  StreamSubscription<PlaybackState>? _stateSubscription;
  StreamSubscription<PlaybackQueueSnapshot>? _queueSubscription;
  PlaybackOutputMode? _outputMode;
  PlaybackState _state = const PlaybackState();
  PlaybackQueueSnapshot _queueState = const PlaybackQueueSnapshot();
  bool _initialized = false;

  bool get isInitialized => _initialized;

  PlaybackOutputMode? get outputMode => _outputMode;

  PlaybackState get state => _state;

  List<Map<String, dynamic>> get queueItems => List<Map<String, dynamic>>.unmodifiable(_queueState.items);

  int? get currentQueueIndex => _queueState.currentIndex;

  bool? get shuffleEnabled => _queueState.shuffleEnabled ?? _state.shuffleEnabled;

  String? get repeatMode => _queueState.repeatMode ?? _state.repeatMode;

  bool? get dontStopTheMusicEnabled => _queueState.dontStopTheMusicEnabled;

  Future<void> initialize() async {
    if (_initialized) {
      return;
    }

    final preferredMode = _players.containsKey(initialOutputMode)
        ? initialOutputMode
      : (_players.keys.isNotEmpty ? _players.keys.first : null);

    if (preferredMode == null) {
      throw StateError('Kein Playback-Player registriert.');
    }

    await switchOutputMode(preferredMode);
    await _activePlayer!.connect();
    _state = await _activePlayer!.getState();
    _queueState = await _activePlayer!.getQueueState();
    _initialized = true;
    notifyListeners();
  }

  Future<void> switchOutputMode(
    PlaybackOutputMode mode, {
    String? targetPlayerId,
  }) async {
    final nextPlayer = _players[mode];
    if (nextPlayer == null) {
      throw StateError('Kein Player fuer den Modus $mode vorhanden.');
    }

    final hadPreviousPlayer = _activePlayer != null;
    final playerChanged = !identical(nextPlayer, _activePlayer);

    if (hadPreviousPlayer && playerChanged) {
      await _activePlayer!.deactivate();
    }

    if (!hadPreviousPlayer || playerChanged) {
      await nextPlayer.activate(targetPlayerId: targetPlayerId);
    }

    _activePlayer = nextPlayer;
    _outputMode = mode;

    await _bindStreams(nextPlayer);
    _state = await nextPlayer.getState();
    _queueState = await nextPlayer.getQueueState();
    notifyListeners();
  }

  Future<void> togglePlayPause() async {
    await _route((SendspinPlayer player) async {
      final current = _state.playbackState;
      if (current == PlaybackStateType.playing) {
        await player.transportPause();
      } else {
        await player.transportPlay();
      }
    });
  }

  Future<void> next() => _route((SendspinPlayer player) => player.transportNext());

  Future<void> previous() => _route((SendspinPlayer player) => player.transportPrevious());

  Future<void> seek(double seconds) =>
      _route((SendspinPlayer player) => player.transportSeek(seconds));

  Future<void> setVolume(int volume) =>
      _route((SendspinPlayer player) => player.setVolume(volume));

  Future<void> setMuted(bool muted) =>
      _route((SendspinPlayer player) => player.setMuted(muted));

  Future<void> playMedia(
    List<Map<String, dynamic>> mediaItems, {
    PlaybackQueueInsertOption option = PlaybackQueueInsertOption.replace,
    bool radioMode = false,
    dynamic startItem,
    String sortBy = 'original',
  }) => _route((SendspinPlayer player) => player.playMedia(mediaItems, option: option, radioMode: radioMode, startItem: startItem, sortBy: sortBy));

  Future<void> playMediaNext(List<Map<String, dynamic>> mediaItems) {
    return playMedia(mediaItems, option: PlaybackQueueInsertOption.next);
  }

  Future<void> playMediaReplaceNext(List<Map<String, dynamic>> mediaItems) {
    return playMedia(mediaItems, option: PlaybackQueueInsertOption.replaceNext);
  }

  Future<void> playMediaLast(List<Map<String, dynamic>> mediaItems) {
    return playMedia(mediaItems, option: PlaybackQueueInsertOption.last);
  }

  Future<void> playMediaRadioNext(List<Map<String, dynamic>> mediaItems) {
    return playMedia(
      mediaItems,
      option: PlaybackQueueInsertOption.next,
      radioMode: true,
    );
  }

  Future<void> shufflePlayMedia(List<Map<String, dynamic>> mediaItems) {
    return playMedia(mediaItems, sortBy: 'random');
  }

  Future<void> clearQueue({bool skipStop = false}) =>
      _route((SendspinPlayer player) => player.clearQueue(skipStop: skipStop));

  Future<void> playQueueIndex(int index) =>
      _route((SendspinPlayer player) => player.playQueueIndex(index));

  Future<void> moveQueueItem(String queueItemId, {int posShift = 0}) =>
      _route((SendspinPlayer player) => player.moveQueueItem(queueItemId, posShift: posShift));

  Future<void> deleteQueueItem(String queueItemId) =>
      _route((SendspinPlayer player) => player.deleteQueueItem(queueItemId));

  Future<void> toggleShuffle([bool? currentShuffleEnabled]) {
    final bool next = !(currentShuffleEnabled ?? _queueState.shuffleEnabled ?? _state.shuffleEnabled ?? false);
    return _route((SendspinPlayer player) => player.setShuffle(next));
  }

  Future<void> toggleRepeatMode([String? currentRepeatMode]) {
    final normalized = (currentRepeatMode ?? _queueState.repeatMode ?? _state.repeatMode ?? '').trim().toLowerCase();
    final String next = switch (normalized) {
      'off' || 'none' || '' => 'all',
      'all' => 'one',
      _ => 'off',
    };

    return _route((SendspinPlayer player) => player.setRepeatMode(next));
  }

  Future<void> setDontStopTheMusic(bool enabled) =>
      _route((SendspinPlayer player) => player.setDontStopTheMusic(enabled));

  Future<void> shutdown() async {
    await _stateSubscription?.cancel();
    await _queueSubscription?.cancel();
    _stateSubscription = null;
    _queueSubscription = null;

    final distinctPlayers = _players.values.toSet();
    for (final player in distinctPlayers) {
      await player.terminate();
    }

    _activePlayer = null;
    _outputMode = null;
    _initialized = false;
    _state = const PlaybackState();
    _queueState = const PlaybackQueueSnapshot();
    notifyListeners();
  }

  Future<void> _bindStreams(SendspinPlayer player) async {
    await _stateSubscription?.cancel();
    await _queueSubscription?.cancel();
    _stateSubscription = player.states.listen((PlaybackState next) {
      _state = next;
      notifyListeners();
    });
    _queueSubscription = player.queueStates.listen((PlaybackQueueSnapshot next) {
      _queueState = next;
      _state = _state.copyWith(
        shuffleEnabled: next.shuffleEnabled ?? _state.shuffleEnabled,
        repeatMode: next.repeatMode ?? _state.repeatMode,
      );
      notifyListeners();
    });
  }

  Future<void> _route(Future<void> Function(SendspinPlayer player) action) async {
    final player = _activePlayer;
    if (player == null) {
      throw StateError('PlaybackCoordinator ist nicht initialisiert.');
    }

    await action(player);
    _state = await player.getState();
    _queueState = await player.getQueueState();
    _state = _state.copyWith(
      shuffleEnabled: _queueState.shuffleEnabled ?? _state.shuffleEnabled,
      repeatMode: _queueState.repeatMode ?? _state.repeatMode,
    );
    notifyListeners();
  }

  @override
  void dispose() {
    unawaited(_stateSubscription?.cancel());
    _stateSubscription = null;
    super.dispose();
  }
}
