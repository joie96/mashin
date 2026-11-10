package com.joie96.mashin.media.library

import com.google.common.util.concurrent.FutureCallback
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.MoreExecutors
import com.google.common.util.concurrent.SettableFuture
import org.json.JSONArray
import org.json.JSONObject

internal enum class LibraryReadKind(val command: String, val catalog: String, val value: String, val mediaType: String) {
    Recent("recentlyPlayedTracks", "RecentlyPlayed", "Tracks", "track"),
    AlbumTracks("albumTracks", "AlbumTracks", "Tracks", "track"),
    PlaylistTracks("playlistTracks", "PlaylistTracks", "Tracks", "track"),
    ArtistAlbums("artistAlbums", "ArtistAlbums", "Albums", "album"),
    ArtistTopTracks("artistTopTracks", "ArtistTopTracks", "Tracks", "track"),
    SimilarArtists("similarArtists", "SimilarArtists", "Artists", "artist"),
    HomeRecommendations("recommendations", "", "", "home"),
    ExploreRecommendations("recommendations", "", "", "explore"),
}

internal data class LibraryRead(val kind: LibraryReadKind, val itemId: String? = null, val provider: String? = null)

/** Request-local observations only; the Rust catalog remains the sole persistent library owner. */
internal class MediaLibraryReads(private val query: (String, String?) -> ListenableFuture<String>) {
    private class Observation {
        val firstValue = SettableFuture.create<String>()
        var currentValue: String? = null
        var request: ListenableFuture<String>? = null
        var error: Throwable? = null
        var hasObservedFolders = false
    }
    private val observations = mutableMapOf<LibraryRead, Observation>()
    private var isClosed = false
    var onChanged: ((LibraryRead) -> Unit)? = null
    var onInvalidated: ((LibraryRead) -> Unit)? = null

    // Lifecycle and last-consumer release.
    @Synchronized
    fun close() {
        isClosed = true
        invalidate()
        onChanged = null
        onInvalidated = null
    }

    @Synchronized
    fun invalidate() {
        observations.keys.toList().forEach(::release)
    }

    @Synchronized
    fun release(read: LibraryRead) {
        val observation = observations.remove(read) ?: return
        observation.firstValue.cancel(false)
        observation.request?.cancel(false)
    }

    // Current values are independent of the still-running native refresh.
    @Synchronized
    fun results(read: LibraryRead): ListenableFuture<String> {
        check(!isClosed) { "Library reads are closed" }
        val observation = observations[read] ?: Observation().also { created ->
            observations[read] = created
            val argument = JSONObject().apply {
                if (read.kind.command == "recommendations") put("view", read.kind.mediaType)
                if (read.kind == LibraryReadKind.Recent || read.kind == LibraryReadKind.SimilarArtists) put("limit", 50)
                read.itemId?.let { put("itemId", it) }
                read.provider?.let { put("provider", it) }
            }
            try {
                val request = query(read.kind.command, argument.toString())
                created.request = request
                Futures.addCallback(request, object : FutureCallback<String> {
                    override fun onSuccess(value: String?) {
                        try {
                            val parsed = if (read.kind.command == "recommendations") JSONObject(checkNotNull(value)).toString()
                                else JSONArray(checkNotNull(value)).toString()
                            accept(read, created, parsed)
                        }
                        catch (error: Exception) { fail(read, created, error) }
                    }
                    override fun onFailure(error: Throwable) = fail(read, created, error)
                }, MoreExecutors.directExecutor())
            } catch (error: Exception) { fail(read, created, error) }
        }
        return observation.currentValue?.let(Futures::immediateFuture) ?: observation.firstValue
    }

    @Synchronized
    fun failure(read: LibraryRead): Throwable? = observations[read]?.error

    @Synchronized
    fun hasObservedFolders(read: LibraryRead): Boolean = observations[read]?.hasObservedFolders == true

    // Native tagged catalog envelopes are decoded only at this boundary.
    @Synchronized
    fun onCoreEvent(event: JSONObject) {
        if (event.optString("type") == "recommendations_updated") {
            observations.toMap().forEach { (read, observation) ->
                if (read.kind.command == "recommendations" && read.kind.mediaType == event.optString("view")) {
                    event.optJSONObject("snapshot")?.let { accept(read, observation, it.toString()) }
                }
            }
            return
        }
        val query = when (val identity = event.opt("query")) {
            is JSONObject -> identity
            is String -> JSONObject().put(identity, JSONObject())
            else -> return
        }
        if (query.has("RecommendationFolders") && event.optJSONObject("snapshot")?.optString("knowledge") == "observed_response") {
            observations.toMap().forEach { (read, observation) ->
                if (read.kind.command == "recommendations" && !observation.hasObservedFolders) {
                    observation.hasObservedFolders = true
                    onChanged?.invoke(read)
                }
            }
        }
        observations.toMap().forEach { (read, observation) ->
            if (read.kind.command == "recommendations") {
                if (event.optString("type") == "catalog_read_changed" &&
                    event.optJSONObject("snapshot")?.optString("phase") == "invalidated" &&
                    observesRecommendationQuery(observation.currentValue, query)) {
                    release(read)
                    onInvalidated?.invoke(read)
                }
                return@forEach
            }
            val identity = query.optJSONObject(read.kind.catalog) ?: return@forEach
            if (read.kind == LibraryReadKind.Recent) {
                if (identity.optInt("limit", -1) != 50) return@forEach
            } else if (identity.optString("item_id") != read.itemId || identity.optString("provider") != read.provider) return@forEach
            if (read.kind == LibraryReadKind.SimilarArtists && identity.optInt("limit", -1) != 50) return@forEach
            if (read.kind == LibraryReadKind.PlaylistTracks && identity.optBoolean("allow_dynamic_tracks")) return@forEach
            if (event.optString("type") == "catalog_read_changed" && event.optJSONObject("snapshot")?.optString("phase") == "invalidated") {
                release(read)
                onInvalidated?.invoke(read)
            } else if (event.optString("type") == "catalog_updated") {
                event.optJSONObject("value")?.optJSONArray(read.kind.value)?.let { accept(read, observation, it.toString()) }
            }
        }
    }

    // Owner identity rejects terminal results from canceled or replaced requests.
    private fun observesRecommendationQuery(value: String?, query: JSONObject): Boolean {
        if (query.has("RecommendationFolders")) return true
        val folderQuery = query.optJSONObject("RecommendationItems")
        val playlistQuery = query.optJSONObject("PlaylistTracks")
        if (folderQuery == null && playlistQuery == null) return false
        val folders = value?.let(::JSONObject)?.optJSONArray("folders") ?: return false
        for (index in 0 until folders.length()) {
            val folder = folders.getJSONObject(index)
            if (folderQuery != null && sameReference(folder, folderQuery)) return true
            val items = folder.optJSONArray("items") ?: continue
            if (playlistQuery != null) for (itemIndex in 0 until items.length()) {
                val item = items.getJSONObject(itemIndex)
                if (item.optString("media_type") == "playlist" && sameReference(item, playlistQuery)) return true
            }
        }
        return false
    }

    private fun sameReference(item: JSONObject, query: JSONObject): Boolean =
        item.optString("item_id") == query.optString("item_id") && item.optString("provider") == query.optString("provider")

    @Synchronized
    private fun accept(read: LibraryRead, observation: Observation, value: String) {
        if (isClosed || observations[read] !== observation || observation.currentValue == value) return
        observation.currentValue = value
        observation.error = null
        observation.firstValue.set(value)
        onChanged?.invoke(read)
    }

    @Synchronized
    private fun fail(read: LibraryRead, observation: Observation, error: Throwable) {
        if (!isClosed && observations[read] === observation) {
            observation.error = error
            if (observation.currentValue == null) observation.firstValue.setException(error)
            onChanged?.invoke(read)
        }
    }
}