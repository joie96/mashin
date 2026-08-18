import 'dart:async';

import 'package:flutter/foundation.dart';

import '../../app/logging/app_logger.dart';
import '../../data/music_assistant/ma_models.dart';
import 'playback_models.dart';
import 'playback_player.dart';

class PlaybackCoordinator extends ChangeNotifier {
  PlaybackCoordinator({
    required List<PlaybackPlayer> players,
    this.initialOutputMode = PlaybackOutputMode.localSendspin,
  }) : _players = <PlaybackOutputMode, PlaybackPlayer>{
          for (final player in players) player.outputMode: player,
        };


  // Fields
  final Map<PlaybackOutputMode, PlaybackPlayer> _players;
  final PlaybackOutputMode initialOutputMode;

  PlaybackPlayer? _activePlayer;
  StreamSubscription<PlaybackState>? _stateSubscription;
  StreamSubscription<PlaybackQueueSnapshot>? _queueSubscription;
  PlaybackOutputMode? _outputMode;
  PlaybackState _state = const PlaybackState();
  PlaybackQueueSnapshot _queueState = const PlaybackQueueSnapshot();
  bool _initialized = false;
  final AppLogger _logger = AppLogger('PlaybackCoordinator');

  void _updateState(PlaybackState next, {required String source}) {
    final previousType = _state.playbackState;
    _state = next;
    final nextType = _state.playbackState;

    final bool isSendspinEventSource = source == 'stream/player.states';
    if (isSendspinEventSource) {
      return;
    }

    if (previousType != nextType) {
      _logger.debug(
        'state changed. Source=$source, PreviousState=${previousType.name}, NextState=${nextType.name}',
      );
    }
  }

  bool get isInitialized => _initialized;

  PlaybackOutputMode? get outputMode => _outputMode;

  PlaybackState get state => _state;

  List<QueueItem> get queueItems => List<QueueItem>.unmodifiable(_queueState.items);

  int? get currentQueueIndex => _queueState.currentIndex;

  bool? get shuffleEnabled => _queueState.shuffleEnabled ?? _state.shuffleEnabled;

  RepeatMode? get repeatMode => _queueState.repeatMode ?? _state.repeatMode;

  bool? get dontStopTheMusicEnabled => _queueState.dontStopTheMusicEnabled;

  // Initialization
  Future<void> initialize() async {
    if (_initialized) {
      _logger.debug(
        'initialize skipped because coordinator is already initialized. OutputMode=${_outputMode?.name ?? 'null'}',
      );
      return;
    }

    final preferredMode = _players.containsKey(initialOutputMode)
        ? initialOutputMode
      : (_players.keys.isNotEmpty ? _players.keys.first : null);

    if (preferredMode == null) {
      throw StateError('Kein Playback-Player registriert.');
    }

    _logger.info(
      'initialize requested. PreferredMode=${preferredMode.name}, AvailablePlayerCount=${_players.length}',
    );

    await switchOutputMode(preferredMode);
    await _activePlayer!.connect();
    _updateState(
      await _activePlayer!.getState(),
      source: 'initialize/getState',
    );
    _queueState = await _activePlayer!.getQueueState();
    _initialized = true;
    _logger.info(
      'initialize completed. OutputMode=${_outputMode?.name ?? 'null'}, QueueItemCount=${_queueState.items.length}',
    );
    notifyListeners();
  }

  // Playback Actions
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

    _logger.info(
      'output mode switch requested. CurrentMode=${_outputMode?.name ?? 'null'}, RequestedMode=${mode.name}, PlayerChanged=$playerChanged, HadPreviousPlayer=$hadPreviousPlayer, TargetPlayerId=${targetPlayerId ?? ''}',
    );

    if (hadPreviousPlayer && playerChanged) {
      _logger.debug(
        'deactivating previous player before output mode switch. PreviousMode=${_outputMode?.name ?? 'null'}',
      );
      await _activePlayer!.deactivate();
    }

    if (!hadPreviousPlayer || playerChanged) {
      _logger.debug('activating target player for output mode switch. RequestedMode=${mode.name}');
      await nextPlayer.activate(targetPlayerId: targetPlayerId);
    }

    _activePlayer = nextPlayer;
    _outputMode = mode;

    await _bindStreams(nextPlayer);
    _updateState(
      await nextPlayer.getState(),
      source: 'switchOutputMode/getState',
    );
    _queueState = await nextPlayer.getQueueState();
    _logger.info(
      'output mode switch completed. OutputMode=${_outputMode?.name ?? 'null'}, QueueItemCount=${_queueState.items.length}',
    );
    notifyListeners();
  }

  Future<void> togglePlayPause() async {
    await _route((PlaybackPlayer player) async {
      final current = _state.playbackState;
      if (current == PlaybackStateType.playing) {
        await player.transportPause();
      } else {
        await player.transportPlay();
      }
    });
  }

    Future<void> next() => _route((PlaybackPlayer player) => player.transportNext());

    Future<void> previous() => _route((PlaybackPlayer player) => player.transportPrevious());

  Future<void> seek(double seconds) =>
      _route((PlaybackPlayer player) => player.transportSeek(seconds));

  Future<void> setVolume(int volume) =>
      _route((PlaybackPlayer player) => player.setVolume(volume));

  Future<void> setMuted(bool muted) =>
      _route((PlaybackPlayer player) => player.setMuted(muted));


  // Media Actions
  Future<void> playMedia(List<MediaItem> mediaItems) =>
      _route((PlaybackPlayer player) => player.playMedia(mediaItems));

  Future<void> playMediaNext(List<MediaItem> mediaItems) {
    return _route((PlaybackPlayer player) => player.playMediaNext(mediaItems));
  }

  Future<void> playMediaReplaceNext(List<MediaItem> mediaItems) {
    return _route((PlaybackPlayer player) => player.playMediaReplaceNext(mediaItems));
  }

  Future<void> playMediaLast(List<MediaItem> mediaItems) {
    return _route((PlaybackPlayer player) => player.playMediaLast(mediaItems));
  }

  Future<void> playMediaRadioNext(List<MediaItem> mediaItems) {
    return _route((PlaybackPlayer player) => player.playMediaRadioNext(mediaItems));
  }

  Future<void> shufflePlayMedia(List<MediaItem> mediaItems) {
    return _route((PlaybackPlayer player) => player.shufflePlayMedia(mediaItems));
  }

  Future<void> clearQueue({bool skipStop = false}) =>
      _route((PlaybackPlayer player) => player.clearQueue(skipStop: skipStop));

  Future<void> playQueueIndex(int index) =>
      _route((PlaybackPlayer player) => player.playQueueIndex(index));

  Future<void> moveQueueItem(String queueItemId, {int posShift = 0}) =>
      _route((PlaybackPlayer player) => player.moveQueueItem(queueItemId, posShift: posShift));

  Future<void> deleteQueueItem(String queueItemId) =>
      _route((PlaybackPlayer player) => player.deleteQueueItem(queueItemId));

  Future<void> toggleShuffle([bool? currentShuffleEnabled]) {
    final bool next = !(currentShuffleEnabled ?? _queueState.shuffleEnabled ?? _state.shuffleEnabled ?? false);
    return _route((PlaybackPlayer player) => player.setShuffle(next));
  }

  Future<void> toggleRepeatMode([RepeatMode? currentRepeatMode]) {
    final current = currentRepeatMode ?? _queueState.repeatMode ?? _state.repeatMode ?? RepeatMode.off;
    final RepeatMode next = switch (current) {
      RepeatMode.off => RepeatMode.all,
      RepeatMode.all => RepeatMode.one,
      RepeatMode.one => RepeatMode.off,
    };

    return _route((PlaybackPlayer player) => player.setRepeatMode(next));
  }

  Future<void> setDontStopTheMusic(bool enabled) =>
      _route((PlaybackPlayer player) => player.setDontStopTheMusic(enabled));

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

  Future<void> _bindStreams(PlaybackPlayer player) async {
    await _stateSubscription?.cancel();
    await _queueSubscription?.cancel();
    _stateSubscription = player.states.listen((PlaybackState next) {
      if (!identical(player, _activePlayer)) {
        return;
      }
      _updateState(next, source: 'stream/player.states');
      notifyListeners();
    });
    _queueSubscription = player.queueStates.listen((PlaybackQueueSnapshot next) {
      if (!identical(player, _activePlayer)) {
        return;
      }
      _queueState = next;
      _updateState(
        _state.copyWith(
        shuffleEnabled: next.shuffleEnabled ?? _state.shuffleEnabled,
        repeatMode: next.repeatMode ?? _state.repeatMode,
        ),
        source: 'stream/player.queueStates',
      );
      notifyListeners();
    });
  }

  Future<void> _route(Future<void> Function(PlaybackPlayer player) action) async {
    final player = _activePlayer;
    if (player == null) {
      throw StateError('PlaybackCoordinator ist nicht initialisiert.');
    }

    _logger.debug('routing command to active player. OutputMode=${_outputMode?.name ?? 'null'}');

    await action(player);
    _updateState(
      await player.getState(),
      source: 'route/getState',
    );
    _queueState = await player.getQueueState();
    _updateState(
      _state.copyWith(
        shuffleEnabled: _queueState.shuffleEnabled ?? _state.shuffleEnabled,
        repeatMode: _queueState.repeatMode ?? _state.repeatMode,
      ),
      source: 'route/queueProjection',
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
