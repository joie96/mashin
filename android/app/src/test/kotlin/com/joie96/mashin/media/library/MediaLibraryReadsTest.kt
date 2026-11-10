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

    private fun event(read: LibraryRead, items: JSONArray): JSONObject = JSONObject()
        .put("type", "catalog_updated")
        .put("query", JSONObject().put(read.kind.catalog, JSONObject().apply {
            put("item_id", read.itemId); put("provider", read.provider); put("limit", 50)
        }))
        .put("value", JSONObject().put(read.kind.value, items))
}