package com.example.mashin_flutter.media.session

import android.os.Looper
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.MoreExecutors
import org.json.JSONArray
import org.json.JSONObject

// Media3 projection of the canonical Rust queue; no independent queue writes.
@UnstableApi
internal class RustSessionPlayer(
    private val dispatchCommand: (PlaybackRequest) -> ListenableFuture<*>,
    private val startPlayback: (String) -> ListenableFuture<*>,
    private val resolveLibraryMediaItem: (String) -> JSONObject?,
) : SimpleBasePlayer(Looper.getMainLooper()) {
    private var snapshot = PlaybackSnapshot.empty()

    // Snapshot projection is confined to the Media3 player looper.
    fun updateState(nextSnapshot: PlaybackSnapshot) {
        snapshot = nextSnapshot
        invalidateState()
    }

    // Media3 commands are forwarded to the service's serialized Rust dispatcher.
    override fun handleSetMediaItems(
        mediaItems: List<MediaItem>,
        startIndex: Int,
        startPositionMs: Long,
    ): ListenableFuture<*> {
        val selectedItem = mediaItems.getOrNull(if (startIndex == C.INDEX_UNSET) 0 else startIndex)
        if (mediaItems.size == 1 && selectedItem?.mediaId?.let {
            it.startsWith("action:") || it.startsWith("artistRadio:") || it.startsWith("browse:track:")
        } == true) {
            val selection = startPlayback(selectedItem.mediaId)
            if (startPositionMs <= 0L) return selection
            return Futures.transformAsync(
                selection,
                { dispatchCommand(PlaybackRequest(PlaybackCommandKind.TransportSeek,
                    JSONObject().put("seconds", startPositionMs / 1_000.0).toString())) },
                MoreExecutors.directExecutor(),
            )
        }
        val selectedIndex = if (mediaItems.isEmpty()) null else if (startIndex == C.INDEX_UNSET) 0 else startIndex
        if (selectedIndex != null && selectedIndex !in mediaItems.indices) {
            return Futures.immediateFailedFuture<Any>(IndexOutOfBoundsException("unknown queue item index: $selectedIndex"))
        }
        val items = JSONArray()
        for (item in mediaItems) {
            val mediaItem = resolveMediaItem(item.mediaId)
                ?: return Futures.immediateFailedFuture<Any>(IllegalArgumentException("unknown queue media ID: ${item.mediaId}"))
            items.put(mediaItem)
        }
        var completion: ListenableFuture<*> = dispatchCommand(PlaybackRequest(
            PlaybackCommandKind.QueuePlayMedia,
            JSONObject().put("request", JSONObject()
                .put("mediaItems", items).put("option", "replace")
                .put("startItem", selectedIndex ?: JSONObject.NULL).put("shuffle", false)).toString(),
        ))
        if (selectedIndex != null && startPositionMs > 0L && startPositionMs != C.TIME_UNSET) {
            completion = Futures.transformAsync(completion, {
                dispatchCommand(PlaybackRequest(PlaybackCommandKind.TransportSeek,
                    JSONObject().put("seconds", startPositionMs / 1_000.0).toString()))
            }, MoreExecutors.directExecutor())
        }
        return completion
    }

    override fun handleAddMediaItems(index: Int, mediaItems: List<MediaItem>): ListenableFuture<*> {
        if (mediaItems.isEmpty()) return Futures.immediateVoidFuture()
        val option = when (index) {
            snapshot.mediaItems.size -> "add"
            snapshot.currentIndex + 1 -> "next"
            else -> return Futures.immediateFailedFuture<Any>(UnsupportedOperationException("MA supports adding next or at the end only"))
        }
        val items = JSONArray()
        for (item in mediaItems) {
            val mediaItem = resolveMediaItem(item.mediaId)
                ?: return Futures.immediateFailedFuture<Any>(IllegalArgumentException("unknown queue media ID: ${item.mediaId}"))
            items.put(mediaItem)
        }
        return dispatchCommand(PlaybackRequest(PlaybackCommandKind.QueuePlayMedia,
            JSONObject().put("request", JSONObject().put("mediaItems", items)
                .put("option", option).put("startItem", JSONObject.NULL).put("shuffle", false)).toString()))
    }

    override fun handleRemoveMediaItems(fromIndex: Int, toIndex: Int): ListenableFuture<*> {
        val itemIds = snapshot.mediaItems.subList(fromIndex, toIndex).map { it.mediaId }
        var completion: ListenableFuture<*> = Futures.immediateVoidFuture()
        for (itemId in itemIds) {
            completion = Futures.transformAsync(completion, {
                dispatchCommand(PlaybackRequest(PlaybackCommandKind.DeleteQueueItem,
                    JSONObject().put("itemId", itemId).toString()))
            }, MoreExecutors.directExecutor())
        }
        return completion
    }

    override fun handleMoveMediaItems(fromIndex: Int, toIndex: Int, newIndex: Int): ListenableFuture<*> {
        val positionShift = newIndex - fromIndex
        if (positionShift == 0 || fromIndex == toIndex) return Futures.immediateVoidFuture()
        val itemIds = snapshot.mediaItems.subList(fromIndex, toIndex).map { it.mediaId }
        val isMovingToEnd = newIndex == snapshot.mediaItems.size - itemIds.size
        val orderedIds = if (positionShift > 0 && !isMovingToEnd) itemIds.reversed() else itemIds
        var completion: ListenableFuture<*> = Futures.immediateVoidFuture()
        for (itemId in orderedIds) {
            completion = Futures.transformAsync(completion, {
                val arguments = JSONObject().put("itemId", itemId)
                val kind = if (isMovingToEnd) PlaybackCommandKind.MoveQueueItemToEnd else {
                    arguments.put("positionShift", positionShift)
                    PlaybackCommandKind.MoveQueueItem
                }
                dispatchCommand(PlaybackRequest(kind, arguments.toString()))
            }, MoreExecutors.directExecutor())
        }
        return completion
    }

    override fun handleReplaceMediaItems(fromIndex: Int, toIndex: Int, mediaItems: List<MediaItem>): ListenableFuture<*> {
        return Futures.immediateFailedFuture<Any>(UnsupportedOperationException("MA does not support replacing a queue range; set media items instead"))
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        if (playWhenReady && (snapshot.mediaItems.isEmpty() || snapshot.hasPlaybackIntent)) {
            return Futures.immediateVoidFuture()
        }
        return dispatchCommand(PlaybackRequest(if (playWhenReady) PlaybackCommandKind.TransportPlay else PlaybackCommandKind.TransportPause))
    }

    override fun handleStop(): ListenableFuture<*> = dispatchCommand(PlaybackRequest(PlaybackCommandKind.TransportStop))

    override fun handleSeek(
        mediaItemIndex: Int,
        positionMs: Long,
        seekCommand: Int,
    ): ListenableFuture<*> {
        if (mediaItemIndex == C.INDEX_UNSET) return Futures.immediateVoidFuture()
        if (seekCommand == Player.COMMAND_SEEK_TO_NEXT || seekCommand == Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM ||
            seekCommand == Player.COMMAND_SEEK_TO_PREVIOUS || seekCommand == Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM) {
            if (mediaItemIndex != snapshot.currentIndex) {
                val command = if (seekCommand == Player.COMMAND_SEEK_TO_NEXT ||
                    seekCommand == Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM) PlaybackCommandKind.TransportNext else PlaybackCommandKind.TransportPrevious
                return dispatchCommand(PlaybackRequest(command))
            }
            if (seekCommand == Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM ||
                seekCommand == Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM) return Futures.immediateVoidFuture()
        }
        if (mediaItemIndex != snapshot.currentIndex) {
            if (mediaItemIndex !in snapshot.mediaItems.indices) {
                return Futures.immediateFailedFuture<Any>(IndexOutOfBoundsException("unknown queue item index: $mediaItemIndex"))
            }
            val selection = dispatchCommand(PlaybackRequest(PlaybackCommandKind.SelectQueueItem,
                JSONObject().put("itemId", snapshot.mediaItems[mediaItemIndex].mediaId).toString()))
            if (positionMs <= 0L || positionMs == C.TIME_UNSET) return selection
            return Futures.transformAsync(
                selection,
                { dispatchCommand(PlaybackRequest(PlaybackCommandKind.TransportSeek,
                    JSONObject().put("seconds", positionMs / 1_000.0).toString())) },
                MoreExecutors.directExecutor(),
            )
        }
        return dispatchCommand(PlaybackRequest(PlaybackCommandKind.TransportSeek,
            JSONObject().put("seconds", (if (positionMs == C.TIME_UNSET) 0L else positionMs) / 1_000.0).toString()))
    }

    override fun handleSetRepeatMode(repeatMode: Int): ListenableFuture<*> {
        val value = when (repeatMode) {
            Player.REPEAT_MODE_ONE -> 1
            Player.REPEAT_MODE_ALL -> 2
            else -> 0
        }
        return dispatchCommand(PlaybackRequest(PlaybackCommandKind.TransportRepeatMode,
            JSONObject().put("mode", value).toString()))
    }

    override fun handleSetShuffleModeEnabled(shuffleModeEnabled: Boolean): ListenableFuture<*> {
        return dispatchCommand(PlaybackRequest(PlaybackCommandKind.TransportShuffle,
            JSONObject().put("enabled", shuffleModeEnabled).toString()))
    }

    override fun handleSetVolume(volume: Float, volumeOperationType: Int): ListenableFuture<*> =
        dispatchCommand(PlaybackRequest(PlaybackCommandKind.SetVolume,
            JSONObject().put("volume", (volume.coerceIn(0f, 1f) * 100).toInt()).toString()))

    // Canonical snapshot projection.
    override fun getState(): State {
        val commands = Player.Commands.Builder()
            .add(Player.COMMAND_GET_CURRENT_MEDIA_ITEM)
            .add(Player.COMMAND_GET_TIMELINE)
            .add(Player.COMMAND_GET_METADATA)
            .add(Player.COMMAND_GET_VOLUME)
            .add(Player.COMMAND_SET_MEDIA_ITEM)
            .add(Player.COMMAND_CHANGE_MEDIA_ITEMS)
            .add(Player.COMMAND_PLAY_PAUSE)
            .add(Player.COMMAND_STOP)
            .add(Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM)
            .add(Player.COMMAND_SEEK_TO_MEDIA_ITEM)
            .add(Player.COMMAND_SEEK_TO_NEXT)
            .add(Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM)
            .add(Player.COMMAND_SEEK_TO_PREVIOUS)
            .add(Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM)
            .add(Player.COMMAND_SET_REPEAT_MODE)
            .add(Player.COMMAND_SET_SHUFFLE_MODE)
            .add(Player.COMMAND_SET_VOLUME)
            .build()
        val builder = State.Builder()
            .setAvailableCommands(commands)
            .setPlayWhenReady(
                snapshot.hasPlaybackIntent,
                Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST,
            )
            .setPlaybackState(snapshot.playerState)
            .setIsLoading(snapshot.playerState == Player.STATE_BUFFERING && snapshot.mediaItems.isNotEmpty())
            .setContentPositionMs(snapshot.positionMillis)
            .setContentBufferedPositionMs(
                PositionSupplier.getConstant(snapshot.bufferedPositionMillis),
            )
            .setRepeatMode(snapshot.repeatMode)
            .setShuffleModeEnabled(snapshot.isShuffleEnabled)
            .setVolume(if (snapshot.isMuted) 0f else snapshot.volume / 100f)
            .setUnmuteVolume((snapshot.volume.takeIf { it > 0 } ?: 100) / 100f)

        if (snapshot.mediaItems.isNotEmpty()) {
            builder.setPlaylist(snapshot.mediaItems.mapIndexed { index, item ->
                val durationUs = if (index == snapshot.currentIndex) {
                    snapshot.durationMillis?.times(1_000L) ?: C.TIME_UNSET
                } else {
                    item.mediaMetadata.durationMs?.times(1_000L) ?: C.TIME_UNSET
                }
                MediaItemData.Builder(item.mediaId)
                    .setMediaItem(item)
                    .setMediaMetadata(item.mediaMetadata)
                    .setIsSeekable(durationUs != C.TIME_UNSET)
                    .setDurationUs(durationUs)
                    .setPeriods(listOf(PeriodData.Builder(item.mediaId).setDurationUs(durationUs).build()))
                    .build()
            })
            builder.setCurrentMediaItemIndex(snapshot.currentIndex)
        }
        return builder.build()
    }

    // Resolve existing queue identities from the snapshot, not the browse cache.
    private fun resolveMediaItem(mediaId: String): JSONObject? = snapshot.mediaItems
        .firstOrNull { it.mediaId == mediaId }?.mediaMetadata?.extras
        ?.getString(PlaybackSnapshotMapper.SERIALIZED_MEDIA_ITEM_KEY)?.let(::JSONObject)
        ?: resolveLibraryMediaItem(mediaId)
}

