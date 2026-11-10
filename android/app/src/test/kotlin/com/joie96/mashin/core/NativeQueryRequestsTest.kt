package com.joie96.mashin.core

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class NativeQueryRequestsTest {
    @Test
    fun completionInvokesConsumersWithoutHoldingDispatcherLock() {
        var id = 0L
        val requests = NativeQueryRequests({ requestId, _, _ -> id = requestId; true }, { true })
        val result = requests.execute("recentlyPlayedTracks", "{}")
        var heldLock = true
        result.addListener({ heldLock = Thread.holdsLock(requests) }, com.google.common.util.concurrent.MoreExecutors.directExecutor())
        requests.onCoreEvent(JSONObject().put("type", "android_query_completed").put("request_id", id).put("result", JSONObject()))
        assertFalse(heldLock)
        requests.close()
    }

    @Test
    fun dispatchesWithoutWaitingAndCancelsOnlyOwnedRequests() {
        val started = mutableListOf<Long>()
        val canceled = mutableListOf<Long>()
        val requests = NativeQueryRequests({ id, _, _ -> started.add(id); true }, { id -> canceled.add(id); true })
        val first = requests.execute("searchMedia", "{}")
        val second = requests.execute("albumTracks", "{}")
        assertEquals(2, started.size)
        assertTrue(started.all { it < 0 })
        first.cancel(false)
        assertEquals(listOf(started.first()), canceled)
        requests.onCoreEvent(JSONObject().put("type", "android_query_completed")
            .put("request_id", started.first()).put("result", JSONObject()))
        assertFalse(second.isDone)
        requests.onCoreEvent(JSONObject().put("type", "android_query_completed")
            .put("request_id", started.last()).put("result", JSONObject().put("name", "cached")))
        assertEquals("cached", JSONObject(second.get()).getString("name"))
        requests.close()
        assertEquals(1, canceled.size)
    }

    @Test
    fun closeAndFailuresFinishPendingRequestsWithoutIdReuse() {
        val started = mutableListOf<Long>()
        val canceled = mutableListOf<Long>()
        val requests = NativeQueryRequests({ id, _, _ -> started.add(id); true }, { id -> canceled.add(id); true })
        val failed = requests.execute("artistAlbums", "{}")
        requests.onCoreEvent(JSONObject().put("type", "android_query_failed").put("request_id", started.last())
            .put("error", JSONObject().put("message", "offline")))
        assertThrows(java.util.concurrent.ExecutionException::class.java) { failed.get() }
        val pending = requests.execute("albumTracks", "{}")
        requests.close()
        assertTrue(pending.isCancelled)
        assertEquals(listOf(started.last()), canceled)
        assertThrows(IllegalStateException::class.java) { requests.execute("albumTracks", "{}") }
        val next = NativeQueryRequests({ id, _, _ -> started.add(id); true }, { true })
        next.execute("albumTracks", "{}")
        assertEquals(started.size, started.toSet().size)
        next.close()
    }
}