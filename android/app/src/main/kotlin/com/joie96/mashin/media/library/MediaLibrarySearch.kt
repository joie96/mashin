package com.joie96.mashin.media.library

import android.util.Log
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.MoreExecutors
import java.util.concurrent.Executors
import org.json.JSONObject

internal enum class SearchMediaCategory(val resultKey: String, val mediaType: String, val title: String) {
    Tracks("tracks", "track", "Tracks"),
    Albums("albums", "album", "Alben"),
    Playlists("playlists", "playlist", "Playlists"),
    Artists("artists", "artist", "Artists"),
}

/** Owns parallel Android searches and reuses boundary results across Media3 callbacks. */
internal class MediaLibrarySearch(private val queryCore: (String, String?) -> String?) {
    private val executor = MoreExecutors.listeningDecorator(Executors.newFixedThreadPool(4))
    private val searches = LinkedHashMap<String, Map<SearchMediaCategory, ListenableFuture<String>>>()
    private var isClosed = false

    // Search worker lifecycle.
    @Synchronized
    fun close() {
        isClosed = true
        searches.values.forEach { results -> results.values.forEach { it.cancel(true) } }
        searches.clear()
        executor.shutdownNow()
    }

    // Queries share one request per media type, including in-flight requests.
    @Synchronized
    fun start(query: String): Map<SearchMediaCategory, ListenableFuture<String>> {
        check(!isClosed) { "Media library search is closed" }
        if (query.isBlank()) return emptyMap()
        searches[query]?.let { return it }
        val results = SearchMediaCategory.entries.associateWith { category ->
            executor.submit<String> {
                try {
                    val argument = JSONObject().put("query", query).put("mediaType", category.mediaType)
                        .put("limit", 50).toString()
                    val response = checkNotNull(queryCore("searchMedia", argument)) { "Missing search response" }
                    JSONObject(response).optJSONArray(category.resultKey)?.toString() ?: "[]"
                } catch (error: Exception) {
                    Log.w("MashinLibrarySearch", "${category.mediaType} search failed", error)
                    "[]"
                }
            }
        }
        searches[query] = results
        if (searches.size > 8) {
            val oldestQuery = searches.keys.first()
            searches.remove(oldestQuery)?.values?.forEach { it.cancel(true) }
        }
        return results
    }

    fun results(query: String, category: SearchMediaCategory): ListenableFuture<String> =
        start(query)[category] ?: Futures.immediateFuture("[]")

    @Synchronized
    fun isCurrent(query: String, category: SearchMediaCategory, result: ListenableFuture<String>): Boolean =
        !isClosed && searches[query]?.get(category) === result
}

