package com.joie96.mashin.media.session

import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import java.util.concurrent.ExecutionException
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Assert.assertThrows
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
@LooperMode(LooperMode.Mode.PAUSED)
class RustSessionPlayerTest {
    private val requests = mutableListOf<PlaybackRequest>()
    private val starts = mutableListOf<String>()
    private var response: ListenableFuture<*> = Futures.immediateVoidFuture()
    private lateinit var player: RustSessionPlayer

    // The fixture exposes only the authoritative snapshot and serialized requests.
    @Before
    fun initialize() {
        player = RustSessionPlayer(
            { request -> requests.add(request); response },
            { mediaId -> starts.add(mediaId); Futures.immediateVoidFuture() },
            { mediaId -> if (mediaId in listOf("unknown", "queue-only")) null else JSONObject()
                .put("media_type", "track").put("item_id", mediaId).put("provider", "test").put("name", mediaId) },
        )
        player.updateState(PlaybackSnapshot.empty().copy(
            mediaItems = (0..5).map { media("queue-$it") }, currentIndex = 0,
        ))
    }

    @After
    fun shutdown() {
        player.release()
    }

    // New media use a single MA media-play request, never a queue difference.
    @Test
    fun legacyControllersCanPrepareAndPlayBrowseSelections() {
        player.updateState(PlaybackSnapshot.empty())
        assertTrue(player.isCommandAvailable(Player.COMMAND_PREPARE))
        player.setMediaItem(media("browse:track:test:new"))
        player.prepare()
        player.play()
        assertEquals(listOf("browse:track:test:new"), starts)
        assertTrue(requests.isEmpty())
    }

    @Test
    fun setMediaItemsReplacesOnceAndKeepsTheRequestedStartIndex() {
        invokeCommand("handleSetMediaItems", listOf(media("new"), media("queue-2")), 1, C.TIME_UNSET).get()
        assertEquals(listOf(PlaybackCommandKind.QueuePlayMedia), requests.map { it.kind })
        val request = JSONObject(requests.single().arguments).getJSONObject("request")
        assertEquals("replace", request.getString("option"))
        assertEquals(1, request.getInt("startItem"))
        assertEquals("new", request.getJSONArray("mediaItems").getJSONObject(0).getString("item_id"))
        assertFalse(request.getBoolean("shuffle"))
    }

    @Test
    fun addMediaItemsUsesNextOrAddOnly() {
        invokeCommand("handleAddMediaItems", 1, listOf(media("new"))).get()
        invokeCommand("handleAddMediaItems", 6, listOf(media("new"))).get()
        assertEquals(listOf("next", "add"), requests.map {
            JSONObject(it.arguments).getJSONObject("request").getString("option")
        })
        assertThrows(ExecutionException::class.java) {
            invokeCommand("handleAddMediaItems", 3, listOf(media("new"))).get()
        }
        assertEquals(2, requests.size)
    }

    @Test
    fun existingQueueMediaResolvesWithoutBrowsingItFirst() {
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(JSONObject("{}"), JSONObject("""
            {"items":[{"queue_item_id":"queue-only","name":"Existing","media_item":
                {"media_type":"track","item_id":"server-track","provider":"test","name":"Existing"}}]}
        """))
        player.updateState(snapshot)
        invokeCommand("handleSetMediaItems", snapshot.mediaItems, 0, C.TIME_UNSET).get()
        val media = JSONObject(requests.single().arguments).getJSONObject("request").getJSONArray("mediaItems")
        assertEquals("server-track", media.getJSONObject(0).getString("item_id"))
    }

    @Test
    fun partialReplacementAndUnknownMediaDoNotMutateTheQueue() {
        assertThrows(ExecutionException::class.java) {
            invokeCommand("handleReplaceMediaItems", 1, 2, listOf(media("new"))).get()
        }
        assertThrows(ExecutionException::class.java) {
            invokeCommand("handleSetMediaItems", listOf(media("unknown")), 0, C.TIME_UNSET).get()
        }
        assertTrue(requests.isEmpty())
    }

    @Test
    fun browserActionsKeepTheirDedicatedPlaybackPath() {
        invokeCommand("handleSetMediaItems", listOf(media("action:play-all")), 0, C.TIME_UNSET).get()
        assertEquals(listOf("action:play-all"), starts)
        assertTrue(requests.isEmpty())
    }

    // Existing queue items retain their IDs and issue only direct server mutations.
    @Test
    fun rightwardRangeMovesUseReverseOrderAndRelativeShifts() {
        invokeCommand("handleMoveMediaItems", 1, 3, 3).get()
        assertEquals(listOf("queue-2", "queue-1"), requestedIds())
        assertTrue(requests.all { it.kind == PlaybackCommandKind.MoveQueueItem })
        assertTrue(requests.all { JSONObject(it.arguments).getInt("positionShift") == 2 })
    }

    @Test
    fun leftwardRangeMovesKeepOrder() {
        invokeCommand("handleMoveMediaItems", 3, 5, 1).get()
        assertEquals(listOf("queue-3", "queue-4"), requestedIds())
        assertTrue(requests.all { JSONObject(it.arguments).getInt("positionShift") == -2 })
    }

    @Test
    fun movingToEndUsesTheServerEndCommandInOriginalOrder() {
        invokeCommand("handleMoveMediaItems", 1, 3, 4).get()
        assertEquals(listOf("queue-1", "queue-2"), requestedIds())
        assertTrue(requests.all { it.kind == PlaybackCommandKind.MoveQueueItemToEnd })
    }

    @Test
    fun unchangedMovesNeverSendTheSpecialNextShift() {
        invokeCommand("handleMoveMediaItems", 1, 3, 1).get()
        assertTrue(requests.isEmpty())
    }

    @Test
    fun deletionCapturesAllStableIdsInOneAcknowledgedBatch() {
        val acknowledgement = SettableFuture.create<Void>()
        response = acknowledgement
        val completion = invokeCommand("handleRemoveMediaItems", 1, 3)
        assertEquals(PlaybackCommandKind.DeleteQueueItems, requests.single().kind)
        val itemIds = JSONObject(requests.single().arguments).getJSONArray("itemIds")
        assertEquals(listOf("queue-1", "queue-2"), (0 until itemIds.length()).map { itemIds.getString(it) })
        assertFalse(completion.isDone)
        acknowledgement.setException(IllegalStateException("server rejected deletion"))
        assertThrows(ExecutionException::class.java) { completion.get() }
        assertEquals(1, requests.size)
    }

    @Test
    fun seekingAnotherItemUsesItsStableIdInsteadOfItsIndex() {
        invokeCommand("handleSeek", 2, 0L, Player.COMMAND_SEEK_TO_MEDIA_ITEM).get()
        assertEquals(PlaybackCommandKind.SelectQueueItem, requests.single().kind)
        assertEquals(listOf("queue-2"), requestedIds())
    }

    // Transport-only updates retain Media3 period data; presentation changes rebuild it.
    @Test
    fun positionAndTransportUpdatesReuseProjectedPlaylist() {
        val snapshot = PlaybackSnapshot.empty().copy(
            status = PlaybackStatus.Paused, mediaItems = listOf(media("queue-1")), durationMillis = 60000,
        )
        player.updateState(snapshot)
        val playlist = projectedPlaylist()
        player.updateState(snapshot.copy(positionMillis = 2500, bufferedPositionMillis = 3000, volume = 75))
        assertSame(playlist, projectedPlaylist())
        assertEquals(2500L, player.currentPosition)
        assertEquals(3000L, player.bufferedPosition)
        assertEquals(0.75f, player.volume)
        player.updateState(snapshot.copy(status = PlaybackStatus.Playing))
        assertSame(playlist, projectedPlaylist())
        assertTrue(player.playWhenReady)
    }

    @Test
    fun identicalFullSnapshotsAndPositionTicksNeverPublishTimelineChanges() {
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","name":"Artist - Song","media_item":{"media_type":"track","name":"Song","duration":60,"artists":[{"name":"Artist"}]}}]}""")
        val state = JSONObject("""{"currentQueueItemId":"queue:1","playbackState":"playing","durationMillis":60000}""")
        val initial = PlaybackSnapshotMapper.fromPlayerState(state, queue)
        player.updateState(initial)
        val playlist = projectedPlaylist()
        var timelineChanges = 0
        player.addListener(object : Player.Listener {
            override fun onTimelineChanged(timeline: androidx.media3.common.Timeline, reason: Int) {
                timelineChanges++
            }
        })
        var previous = initial
        repeat(20) { tick ->
            state.put("positionMillis", tick * 250L).put("bufferedPositionMillis", tick * 250L + 1000)
            val refreshed = if (tick % 2 == 0) {
                PlaybackSnapshotMapper.fromPlayerState(state, JSONObject(queue.toString())).also {
                    assertNotSame(previous.mediaItems, it.mediaItems)
                }
            } else requireNotNull(PlaybackSnapshotMapper.updatePlayerState(previous, state))
            player.updateState(refreshed)
            assertSame(playlist, projectedPlaylist())
            previous = refreshed
        }
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
        assertEquals(0, timelineChanges)
        assertEquals(4750L, player.currentPosition)
        assertEquals(5750L, player.bufferedPosition)
    }

    @Test
    fun selectionChangesKeepPlaylistWhenItemDurationsStayTheSame() {
        val mediaItems = listOf("queue-1", "queue-2").map { id ->
            media(id).buildUpon().setMediaMetadata(androidx.media3.common.MediaMetadata.Builder()
                .setDurationMs(60000).build()).build()
        }
        val initial = PlaybackSnapshot.empty().copy(mediaItems = mediaItems, durationMillis = 60000)
        player.updateState(initial)
        val playlist = projectedPlaylist()
        var timelineChanges = 0
        var itemTransitions = 0
        player.addListener(object : Player.Listener {
            override fun onTimelineChanged(timeline: androidx.media3.common.Timeline, reason: Int) {
                timelineChanges++
            }
            override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) {
                itemTransitions++
            }
        })
        player.updateState(initial.copy(currentIndex = 1))
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
        assertSame(playlist, projectedPlaylist())
        assertEquals(1, player.currentMediaItemIndex)
        assertEquals(0, timelineChanges)
        assertEquals(1, itemTransitions)
    }

    @Test
    fun realQueueEditsAndPresentationChangesPublishTimelineUpdates() {
        val initial = PlaybackSnapshot.empty().copy(
            mediaItems = listOf(media("queue-1"), media("queue-2")), durationMillis = 60000,
        )
        val renamed = initial.mediaItems.first().buildUpon().setMediaMetadata(
            androidx.media3.common.MediaMetadata.Builder().setTitle("Updated title").build()).build()
        val newCover = renamed.buildUpon().setMediaMetadata(renamed.mediaMetadata.buildUpon()
            .setArtworkUri(android.net.Uri.parse("content://com.joie96.mashin.mediaart/updated")).build()).build()
        var timelineChanges = 0
        player.updateState(initial)
        player.addListener(object : Player.Listener {
            override fun onTimelineChanged(timeline: androidx.media3.common.Timeline, reason: Int) {
                timelineChanges++
            }
        })
        val updates = listOf(
            initial.copy(mediaItems = initial.mediaItems + media("queue-3")),
            initial.copy(mediaItems = initial.mediaItems.reversed()),
            initial.copy(mediaItems = listOf(initial.mediaItems.first())),
            initial.copy(mediaItems = listOf(renamed)),
            initial.copy(mediaItems = listOf(newCover)),
            initial.copy(mediaItems = listOf(newCover), durationMillis = 90000),
        )
        updates.forEachIndexed { index, updated ->
            val previousPlaylist = projectedPlaylist()
            player.updateState(updated)
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            assertNotSame(previousPlaylist, projectedPlaylist())
            assertEquals(index + 1, timelineChanges)
            assertEquals(updated.mediaItems.map { it.mediaId },
                (0 until player.mediaItemCount).map { player.getMediaItemAt(it).mediaId })
            assertEquals(updated.mediaItems.first().mediaMetadata.title, player.mediaMetadata.title)
            assertEquals(updated.mediaItems.first().mediaMetadata.artworkUri, player.mediaMetadata.artworkUri)
            assertEquals(updated.durationMillis, player.duration)
        }
    }

    @Test
    fun changedMetadataDurationAndSelectionRebuildProjectedPlaylist() {
        val snapshot = PlaybackSnapshot.empty().copy(
            mediaItems = listOf(media("queue-1"), media("queue-2")), durationMillis = 60000,
        )
        player.updateState(snapshot)
        var previousPlaylist = projectedPlaylist()
        for (updated in listOf(
            snapshot.copy(durationMillis = 90000),
            snapshot.copy(currentIndex = 1),
            snapshot.copy(mediaItems = listOf(media("replacement"))),
            PlaybackSnapshot.empty(),
        )) {
            player.updateState(updated)
            assertNotSame(previousPlaylist, projectedPlaylist())
            assertEquals(updated.mediaItems.size, player.mediaItemCount)
            previousPlaylist = projectedPlaylist()
        }
    }

    // Invoke the protected Media3 boundary without its optimistic local timeline.
    private fun invokeCommand(name: String, vararg arguments: Any): ListenableFuture<*> {
        val method = RustSessionPlayer::class.java.declaredMethods.single { it.name == name }
        method.isAccessible = true
        return method.invoke(player, *arguments) as ListenableFuture<*>
    }

    private fun media(mediaId: String): MediaItem = MediaItem.Builder()
        .setMediaId(mediaId).setUri("mashin://queue/$mediaId").build()

    private fun requestedIds(): List<String> = requests.map { JSONObject(it.arguments).getString("itemId") }

    private fun projectedPlaylist(): Any = requireNotNull(RustSessionPlayer::class.java.getDeclaredField("playlist")
        .apply { isAccessible = true }.get(player))
}