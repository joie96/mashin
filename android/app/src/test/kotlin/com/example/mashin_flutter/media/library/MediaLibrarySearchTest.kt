package com.example.mashin_flutter.media.library

import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class MediaLibrarySearchTest {
    // Parallel completion and shared Media3 result requests.
    @Test
    fun loadsEachCategoryIndependentlyAndReusesItsResult() {
        val started = CountDownLatch(4)
        val releaseAlbums = CountDownLatch(1)
        val requests = ConcurrentHashMap<String, JSONObject>()
        val search = MediaLibrarySearch { kind, argument ->
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
            assertEquals("[]", results.getValue(SearchMediaCategory.Artists).get(5, TimeUnit.SECONDS))
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
        val search = MediaLibrarySearch { _, argument ->
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
}