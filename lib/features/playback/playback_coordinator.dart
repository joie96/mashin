import 'dart:async';

import 'package:flutter/foundation.dart';

import '../../app/logging/app_logger.dart';
import '../../data/music_assistant/ma_models.dart';
import 'playback_models.dart';
import 'playback_player.dart';

class PlaybackCoordinator {
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
  StreamSubscription<PlaybackState>? _playbackStateSubscription;
  StreamSubscription<PlaybackQueue>? _queueSubscription;
  PlaybackOutputMode? _outputMode;
  PlaybackState _playbackState = const PlaybackState();
  PlaybackQueue _queueState = const PlaybackQueue();
  bool _initialized = false;
  final _PlaybackChangeNotifier _playbackStateChanges =
      _PlaybackChangeNotifier();
  final _PlaybackChangeNotifier _playbackQueueChanges =
      _PlaybackChangeNotifier();
  final AppLogger _logger = AppLogger('PlaybackCoordinator');

  void _updatePlaybackState(
    PlaybackState nextPlaybackState, {
    required String source,
  }) {
    final previousType = _playbackState.playbackState;
    _playbackState = nextPlaybackState;
    final nextType = _playbackState.playbackState;

    final bool isSendspinEventSource = source == 'stream/player.playbackStates';
    if (isSendspinEventSource) {
      return;
    }

    if (previousType != nextType) {
      _logger.debug(
        'PlaybackState changed. Source=$source, PreviousPlaybackState=${previousType.name}, NextPlaybackState=${nextType.name}',
      );
    }
  }

  bool get isInitialized => _initialized;

  PlaybackOutputMode? get outputMode => _outputMode;

  PlaybackState get playbackState => _playbackState;

  PlaybackQueue get playbackQueue => _queueState;

  Listenable get playbackStateChanges => _playbackStateChanges;

  Listenable get playbackQueueChanges => _playbackQueueChanges;

  List<QueueItem> get queueItems =>
      List<QueueItem>.unmodifiable(_queueState.items);

  int? get currentQueueIndex => _queueState.currentIndex;

  bool? get shuffleEnabled =>
      _queueState.shuffleEnabled ?? _playbackState.shuffleEnabled;

  RepeatMode? get repeatMode =>
      _queueState.repeatMode ?? _playbackState.repeatMode;

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
    _updatePlaybackState(
      await _activePlayer!.getPlaybackState(),
      source: 'initialize/getPlaybackState',
    );
    _queueState = await _activePlayer!.getQueueState();
    _initialized = true;
    _logger.info(
      'initialize completed. OutputMode=${_outputMode?.name ?? 'null'}, QueueItemCount=${_queueState.items.length}',
    );
    _notifyPlaybackStateChanged();
    _notifyPlaybackQueueChanged();
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
      _logger.debug(
        'activating target player for output mode switch. RequestedMode=${mode.name}',
      );
      await nextPlayer.activate(targetPlayerId: targetPlayerId);
    }

    _activePlayer = nextPlayer;
    _outputMode = mode;

    await _bindStreams(nextPlayer);
    _updatePlaybackState(
      await nextPlayer.getPlaybackState(),
      source: 'switchOutputMode/getPlaybackState',
    );
    _queueState = await nextPlayer.getQueueState();
    _logger.info(
      'output mode switch completed. OutputMode=${_outputMode?.name ?? 'null'}, QueueItemCount=${_queueState.items.length}',
    );
    _notifyPlaybackStateChanged();
    _notifyPlaybackQueueChanged();
  }

  Future<void> togglePlayPause() async {
    await _route((PlaybackPlayer player) async {
      final current = _playbackState.playbackState;
      if (current == PlaybackStateType.playing) {
        await player.transportPause();
      } else {
        await player.transportPlay();
      }
    });
  }

  Future<void> next() =>
      _route((PlaybackPlayer player) => player.transportNext());

  Future<void> previous() =>
      _route((PlaybackPlayer player) => player.transportPrevious());

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
    return _route(
      (PlaybackPlayer player) => player.playMediaReplaceNext(mediaItems),
    );
  }

  Future<void> playMediaLast(List<MediaItem> mediaItems) {
    return _route((PlaybackPlayer player) => player.playMediaLast(mediaItems));
  }

  Future<void> playMediaRadioNext(List<MediaItem> mediaItems) {
    return _route(
      (PlaybackPlayer player) => player.playMediaRadioNext(mediaItems),
    );
  }

  Future<void> shufflePlayMedia(List<MediaItem> mediaItems) {
    return _route(
      (PlaybackPlayer player) => player.shufflePlayMedia(mediaItems),
    );
  }

  Future<void> clearQueue({bool skipStop = false}) =>
      _route((PlaybackPlayer player) => player.clearQueue(skipStop: skipStop));

  Future<void> playQueueIndex(int index) =>
      _route((PlaybackPlayer player) => player.playQueueIndex(index));

  Future<void> moveQueueItem(String queueItemId, {int posShift = 0}) => _route(
    (PlaybackPlayer player) =>
        player.moveQueueItem(queueItemId, posShift: posShift),
  );

  Future<void> deleteQueueItem(String queueItemId) =>
      _route((PlaybackPlayer player) => player.deleteQueueItem(queueItemId));

  Future<void> toggleShuffle([bool? currentShuffleEnabled]) {
    final bool next =
        !(currentShuffleEnabled ??
            _queueState.shuffleEnabled ??
            _playbackState.shuffleEnabled ??
            false);
    return _route((PlaybackPlayer player) => player.setShuffle(next));
  }

  Future<void> toggleRepeatMode([RepeatMode? currentRepeatMode]) {
    final current =
        currentRepeatMode ??
        _queueState.repeatMode ??
        _playbackState.repeatMode ??
        RepeatMode.off;
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
    await _playbackStateSubscription?.cancel();
    await _queueSubscription?.cancel();
    _playbackStateSubscription = null;
    _queueSubscription = null;

    final distinctPlayers = _players.values.toSet();
    for (final player in distinctPlayers) {
      await player.terminate();
    }

    _activePlayer = null;
    _outputMode = null;
    _initialized = false;
    _playbackState = const PlaybackState();
    _queueState = const PlaybackQueue();
    _notifyPlaybackStateChanged();
    _notifyPlaybackQueueChanged();
  }

  Future<void> _bindStreams(PlaybackPlayer player) async {
    await _playbackStateSubscription?.cancel();
    await _queueSubscription?.cancel();
    _playbackStateSubscription = player.playbackStates.listen((
      PlaybackState nextPlaybackState,
    ) {
      if (!identical(player, _activePlayer)) {
        return;
      }
      _updatePlaybackState(
        nextPlaybackState,
        source: 'stream/player.playbackStates',
      );
      _notifyPlaybackStateChanged();
    });
    _queueSubscription = player.queueStates.listen((PlaybackQueue next) {
      if (!identical(player, _activePlayer)) {
        return;
      }
      _queueState = next;
      _updatePlaybackState(
        _playbackState.copyWith(
          shuffleEnabled: next.shuffleEnabled ?? _playbackState.shuffleEnabled,
          repeatMode: next.repeatMode ?? _playbackState.repeatMode,
        ),
        source: 'stream/player.queueStates',
      );
      _notifyPlaybackStateChanged();
      _notifyPlaybackQueueChanged();
    });
  }

  Future<void> _route(
    Future<void> Function(PlaybackPlayer player) action,
  ) async {
    final player = _activePlayer;
    if (player == null) {
      throw StateError('PlaybackCoordinator ist nicht initialisiert.');
    }

    _logger.debug(
      'routing command to active player. OutputMode=${_outputMode?.name ?? 'null'}',
    );

    await action(player);
  }

  void dispose() {
    unawaited(_playbackStateSubscription?.cancel());
    _playbackStateSubscription = null;
    _playbackStateChanges.dispose();
    _playbackQueueChanges.dispose();
  }

  void _notifyPlaybackStateChanged() {
    _playbackStateChanges.emit();
  }

  void _notifyPlaybackQueueChanged() {
    _playbackQueueChanges.emit();
  }
}

class _PlaybackChangeNotifier extends ChangeNotifier {
  void emit() {
    notifyListeners();
  }
}
