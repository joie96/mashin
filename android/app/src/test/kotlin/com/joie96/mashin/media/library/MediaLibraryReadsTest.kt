package com.joie96.mashin.media.library

import com.google.common.util.concurrent.SettableFuture
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class MediaLibraryReadsTest {
    @Test
    fun explicitRetryPreservesCacheAndCoalescesConsumers() {
        val requests = mutableListOf<SettableFuture<String>>()
        val reads = MediaLibraryReads { _, _ -> SettableFuture.create<String>().also(requests::add) }
        val read = LibraryRead(LibraryReadKind.AlbumTracks, "id", "provider")
        val first = reads.results(read)
        requests.single().setException(IllegalStateException("offline"))
        assertThrows(java.util.concurrent.ExecutionException::class.java) { first.get() }
        reads.retryFailed(read)
        val retry = reads.results(read)
        reads.retryFailed(read)
        assertEquals(2, requests.size)
        assertFalse(retry.isDone)
        reads.onCoreEvent(event(read, JSONArray().put(JSONObject().put("name", "cached"))))
        requests.last().setException(IllegalStateException("offline again"))
        reads.retryFailed(read)
        assertEquals("cached", JSONArray(reads.results(read).get()).getJSONObject(0).getString("name"))
        assertEquals(3, requests.size)
        requests.last().set("[]")
        assertEquals("[]", reads.results(read).get())
        reads.close()
    }

    @Test
    fun recommendationChangesCoalesceBehindActiveReadAndRetainVisibleFolders() {
        val requests = mutableListOf<SettableFuture<String>>()
        val reads = MediaLibraryReads { _, _ -> SettableFuture.create<String>().also(requests::add) }
        val read = LibraryRead(LibraryReadKind.HomeRecommendations)
        reads.results(read)
        reads.refreshRecommendations()
        reads.refreshRecommendations()
        assertEquals(1, requests.size)
        requests.first().set("""{"revision":1,"folders":[],"is_loading":false}""")
        assertEquals(2, requests.size)
        assertEquals(1, JSONObject(reads.results(read).get()).getLong("revision"))
        requests.last().set("""{"revision":2,"folders":[],"is_loading":false}""")
        assertEquals(2, JSONObject(reads.results(read).get()).getLong("revision"))
        assertEquals(2, requests.size)
        reads.close()
    }

    @Test
    fun payloadFreeStatusRetainsCachedValueUntilInvalidation() {
        val request = SettableFuture.create<String>()
        val read = LibraryRead(LibraryReadKind.PlaylistTracks, "id", "provider")
        val reads = MediaLibraryReads { _, _ -> request }
        val first = reads.results(read)
        reads.onCoreEvent(event(read, JSONArray().put(JSONObject().put("name", "cached"))))
        val status = JSONObject().put("type", "catalog_read_changed")
            .put("query", event(read, JSONArray()).getJSONObject("query"))
            .put("snapshot", JSONObject().put("phase", "failed").put("source", "cache")
                .put("knowledge", "observed_response").put("error", "offline"))
        assertFalse(status.getJSONObject("snapshot").has("value"))
        reads.onCoreEvent(status)
        assertEquals("cached", JSONArray(reads.results(read).get()).getJSONObject(0).getString("name"))
        assertTrue(first.isDone)
        assertFalse(request.isDone)
        var invalidated: LibraryRead? = null
        reads.onInvalidated = { invalidated = it }
        status.getJSONObject("snapshot").put("phase", "invalidated")
        reads.onCoreEvent(status)
        assertEquals(read, invalidated)
        assertTrue(request.isCancelled)
        reads.close()
    }

    @Test
    fun everyReadAcceptsKnownEmptyBeforeRefreshAndRetainsItOnFailure() {
        LibraryReadKind.entries.filter { it.command != "recommendations" }.forEach { kind ->
            val remote = SettableFuture.create<String>()
            val read = LibraryRead(kind, if (kind == LibraryReadKind.Recent) null else "id", "provider")
            val reads = MediaLibraryReads { command, _ -> assertEquals(kind.command, command); remote }
            val changes = mutableListOf<LibraryRead>()
            reads.onChanged = changes::add
            val first = reads.results(read)
            assertFalse(first.isDone)
            reads.onCoreEvent(event(read, JSONArray()))
            assertEquals("[]", first.get())
            assertFalse(remote.isDone)
            reads.onCoreEvent(event(read, JSONArray()))
            assertEquals(listOf(read), changes)
            remote.setException(IllegalStateException("offline"))
            assertEquals("[]", reads.results(read).get())
            reads.close()
        }
    }

    @Test
    fun exactIdentityReplacementAndInvalidationRejectOldRequests() {
        val requests = mutableListOf<SettableFuture<String>>()
        val reads = MediaLibraryReads { _, _ -> SettableFuture.create<String>().also(requests::add) }
        val read = LibraryRead(LibraryReadKind.AlbumTracks, "id", "provider")
        val first = reads.results(read)
        reads.onCoreEvent(event(read.copy(provider = "other"), JSONArray()))
        assertFalse(first.isDone)
        reads.onCoreEvent(event(read, JSONArray().put(JSONObject().put("name", "cached"))))
        assertEquals("cached", JSONArray(first.get()).getJSONObject(0).getString("name"))
        reads.release(read)
        assertTrue(requests.single().isCancelled)
        val replacement = reads.results(read)
        requests.first().set("[]")
        assertFalse(replacement.isDone)
        var invalidated: LibraryRead? = null
        reads.onInvalidated = { invalidated = it }
        reads.onCoreEvent(event(read, JSONArray()).put("type", "catalog_read_changed")
            .put("snapshot", JSONObject().put("phase", "invalidated")))
        assertEquals(read, invalidated)
        assertTrue(replacement.isCancelled)
        assertTrue(requests.last().isCancelled)
        reads.close()
    }

    @Test
    fun failedMissRemainsErrorWhileOtherReadsCompleteAndCloseCancelsPending() {
        val requests = mutableListOf<SettableFuture<String>>()
        val reads = MediaLibraryReads { _, _ -> SettableFuture.create<String>().also(requests::add) }
        val first = reads.results(LibraryRead(LibraryReadKind.Recent))
        val second = reads.results(LibraryRead(LibraryReadKind.ArtistAlbums, "a", "p"))
        requests.first().setException(IllegalStateException("offline"))
        assertThrows(java.util.concurrent.ExecutionException::class.java) { first.get() }
        assertFalse(second.isDone)
        reads.close()
        assertTrue(second.isCancelled)
        assertTrue(requests.last().isCancelled)
    }

    @Test
    fun recommendationViewsReceiveIndependentEarlySnapshots() {
        val requests = mutableListOf<SettableFuture<String>>()
        val reads = MediaLibraryReads { _, _ -> SettableFuture.create<String>().also(requests::add) }
        val home = reads.results(LibraryRead(LibraryReadKind.HomeRecommendations))
        val explore = reads.results(LibraryRead(LibraryReadKind.ExploreRecommendations))
        reads.onCoreEvent(JSONObject().put("type", "recommendations_updated").put("view", "home")
            .put("snapshot", JSONObject().put("folders", JSONArray()).put("is_loading", true)))
        assertTrue(JSONObject(home.get()).getBoolean("is_loading"))
        assertFalse(explore.isDone)
        assertTrue(requests.none { it.isDone })
        assertFalse(reads.hasObservedFolders(LibraryRead(LibraryReadKind.HomeRecommendations)))
        reads.onCoreEvent(JSONObject().put("type", "catalog_read_changed")
            .put("query", "RecommendationFolders")
            .put("snapshot", JSONObject().put("knowledge", "observed_response")))
        assertTrue(reads.hasObservedFolders(LibraryRead(LibraryReadKind.HomeRecommendations)))
        reads.close()
    }

    @Test
    fun recommendationFolderInvalidationCancelsItsNativeRead() {
        val request = SettableFuture.create<String>()
        val read = LibraryRead(LibraryReadKind.HomeRecommendations)
        val reads = MediaLibraryReads { _, _ -> request }
        val first = reads.results(read)
        var invalidated: LibraryRead? = null
        reads.onInvalidated = { invalidated = it }
        reads.onCoreEvent(JSONObject().put("type", "catalog_read_changed")
            .put("query", "RecommendationFolders")
            .put("snapshot", JSONObject().put("phase", "invalidated")))
        assertTrue(first.isCancelled)
        assertTrue(request.isCancelled)
        assertEquals(read, invalidated)
        reads.close()
    }

    @Test
    fun recommendationDeltasRetainNeighborsRemoveFoldersAndRejectRetiredValues() {
        assertRecommendationDeltas(LibraryReadKind.HomeRecommendations)
    }

    @Test
    fun exploreDeltasRetainNeighborsRemoveFoldersAndRejectRetiredValues() {
        assertRecommendationDeltas(LibraryReadKind.ExploreRecommendations)
    }

    private fun assertRecommendationDeltas(kind: LibraryReadKind) {
        val request = SettableFuture.create<String>()
        val reads = MediaLibraryReads { _, _ -> request }
        val read = LibraryRead(kind)
        val first = reads.results(read)
        fun folder(id: String, title: String) = JSONObject().put("provider", "source")
            .put("item_id", id).put("title", title).put("items", JSONArray())
        fun publish(revision: Long, sequence: Long, folders: List<JSONObject>, order: List<String>) {
            reads.onCoreEvent(JSONObject().put("type", "recommendations_updated")
                .put("view", if (kind == LibraryReadKind.HomeRecommendations) "home" else "explore")
                .put("snapshot", JSONObject().put("revision", revision).put("folders", JSONArray(folders))
                    .put("is_loading", false).put("has_load_error", false)
                    .put("delta", JSONObject().put("revision", revision).put("sequence", sequence)
                        .put("order", JSONArray(order.map { JSONObject().put("provider", "source").put("item_id", it) })))))
        }
        publish(1, 1, listOf(folder("a", "First"), folder("b", "Second")), listOf("a", "b"))
        assertEquals(2, JSONObject(first.get()).getJSONArray("folders").length())
        publish(1, 2, listOf(folder("b", "Updated")), listOf("a", "b"))
        val updated = JSONObject(reads.results(read).get()).getJSONArray("folders")
        assertEquals("First", updated.getJSONObject(0).getString("title"))
        assertEquals("Updated", updated.getJSONObject(1).getString("title"))
        publish(1, 2, listOf(folder("b", "Duplicate")), listOf("a", "b"))
        assertEquals("Updated", JSONObject(reads.results(read).get()).getJSONArray("folders").getJSONObject(1).getString("title"))
        publish(1, 3, emptyList(), listOf("a"))
        assertEquals(1, JSONObject(reads.results(read).get()).getJSONArray("folders").length())
        publish(2, 1, listOf(folder("a", "New read")), listOf("a"))
        request.set(JSONObject().put("revision", 1).put("folders", JSONArray())
            .put("is_loading", false).put("has_load_error", false).toString())
        assertEquals("New read", JSONObject(reads.results(read).get()).getJSONArray("folders").getJSONObject(0).getString("title"))
        var invalidated: LibraryRead? = null
        reads.onInvalidated = { invalidated = it }
        publish(2, 3, listOf(folder("a", "Gap")), listOf("a"))
        assertEquals(read, invalidated)
        reads.close()
    }

    private fun event(read: LibraryRead, items: JSONArray): JSONObject = JSONObject()
        .put("type", "catalog_updated")
        .put("query", JSONObject().put(read.kind.catalog, JSONObject().apply {
            put("item_id", read.itemId); put("provider", read.provider); put("limit", 50)
        }))
        .put("value", JSONObject().put(read.kind.value, items))
}