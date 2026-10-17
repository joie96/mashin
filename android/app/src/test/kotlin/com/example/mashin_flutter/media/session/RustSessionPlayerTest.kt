package com.example.mashin_flutter.media.session

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
    fun deletionCapturesStableIdsAndStopsAfterFailure() {
        val acknowledgement = SettableFuture.create<Void>()
        response = acknowledgement
        val completion = invokeCommand("handleRemoveMediaItems", 1, 3)
        assertEquals(listOf("queue-1"), requestedIds())
        acknowledgement.setException(IllegalStateException("server rejected deletion"))
        assertThrows(ExecutionException::class.java) { completion.get() }
        assertEquals(listOf("queue-1"), requestedIds())
        assertEquals(PlaybackCommandKind.DeleteQueueItem, requests.single().kind)
    }

    @Test
    fun seekingAnotherItemUsesItsStableIdInsteadOfItsIndex() {
        invokeCommand("handleSeek", 2, 0L, Player.COMMAND_SEEK_TO_MEDIA_ITEM).get()
        assertEquals(PlaybackCommandKind.SelectQueueItem, requests.single().kind)
        assertEquals(listOf("queue-2"), requestedIds())
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
}