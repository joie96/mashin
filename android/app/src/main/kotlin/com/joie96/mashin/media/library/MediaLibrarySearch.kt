package com.joie96.mashin.media.library

import android.util.Log
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.FutureCallback
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.MoreExecutors
import com.google.common.util.concurrent.SettableFuture
import org.json.JSONArray
import org.json.JSONObject

internal enum class SearchMediaCategory(val resultKey: String, val mediaType: String, val title: String) {
    Tracks("tracks", "track", "Tracks"),
    Albums("albums", "album", "Alben"),
    Playlists("playlists", "playlist", "Playlists"),
    Artists("artists", "artist", "Artists"),
}

/** Owns eight request-local searches and their asynchronous native requests. */
internal class MediaLibrarySearch(private val queryCore: (String, String?) -> ListenableFuture<String>) {
    private class CategoryRead {
        val firstValue = SettableFuture.create<String>()
        var currentValue: String? = null
        var currentResult: ListenableFuture<String> = firstValue
        var request: ListenableFuture<*>? = null
        var error: Throwable? = null
    }
    private class SearchRead {
        val categories = SearchMediaCategory.entries.associateWith { CategoryRead() }.toMutableMap()
        val results = categories.mapValues { it.value.firstValue as ListenableFuture<String> }.toMutableMap()
    }
    private val searches = LinkedHashMap<String, SearchRead>()
    private var isClosed = false
    var onResultsChanged: ((String, SearchMediaCategory, ListenableFuture<String>, Int) -> Unit)? = null
    var onSearchInvalidated: ((String) -> Unit)? = null

    // Search request lifecycle.
    @Synchronized
    fun close() {
        isClosed = true
        searches.values.forEach(::cancel)
        searches.clear()
        onResultsChanged = null
        onSearchInvalidated = null
    }

    @Synchronized
    fun invalidate() {
        val retired = searches.toMap()
        searches.clear()
        retired.forEach { (query, read) ->
            cancel(read)
            onSearchInvalidated?.invoke(query)
        }
    }

    @Synchronized
    fun release(query: String) {
        searches.remove(query)?.let(::cancel)
    }

    // Queries share one request per media type, including in-flight requests.
    @Synchronized
    fun start(query: String): Map<SearchMediaCategory, ListenableFuture<String>> {
        check(!isClosed) { "Media library search is closed" }
        if (query.isBlank()) return emptyMap()
        searches[query]?.let { return it.results }
        val read = SearchRead()
        searches[query] = read
        read.categories.forEach { (category, categoryRead) ->
            startCategory(query, read, category, categoryRead)
        }
        if (searches.size > 8) {
            val oldestQuery = searches.keys.first()
            searches.remove(oldestQuery)?.let(::cancel)
        }
        return read.results
    }

    @Synchronized
    fun retryFailed(query: String, categoryToRetry: SearchMediaCategory? = null) {
        check(!isClosed) { "Media library search is closed" }
        val read = searches[query] ?: return
        read.categories.toMap().forEach { (category, previous) ->
            if (previous.error != null && (categoryToRetry == null || categoryToRetry == category)) {
                val replacement = CategoryRead().apply {
                    currentValue = previous.currentValue
                    currentValue?.let { firstValue.set(it) }
                }
                read.categories[category] = replacement
                read.results[category] = replacement.firstValue
                previous.request?.cancel(false)
                startCategory(query, read, category, replacement)
            }
        }
    }

    // Current results and callback ownership.
    @Synchronized
    fun results(query: String, category: SearchMediaCategory): ListenableFuture<String> {
        val first = start(query)[category] ?: return Futures.immediateFuture("[]")
        return searches[query]?.categories?.get(category)?.currentResult ?: first
    }

    @Synchronized
    fun isCurrent(query: String, category: SearchMediaCategory, result: ListenableFuture<String>): Boolean =
        !isClosed && searches[query]?.results?.get(category) === result

    // Values remain request-local; later refreshes replace only the matching category.
    @Synchronized
    fun onCatalogEvent(event: JSONObject) {
        if (isClosed) return
        val arguments = event.optJSONObject("query")?.optJSONObject("Search") ?: return
        if (arguments.optInt("limit") != 50) return
        val query = arguments.optString("query")
        val read = searches[query] ?: return
        val category = SearchMediaCategory.entries.firstOrNull { it.mediaType == arguments.optString("media_type") } ?: return
        if (event.optString("type") == "catalog_read_changed" &&
            event.optJSONObject("snapshot")?.optString("phase") == "invalidated") {
            searches.remove(query)
            cancel(read)
            onSearchInvalidated?.invoke(query)
            return
        }
        if (event.optString("type") != "catalog_updated") return
        val items = event.optJSONObject("value")?.optJSONObject("Search")?.optJSONArray(category.resultKey) ?: return
        accept(query, read, category, items.toString())
    }

    // Category ownership fences late completions while independent retries keep cached results.
    private fun startCategory(query: String, read: SearchRead, category: SearchMediaCategory, categoryRead: CategoryRead) {
        try {
            val argument = JSONObject().put("query", query).put("mediaType", category.mediaType).put("limit", 50).toString()
            val response = queryCore("searchMedia", argument)
            categoryRead.request = response
            Futures.addCallback(response, object : FutureCallback<String> {
                override fun onSuccess(value: String?) {
                    try {
                        accept(query, read, category, JSONObject(checkNotNull(value)).getJSONArray(category.resultKey).toString(), categoryRead)
                    } catch (error: Exception) { fail(query, read, category, error, categoryRead) }
                }
                override fun onFailure(error: Throwable) = fail(query, read, category, error, categoryRead)
            }, MoreExecutors.directExecutor())
        } catch (error: Exception) {
            Log.w("MashinLibrarySearch", "${category.mediaType} search failed", error)
            fail(query, read, category, error, categoryRead)
        }
    }

    // Request completion cannot overwrite values owned by an evicted or closed search.
    @Synchronized
    private fun accept(query: String, read: SearchRead, category: SearchMediaCategory, value: String,
        expected: CategoryRead = read.categories.getValue(category)) {
        if (isClosed || searches[query] !== read || read.categories[category] !== expected) return
        val categoryRead = read.categories.getValue(category)
        categoryRead.error = null
        if (categoryRead.currentValue == value) return
        categoryRead.currentValue = value
        val wasPublished = categoryRead.firstValue.isDone
        if (wasPublished) categoryRead.currentResult = Futures.immediateFuture(value)
        else {
            categoryRead.currentResult = categoryRead.firstValue
            categoryRead.firstValue.set(value)
        }
        if (wasPublished) onResultsChanged?.invoke(query, category, categoryRead.firstValue, JSONArray(value).length())
    }

    @Synchronized
    private fun fail(query: String, read: SearchRead, category: SearchMediaCategory, error: Throwable,
        expected: CategoryRead = read.categories.getValue(category)) {
        if (isClosed || searches[query] !== read || read.categories[category] !== expected) return
        val categoryRead = read.categories.getValue(category)
        categoryRead.error = error
        if (categoryRead.currentValue == null) categoryRead.firstValue.setException(error)
    }

    // Canceling the request future revokes the native session-owned query.
    private fun cancel(read: SearchRead) {
        read.categories.values.forEach {
            it.firstValue.cancel(false)
            it.request?.cancel(false)
        }
    }
}

