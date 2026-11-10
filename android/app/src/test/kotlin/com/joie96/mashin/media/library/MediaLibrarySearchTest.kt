package com.joie96.mashin.media.library

import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeUnit
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Assert.assertThrows
import org.junit.Assert.assertNotSame
import org.junit.Test
import org.junit.After
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class MediaLibrarySearchTest {
    private val queryFixture = BlockingSearchFixture()

    @After
    fun closeFixture() = queryFixture.close()

    @Test
    fun newSearchDispatchesBeforeOldRequestsCompleteAndCloseCancelsAll() {
        val requests = mutableListOf<com.google.common.util.concurrent.SettableFuture<String>>()
        val search = MediaLibrarySearch { _, _ ->
            com.google.common.util.concurrent.SettableFuture.create<String>().also(requests::add)
        }
        search.start("old")
        search.start("new")
        assertEquals(8, requests.size)
        assertTrue(requests.none { it.isDone })
        search.close()
        assertTrue(requests.all { it.isCancelled })
    }
    @Test
    fun allCategoriesObserveEmptyValuesAndNotifyOnlyChangedMatchingValues() {
        val started = CountDownLatch(4)
        val releaseRefresh = CountDownLatch(1)
        val changes = mutableListOf<Pair<SearchMediaCategory, Int>>()
        val search = queryFixture.search { _, _ ->
            started.countDown()
            check(releaseRefresh.await(5, TimeUnit.SECONDS))
            error("Offline refresh")
        }
        search.onResultsChanged = { query, category, result, count ->
            assertEquals("jazz", query)
            assertTrue(search.isCurrent(query, category, result))
            changes.add(category to count)
        }
        try {
            val results = search.start("jazz")
            assertTrue(started.await(5, TimeUnit.SECONDS))
            for (category in SearchMediaCategory.entries) {
                search.onCatalogEvent(catalogEvent(category, query = "old"))
                search.onCatalogEvent(catalogEvent(category, limit = 1))
                search.onCatalogEvent(catalogEvent(category).put("value", JSONObject().put("Albums", org.json.JSONArray())))
                assertFalse(results.getValue(category).isDone)
                search.onCatalogEvent(catalogEvent(category))
                assertEquals("[]", results.getValue(category).get(5, TimeUnit.SECONDS))
                search.onCatalogEvent(catalogEvent(category))
                assertTrue(changes.none { it.first == category })
                val items = org.json.JSONArray().put(JSONObject().put("name", "Cached"))
                search.onCatalogEvent(catalogEvent(category, items = items))
                assertEquals("Cached", org.json.JSONArray(search.results("jazz", category).get()).getJSONObject(0).getString("name"))
                search.onCatalogEvent(catalogEvent(category, items = items))
                search.onCatalogEvent(catalogEvent(category))
                assertEquals(listOf(category to 1, category to 0), changes.filter { it.first == category })
                assertEquals("[]", search.results("jazz", category).get())
            }
        } finally {
            releaseRefresh.countDown()
            search.close()
        }
    }

    @Test
    fun completedRefreshPublishesNewValueWithoutReplacingTheFirstSnapshot() {
        val started = CountDownLatch(4)
        val releaseRefresh = CountDownLatch(1)
        val changed = CountDownLatch(1)
        val search = queryFixture.search { _, argument ->
            val category = SearchMediaCategory.entries.first {
                it.mediaType == JSONObject(argument!!).getString("mediaType")
            }
            started.countDown()
            check(releaseRefresh.await(5, TimeUnit.SECONDS))
            JSONObject().put(category.resultKey, org.json.JSONArray().put(JSONObject().put("name", "Refreshed"))).toString()
        }
        search.onResultsChanged = { _, category, _, _ -> if (category == SearchMediaCategory.Tracks) changed.countDown() }
        try {
            val results = search.start("jazz")
            assertTrue(started.await(5, TimeUnit.SECONDS))
            search.onCatalogEvent(catalogEvent(SearchMediaCategory.Tracks,
                items = org.json.JSONArray().put(JSONObject().put("name", "Cached"))))
            releaseRefresh.countDown()
            assertTrue(changed.await(5, TimeUnit.SECONDS))
            assertEquals("Cached", org.json.JSONArray(results.getValue(SearchMediaCategory.Tracks).get()).getJSONObject(0).getString("name"))
            assertEquals("Refreshed", org.json.JSONArray(search.results("jazz", SearchMediaCategory.Tracks).get()).getJSONObject(0).getString("name"))
        } finally {
            releaseRefresh.countDown()
            search.close()
        }
    }

    @Test
    fun invalidationAndCloseRetireOnlyTheirOwnedSearches() {
        val started = CountDownLatch(4)
        val releaseRefresh = CountDownLatch(1)
        val invalidated = mutableListOf<String>()
        val search = queryFixture.search { _, _ ->
            started.countDown()
            check(releaseRefresh.await(5, TimeUnit.SECONDS))
            "{}"
        }
        search.onSearchInvalidated = invalidated::add
        try {
            val first = search.start("jazz")
            assertTrue(started.await(5, TimeUnit.SECONDS))
            val invalidation = catalogEvent(SearchMediaCategory.Tracks)
                .put("type", "catalog_read_changed").put("snapshot", JSONObject().put("phase", "invalidated"))
            search.onCatalogEvent(JSONObject(invalidation.toString()).put("query", JSONObject().put("Search",
                JSONObject().put("query", "old").put("media_type", "track").put("limit", 50))))
            assertTrue(search.isCurrent("jazz", SearchMediaCategory.Tracks, first.getValue(SearchMediaCategory.Tracks)))
            search.onCatalogEvent(invalidation)
            assertEquals(listOf("jazz"), invalidated)
            assertTrue(first.values.all { it.isCancelled })
            assertFalse(search.isCurrent("jazz", SearchMediaCategory.Tracks, first.getValue(SearchMediaCategory.Tracks)))
            val next = search.start("jazz")
            assertNotSame(first, next)
            search.start("rock")
            search.invalidate()
            assertEquals(listOf("jazz", "jazz", "rock"), invalidated)
            assertTrue(next.values.all { it.isCancelled })
            val last = search.start("last")
            search.close()
            search.onCatalogEvent(catalogEvent(SearchMediaCategory.Tracks, query = "last"))
            assertTrue(last.values.all { it.isCancelled })
            assertFalse(search.isCurrent("last", SearchMediaCategory.Tracks, last.getValue(SearchMediaCategory.Tracks)))
        } finally {
            releaseRefresh.countDown()
            search.close()
        }
    }

    @Test
    fun cachedSearchValueCompletesBeforeRefreshAndSurvivesItsFailure() {
        val started = CountDownLatch(4)
        val releaseRefresh = CountDownLatch(1)
        val finished = CountDownLatch(4)
        val search = queryFixture.search { _, _ ->
            started.countDown()
            try {
                check(releaseRefresh.await(5, TimeUnit.SECONDS))
                error("Offline refresh")
            } finally {
                finished.countDown()
            }
        }
        try {
            val results = search.start("jazz")
            assertTrue(started.await(5, TimeUnit.SECONDS))
            search.onCatalogEvent(JSONObject("""{
                "type":"catalog_updated",
                "query":{"Search":{"query":"jazz","media_type":"track","limit":50}},
                "value":{"Search":{"tracks":[{"item_id":"cached","provider":"library","name":"Cached"}]}}
            }"""))
            val tracks = results.getValue(SearchMediaCategory.Tracks)
            assertTrue("Cached tracks must not wait for the remote refresh", tracks.isDone)
            val cached = tracks.get(5, TimeUnit.SECONDS)
            assertEquals("Cached", org.json.JSONArray(cached).getJSONObject(0).getString("name"))
            assertFalse(results.getValue(SearchMediaCategory.Albums).isDone)
            releaseRefresh.countDown()
            assertTrue(finished.await(5, TimeUnit.SECONDS))
            assertEquals(cached, search.results("jazz", SearchMediaCategory.Tracks).get(5, TimeUnit.SECONDS))
        } finally {
            releaseRefresh.countDown()
            search.close()
        }
    }

    // Parallel completion and shared Media3 result requests.
    @Test
    fun loadsEachCategoryIndependentlyAndReusesItsResult() {
        val started = CountDownLatch(4)
        val releaseAlbums = CountDownLatch(1)
        val requests = ConcurrentHashMap<String, JSONObject>()
        val search = queryFixture.search { kind, argument ->
            assertEquals("searchMedia", kind)
            val request = JSONObject(argument!!)
            val mediaType = request.getString("mediaType")
            requests[mediaType] = request
            started.countDown()
            check(started.await(5, TimeUnit.SECONDS)) { "Searches were not parallel" }
            if (mediaType == "album") check(releaseAlbums.await(5, TimeUnit.SECONDS))
            if (mediaType == "artist") error("Provider unavailable")
            val category = SearchMediaCategory.entries.first { it.mediaType == mediaType }
            "{\"${category.resultKey}\":[{\"name\":\"Result\"}]}"
        }
        try {
            val results = search.start("jazz")
            assertTrue(started.await(5, TimeUnit.SECONDS))
            assertEquals("[{\"name\":\"Result\"}]", results.getValue(SearchMediaCategory.Tracks).get(5, TimeUnit.SECONDS))
            results.getValue(SearchMediaCategory.Playlists).get(5, TimeUnit.SECONDS)
            assertThrows(ExecutionException::class.java) {
                results.getValue(SearchMediaCategory.Artists).get(5, TimeUnit.SECONDS)
            }
            assertFalse(results.getValue(SearchMediaCategory.Albums).isDone)
            assertFalse(search.results("jazz", SearchMediaCategory.Albums).isDone)
            assertTrue(search.results("jazz", SearchMediaCategory.Artists).isDone)
            assertTrue(search.results("jazz", SearchMediaCategory.Tracks).isDone)
            assertSame(results, search.start("jazz"))
            assertSame(results.getValue(SearchMediaCategory.Tracks), search.results("jazz", SearchMediaCategory.Tracks))
            assertEquals(4, requests.size)
            requests.values.forEach {
                assertEquals("jazz", it.getString("query"))
                assertEquals(50, it.getInt("limit"))
            }
            releaseAlbums.countDown()
            results.getValue(SearchMediaCategory.Albums).get(5, TimeUnit.SECONDS)
            assertTrue(search.results("jazz", SearchMediaCategory.Albums).isDone)
        } finally {
            releaseAlbums.countDown()
            search.close()
        }
    }

    @Test
    fun blankSearchDoesNotQueryCoreAndDifferentQueriesRemainSeparate() {
        val search = queryFixture.search { _, argument ->
            val request = JSONObject(argument!!)
            "{\"tracks\":[{\"name\":\"${request.getString("query")}\"}]}"
        }
        try {
            assertTrue(search.start("  ").isEmpty())
            assertTrue(search.results("  ", SearchMediaCategory.Tracks).isDone)
            val oldResult = search.results("old", SearchMediaCategory.Tracks)
            val newResult = search.results("new", SearchMediaCategory.Tracks)
            assertEquals("[{\"name\":\"old\"}]", oldResult.get(5, TimeUnit.SECONDS))
            assertEquals("[{\"name\":\"new\"}]", newResult.get(5, TimeUnit.SECONDS))
        } finally {
            search.close()
        }
    }

    private fun catalogEvent(
        category: SearchMediaCategory,
        query: String = "jazz",
        limit: Int = 50,
        items: org.json.JSONArray = org.json.JSONArray(),
    ): JSONObject = JSONObject().put("type", "catalog_updated")
        .put("query", JSONObject().put("Search", JSONObject()
            .put("query", query).put("media_type", category.mediaType).put("limit", limit)))
        .put("value", JSONObject().put("Search", JSONObject().put(category.resultKey, items)))
}

internal class BlockingSearchFixture : AutoCloseable {
    private val executor = com.google.common.util.concurrent.MoreExecutors.listeningDecorator(java.util.concurrent.Executors.newFixedThreadPool(4))

    fun search(query: (String, String?) -> String?): MediaLibrarySearch = MediaLibrarySearch { kind, argument ->
        executor.submit<String> { checkNotNull(query(kind, argument)) }
    }

    override fun close() { executor.shutdownNow() }
}