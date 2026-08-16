import '../../data/music_assistant/ma_models.dart';
import 'playback_models.dart';


// Interface for players
abstract class PlaybackPlayer {
  PlaybackOutputMode get outputMode;

  Stream<PlaybackState> get states;

  Stream<PlaybackQueueSnapshot> get queueStates;

  Future<void> activate({String? targetPlayerId});

  Future<void> deactivate();

  Future<void> connect();

  Future<void> terminate();

  Future<void> transportPlay();

  Future<void> transportPause();

  Future<void> transportNext();

  Future<void> transportPrevious();

  Future<void> transportSeek(double seconds);

  Future<void> setVolume(int volume);

  Future<void> setMuted(bool muted);

  Future<void> playMedia(
    List<MediaItem> mediaItems, {
    PlaybackQueueInsertOption option = PlaybackQueueInsertOption.replace,
    bool radioMode = false,
    dynamic startItem,
    String sortBy = 'original',
  });

  Future<void> clearQueue({bool skipStop = false});

  Future<void> playQueueIndex(int index);

  Future<void> moveQueueItem(String queueItemId, {int posShift = 0});

  Future<void> deleteQueueItem(String queueItemId);

  Future<void> setShuffle(bool enabled);

  Future<void> setRepeatMode(String repeatMode);

  Future<void> setDontStopTheMusic(bool enabled);

  Future<PlaybackState> getState();

  Future<PlaybackQueueSnapshot> getQueueState();
}