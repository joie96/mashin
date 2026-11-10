package com.joie96.mashin.media.library

import android.content.ContentResolver
import android.content.Context
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaConstants
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession
import com.joie96.mashin.core.RustCoreBridge
import com.joie96.mashin.core.NativeQueryRequests
import com.joie96.mashin.media.artwork.MediaArtworkContentProvider
import com.joie96.mashin.media.session.PlaybackCommandKind
import com.joie96.mashin.media.session.PlaybackRequest
import com.joie96.mashin.R
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.MoreExecutors
import com.google.common.util.concurrent.SettableFuture
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import org.json.JSONArray
import org.json.JSONObject

// Online media-library browsing and Media3 playback identity mapping.
@UnstableApi
internal class MediaLibraryBrowser(
    private val context: Context,
    search: MediaLibrarySearch? = null,
    private val query: (String, String?) -> String? = { kind, argument -> RustCoreBridge.query(kind, argument) },
    private val queryRequests: NativeQueryRequests = NativeQueryRequests(),
    private val catalogQuery: (String, String?) -> ListenableFuture<String> = queryRequests::execute,
    private val dispatchCommand: (PlaybackRequest) -> ListenableFuture<*>,
) : MediaLibraryService.MediaLibrarySession.Callback {
    private val search = search ?: MediaLibrarySearch(catalogQuery)
    private val catalogReads = MediaLibraryReads(catalogQuery)
    private val parentReads = ConcurrentHashMap<String, LibraryRead>()
    private val parentConsumers = ConcurrentHashMap<MediaSession.ControllerInfo, MutableSet<String>>()
    private val subscriptions = ConcurrentHashMap<MediaSession.ControllerInfo, MutableSet<String>>()
    private val controllerSearches = ConcurrentHashMap<MediaSession.ControllerInfo, String>()
    private val activeRequests = ConcurrentHashMap<ListenableFuture<*>, Pair<MediaSession.ControllerInfo, String>>()
    private val deliveredValues = ConcurrentHashMap<String, List<String>>()
    private val readRevision = AtomicLong()
    private val browseExecutor = MoreExecutors.listeningDecorator(Executors.newSingleThreadExecutor())
    private val mainHandler = Handler(Looper.getMainLooper())
    private var isClosed = false
    private val libraryItemsById = ConcurrentHashMap<String, MediaItem>()
    private val serializedMediaItems = ConcurrentHashMap<String, String>()
    private val serializedTrackLists = ConcurrentHashMap<String, String>()
    private val identifierSequence = AtomicLong()
    private val deliveredParents = ConcurrentHashMap<String, List<String>>()
    private var activeSession = java.lang.ref.WeakReference<MediaLibraryService.MediaLibrarySession>(null)
    private data class LibraryChildren(val items: List<MediaItem>, val isLoading: Boolean = false)
    private data class PendingBrowse(
        val parentId: String,
        val page: Int,
        val pageSize: Int,
        val params: MediaLibraryService.LibraryParams?,
        val result: SettableFuture<LibraryResult<ImmutableList<MediaItem>>>,
    )
    private val pendingBrowses = ConcurrentHashMap.newKeySet<PendingBrowse>()

    init {
        catalogReads.onChanged = { read -> updateParents(parentReads.filterValues { it == read }.keys) }
        catalogReads.onInvalidated = { read ->
            val revision = readRevision.get()
            mainHandler.post { if (!isClosed && revision == readRevision.get()) {
                parentReads.filterValues { it == read }.keys.forEach { parent ->
                    activeRequests.filterValues { it.second == parent }.keys.toList().forEach { it.cancel(false) }
                    deliveredValues.remove(parent)
                    activeSession.get()?.notifyChildrenChanged(parent, 0, null)
                }
            } }
        }
        this.search.onResultsChanged = { query, category, result, itemCount ->
            mainHandler.post {
                if (!isClosed && this.search.isCurrent(query, category, result)) {
                    activeSession.get()?.notifyChildrenChanged("search:${category.resultKey}?$query",
                        itemCount + if (category == SearchMediaCategory.Tracks && itemCount > 0) 2 else 0, null)
                }
            }
        }
        this.search.onSearchInvalidated = { query ->
            mainHandler.post {
                if (!isClosed) SearchMediaCategory.entries.forEach { category ->
                    activeSession.get()?.notifyChildrenChanged("search:${category.resultKey}?$query", 0, null)
                }
            }
        }
    }

    // Browser and controller lifecycle.
    fun close() {
        isClosed = true
        readRevision.incrementAndGet()
        pendingBrowses.forEach { it.result.cancel(false) }
        pendingBrowses.clear()
        deliveredParents.clear()
        deliveredValues.clear()
        parentConsumers.clear()
        subscriptions.clear()
        controllerSearches.clear()
        activeRequests.keys.toList().forEach { it.cancel(false) }
        parentReads.clear()
        activeSession.clear()
        search.close()
        catalogReads.close()
        queryRequests.close()
        browseExecutor.shutdown()
    }

    override fun onDisconnected(session: MediaSession, controller: MediaSession.ControllerInfo) {
        subscriptions.remove(controller)
        parentConsumers[controller]?.toList()?.forEach { releaseParent(controller, it) }
        controllerSearches.remove(controller)?.let(::releaseSearchIfUnused)
        activeRequests.filterValues { it.first == controller }.keys.toList().forEach { it.cancel(false) }
    }

    override fun onSubscribe(session: MediaLibraryService.MediaLibrarySession, browser: MediaSession.ControllerInfo,
        parentId: String, params: MediaLibraryService.LibraryParams?): ListenableFuture<LibraryResult<Void>> {
        if (isClosed) return Futures.immediateFuture(LibraryResult.ofError(LibraryResult.RESULT_ERROR_UNKNOWN))
        activeSession = java.lang.ref.WeakReference(session)
        subscriptions.computeIfAbsent(browser) { ConcurrentHashMap.newKeySet() }.add(parentId)
        retainParent(browser, parentId)
        return Futures.immediateFuture(LibraryResult.ofVoid(params))
    }

    override fun onUnsubscribe(session: MediaLibraryService.MediaLibrarySession, browser: MediaSession.ControllerInfo,
        parentId: String): ListenableFuture<LibraryResult<Void>> {
        subscriptions[browser]?.remove(parentId)
        releaseParent(browser, parentId)
        activeRequests.filterValues { it.first == browser && it.second == parentId }.keys.toList().forEach { it.cancel(false) }
        return Futures.immediateFuture(LibraryResult.ofVoid(null))
    }

    // Rust events retry only requests that are still waiting for their snapshot data.
    fun onCoreEvent(eventType: String, proxyId: String? = null, artworkRevision: Long? = null, catalogEvent: JSONObject? = null) {
        if (isClosed) return
        if (catalogEvent != null) queryRequests.onCoreEvent(catalogEvent)
        if (catalogEvent != null) search.onCatalogEvent(catalogEvent)
        if (catalogEvent != null) catalogReads.onCoreEvent(catalogEvent)
        if ((eventType == "cache_cleared" && artworkRevision == null) || eventType in setOf("provider_instances_changed", "session_changed")) {
            readRevision.incrementAndGet()
            activeRequests.keys.toList().forEach { it.cancel(false) }
            search.invalidate()
            catalogReads.invalidate()
            queryRequests.invalidate()
            parentReads.clear()
            deliveredValues.clear()
            serializedMediaItems.clear()
            serializedTrackLists.clear()
            libraryItemsById.clear()
            deliveredParents.keys.forEach { parent -> activeSession.get()?.notifyChildrenChanged(parent, 0, null) }
            if (eventType == "session_changed") {
                controllerSearches.clear()
                deliveredParents.clear()
            }
        }
        if (artworkRevision != null) {
            val changed = mutableSetOf<String>()
            libraryItemsById.replaceAll { id, item ->
                val uri = MediaArtworkContentProvider.reviseUri(item.mediaMetadata.artworkUri, proxyId, artworkRevision)
                if (uri == null) item else {
                    changed.add(id)
                    item.buildUpon().setMediaMetadata(item.mediaMetadata.buildUpon().setArtworkUri(uri).build()).build()
                }
            }
            deliveredParents.forEach { (parent, children) ->
                if (children.any(changed::contains)) activeSession.get()?.notifyChildrenChanged(parent, children.size, null)
            }
        }
        if (eventType == "cache_cleared" && artworkRevision == null) {
            deliveredParents.forEach { (parent, children) -> activeSession.get()?.notifyChildrenChanged(parent, children.size, null) }
        }
        if (eventType !in setOf("recommendations_changed", "user_data_changed", "cache_cleared")) return
        updateParents((pendingBrowses.map { it.parentId } + deliveredParents.keys).filter { parent ->
            if (eventType == "user_data_changed") parent.startsWith("favorites:") || parent == "node:playlists" || parent.startsWith("browse:playlist:")
            else parent.startsWith("home:") || parent.startsWith("discover:")
        }.toSet())
    }

    // Controller media selection and canonical playback requests.
    override fun onSetMediaItems(
        mediaSession: MediaSession,
        controller: MediaSession.ControllerInfo,
        mediaItems: List<MediaItem>,
        startIndex: Int,
        startPositionMs: Long,
    ): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
        val resolvedItems = mediaItems.map { item ->
            (libraryItemsById[item.mediaId] ?: item).buildUpon()
                .setUri(android.net.Uri.Builder().scheme("mashin").authority("media").appendPath(item.mediaId).build())
                .build()
        }
        return Futures.immediateFuture(MediaSession.MediaItemsWithStartPosition(resolvedItems, startIndex, startPositionMs))
    }

    fun startPlayback(mediaId: String): ListenableFuture<*> {
        if (mediaId.startsWith("artistRadio:")) {
            val artist = serializedMediaItems[mediaId.removePrefix("artistRadio:")]
                ?: return Futures.immediateFailedFuture<Any>(IllegalArgumentException("unknown Android media ID: $mediaId"))
            return dispatchCommand(PlaybackRequest(PlaybackCommandKind.StartRadio,
                JSONObject().put("mediaItems", JSONArray().put(JSONObject(artist))).toString()))
        }
        serializedTrackLists[mediaId]?.let { serializedItems ->
            return dispatchCommand(PlaybackRequest(
                PlaybackCommandKind.QueuePlayMedia,
                JSONObject().put("request", JSONObject()
                    .put("mediaItems", JSONArray(serializedItems))
                    .put("option", "play")
                    .put("shuffle", mediaId.startsWith("action:shuffle:")))
                    .toString(),
            ))
        }
        serializedMediaItems[mediaId]?.let { serializedItem ->
            if (JSONObject(serializedItem).optString("media_type") == "track") {
                return dispatchCommand(PlaybackRequest(PlaybackCommandKind.StartRadio,
                    JSONObject().put("mediaItems", JSONArray().put(JSONObject(serializedItem))).toString()))
            }
            return dispatchCommand(PlaybackRequest(
                PlaybackCommandKind.QueuePlayMedia,
                JSONObject().put("request", JSONObject().put("mediaItems", JSONArray().put(JSONObject(serializedItem)))
                    .put("option", "play")).toString(),
            ))
        }
        return Futures.immediateFailedFuture<Any>(IllegalArgumentException("unknown Android media ID: $mediaId"))
    }

    // Library queries and search results.
    override fun onGetLibraryRoot(
        session: MediaLibraryService.MediaLibrarySession,
        browser: MediaSession.ControllerInfo,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<MediaItem>> {
        return Futures.immediateFuture(LibraryResult.ofItem(libraryItem("root", "Mashin", false), params))
    }

    override fun onGetChildren(
        session: MediaLibraryService.MediaLibrarySession,
        browser: MediaSession.ControllerInfo,
        parentId: String,
        page: Int,
        pageSize: Int,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<com.google.common.collect.ImmutableList<MediaItem>>> {
        if (isClosed || page < 0 || pageSize <= 0) return Futures.immediateFuture(LibraryResult.ofError(LibraryResult.RESULT_ERROR_BAD_VALUE))
        activeSession = java.lang.ref.WeakReference(session)
        retainParent(browser, parentId)
        if (parentId in setOf("root", "node:home", "node:discover", "node:favorites") ||
            parentId.startsWith("browse:artist:")) {
            val items = libraryItems(parentId).items
            deliveredParents[parentId] = items.map { it.mediaId }
            return Futures.immediateFuture(LibraryResult.ofItemList(pageItems(items, page, pageSize), params))
        }
        if (parentId.startsWith("search:")) {
            return trackRequest(browser, parentId, searchCategoryItems(parentId, page, pageSize, params))
        }
        val result = SettableFuture.create<LibraryResult<ImmutableList<MediaItem>>>()
        val request = PendingBrowse(parentId, page, pageSize, params, result)
        pendingBrowses.add(request)
        result.addListener({ pendingBrowses.remove(request) }, MoreExecutors.directExecutor())
        browseExecutor.execute { completeBrowse(request) }
        return trackRequest(browser, parentId, result)
    }

    override fun onGetItem(
        session: MediaLibraryService.MediaLibrarySession,
        browser: MediaSession.ControllerInfo,
        mediaId: String,
    ): ListenableFuture<LibraryResult<MediaItem>> = Futures.immediateFuture(
        libraryItemsById[mediaId]?.let { LibraryResult.ofItem(it, null) }
            ?: LibraryResult.ofError(LibraryResult.RESULT_ERROR_BAD_VALUE),
    )

    override fun onSearch(
        session: MediaLibraryService.MediaLibrarySession,
        browser: MediaSession.ControllerInfo,
        query: String,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<Void>> {
        activeSession = java.lang.ref.WeakReference(session)
        controllerSearches.put(browser, query)?.takeIf { it != query }?.let { previous ->
            parentConsumers[browser]?.toList()?.filter {
                it.startsWith("search:") && it.substringAfter('?', "") == previous
            }?.forEach { parent ->
                subscriptions[browser]?.remove(parent)
                releaseParent(browser, parent)
                activeRequests.filterValues { it.first == browser && it.second == parent }.keys.toList().forEach { it.cancel(false) }
            }
            releaseSearchIfUnused(previous)
        }
        val results = search.start(query)
        session.notifySearchResultChanged(browser, query, results.size, params)
        results.forEach { (category, result) ->
            result.addListener({
                mainHandler.post {
                    if (!isClosed && !result.isCancelled && search.isCurrent(query, category, result)) {
                        val itemCount = try {
                            JSONArray(Futures.getDone(result)).length()
                        } catch (_: Exception) {
                            return@post
                        }
                        session.notifyChildrenChanged(browser, "search:${category.resultKey}?$query",
                            itemCount + if (category == SearchMediaCategory.Tracks && itemCount > 0) 2 else 0, params)
                    }
                }
            }, MoreExecutors.directExecutor())
        }
        return Futures.immediateFuture(LibraryResult.ofVoid(params))
    }

    override fun onGetSearchResult(
        session: MediaLibraryService.MediaLibrarySession,
        browser: MediaSession.ControllerInfo,
        query: String,
        page: Int,
        pageSize: Int,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<com.google.common.collect.ImmutableList<MediaItem>>> {
        if (page < 0 || pageSize <= 0) return Futures.immediateFuture(LibraryResult.ofError(LibraryResult.RESULT_ERROR_BAD_VALUE))
        return Futures.immediateFuture(LibraryResult.ofItemList(searchItems(query).drop(
            (page.toLong() * pageSize).coerceAtMost(Int.MAX_VALUE.toLong()).toInt()).take(pageSize), params))
    }

    fun resolveLibraryMediaItem(mediaId: String): JSONObject? = serializedMediaItems[mediaId]?.let(::JSONObject)

    // An unfinished future keeps the host's loading UI active without blocking the browse worker.
    private fun completeBrowse(request: PendingBrowse) {
        if (isClosed || request.result.isDone) return
        val revision = readRevision.get()
        try {
            val children = libraryItems(request.parentId)
            if (revision != readRevision.get()) return
            if (!children.isLoading || children.items.isNotEmpty()) {
                deliveredParents[request.parentId] = children.items.map { it.mediaId }
                deliveredValues[request.parentId] = contentIdentity(children.items)
                request.result.set(LibraryResult.ofItemList(pageItems(children.items, request.page, request.pageSize), request.params))
            }
        } catch (error: Exception) {
            request.result.set(LibraryResult.ofError(LibraryResult.RESULT_ERROR_UNKNOWN))
        }
    }

    // Owners survive first delivery until unsubscribe, disconnect, replacement or cancellation.
    private fun retainParent(controller: MediaSession.ControllerInfo, parent: String) {
        parentConsumers.computeIfAbsent(controller) { ConcurrentHashMap.newKeySet() }.add(parent)
    }

    private fun releaseParent(controller: MediaSession.ControllerInfo, parent: String) {
        parentConsumers[controller]?.remove(parent)
        if (parentConsumers.values.any { parent in it }) return
        val read = parentReads.remove(parent)
        if (read != null && read !in parentReads.values) catalogReads.release(read)
        deliveredParents.remove(parent)
        deliveredValues.remove(parent)
        if (parent.startsWith("search:")) releaseSearchIfUnused(parent.substringAfter('?', ""))
    }

    private fun releaseSearchIfUnused(query: String) {
        if (query in controllerSearches.values || parentConsumers.values.any { parents ->
            parents.any { it.startsWith("search:") && it.substringAfter('?', "") == query }
        }) return
        search.release(query)
    }

    private fun <Value> trackRequest(controller: MediaSession.ControllerInfo, parent: String,
        result: ListenableFuture<Value>): ListenableFuture<Value> {
        activeRequests[result] = controller to parent
        result.addListener({
            activeRequests.remove(result)
            if (result.isCancelled && subscriptions[controller]?.contains(parent) != true &&
                activeRequests.values.none { it.first == controller && it.second == parent }) {
                releaseParent(controller, parent)
            }
        }, MoreExecutors.directExecutor())
        return result
    }

    private fun updateParents(parents: Set<String>) {
        if (isClosed) return
        val revision = readRevision.get()
        browseExecutor.execute {
            if (isClosed || revision != readRevision.get()) return@execute
            pendingBrowses.filter { it.parentId in parents }.forEach(::completeBrowse)
            parents.filter(deliveredParents::containsKey).forEach { parent ->
                try {
                    val children = libraryItems(parent)
                    if (revision != readRevision.get() || (children.isLoading && children.items.isEmpty())) return@forEach
                    val identity = contentIdentity(children.items)
                    if (deliveredValues[parent] == identity) return@forEach
                    deliveredValues[parent] = identity
                    deliveredParents[parent] = children.items.map { it.mediaId }
                    mainHandler.post { if (!isClosed && revision == readRevision.get() && deliveredParents.containsKey(parent)) {
                        activeSession.get()?.notifyChildrenChanged(parent, children.items.size, null)
                    } }
                } catch (_: Exception) { /* A refresh failure retains already delivered media. */ }
            }
        }
    }

    private fun contentIdentity(items: List<MediaItem>): List<String> = items.filterNot { it.mediaId.startsWith("action:") }
        .map { serializedMediaItems[it.mediaId] ?: it.mediaId }

    private fun pageItems(items: List<MediaItem>, page: Int, pageSize: Int): List<MediaItem> =
        items.drop((page.toLong() * pageSize).coerceAtMost(Int.MAX_VALUE.toLong()).toInt()).take(pageSize)

    private fun catalogValue(parentId: String, read: LibraryRead): String? {
        parentReads[parentId] = read
        val value = catalogReads.results(read)
        return if (value.isDone) Futures.getDone(value) else null
    }

    // Library boundary mapping and serialized playback references.
    private fun libraryItem(
        id: String,
        title: String,
        playable: Boolean,
        subtitle: String? = null,
        artworkUri: String? = null,
        browsableStyle: Int? = null,
        iconResource: Int? = null,
    ): MediaItem = MediaItem.Builder()
        .setMediaId(id)
        .setMediaMetadata(
            MediaMetadata.Builder()
                .setTitle(title)
                .setSubtitle(subtitle)
                .setArtworkUri(artworkUri?.let(android.net.Uri::parse) ?: iconResource?.let(::resourceUri))
                .setIsBrowsable(!playable)
                .setIsPlayable(playable)
                .setExtras(browsableStyle?.let { style ->
                    Bundle().apply { putInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE, style) }
                })
                .build(),
        )
        .build()
        .also { libraryItemsById[id] = it }

    private fun resourceUri(resourceId: Int): android.net.Uri = android.net.Uri.Builder()
        .scheme(ContentResolver.SCHEME_ANDROID_RESOURCE)
        .authority(context.resources.getResourcePackageName(resourceId))
        .appendPath(context.resources.getResourceTypeName(resourceId))
        .appendPath(context.resources.getResourceEntryName(resourceId))
        .build()

    private fun libraryItems(parentId: String): LibraryChildren = when (parentId) {
        "root" -> LibraryChildren(listOf(
            libraryItem("node:home", "Home", false, iconResource = R.drawable.home),
            libraryItem("node:discover", "Entdecken", false, iconResource = R.drawable.explore),
            libraryItem("node:playlists", "Playlists", false, browsableStyle = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM, iconResource = R.drawable.playlist_play),
            libraryItem("node:favorites", "Favoriten", false, iconResource = R.drawable.favorite),
        ))
        "node:home" -> LibraryChildren(homeItems())
        "node:discover" -> LibraryChildren(discoverItems())
        "node:playlists" -> playlistItems()
        "node:favorites" -> LibraryChildren(favoriteCategoryItems())
        "home:recommendations" -> recommendationItems(parentId, "user_recommendations", "track")
        "home:favoriteGenres" -> recommendationItems(parentId, "user_genre_playlists_monthly", "playlist", 9, true)
        "home:playlistSuggestions" -> recommendationItems(parentId, "user_artist_playlists_monthly", "playlist", normalizePlaylistNames = true)
        "home:topTracks" -> recommendationItems(parentId, "user_top_tracks_monthly", "track")
        "home:topArtists" -> recommendationItems(parentId, "user_top_artists_monthly", "artist")
        "home:recent" -> catalogValue(parentId, LibraryRead(LibraryReadKind.Recent))?.let {
            LibraryChildren(mediaItemsFromJson(it, true, mediaType = "track"))
        } ?: LibraryChildren(emptyList(), true)
        "discover:exploration" -> discoverRecommendationItems(parentId, "user_exploration_weekly", "track", true)
        "discover:albums" -> discoverRecommendationItems(parentId, "sitewide_top_albums_weekly", "album")
        "discover:similarUsers" -> discoverRecommendationItems(parentId, "similarusers_jam_weekly", "track", true)
        "discover:artists" -> discoverRecommendationItems(parentId, "sitewide_top_artists_weekly", "artist")
        "discover:tracks" -> discoverRecommendationItems(parentId, "sitewide_top_tracks_weekly", "track")
        "favorites:tracks" -> userDataItems("tracks", "track", true)
        "favorites:albums" -> userDataItems("albums", "album")
        "favorites:playlists" -> userDataItems("playlists", "playlist")
        "favorites:artists" -> userDataItems("artists", "artist")
        else -> dynamicLibraryChildren(parentId)
    }

    private fun homeItems(): List<MediaItem> = listOf(
        libraryItem("home:recommendations", "Empfehlungen", false, iconResource = R.drawable.music_note_2),
        libraryItem("home:favoriteGenres", "Lieblingsgenres", false, iconResource = R.drawable.genres),
        libraryItem("home:playlistSuggestions", "Playlist-Empfehlungen", false, iconResource = R.drawable.playlist_play),
        libraryItem("home:topTracks", "Top-Tracks", false, iconResource = R.drawable.music_note),
        libraryItem("home:topArtists", "Top-Artists", false, browsableStyle = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM, iconResource = R.drawable.artist),
        libraryItem("home:recent", "Kürzlich gespielt", false, iconResource = R.drawable.music_history),
    )

    private fun favoriteCategoryItems(): List<MediaItem> = listOf(
        libraryItem("favorites:tracks", "Tracks", false, iconResource = R.drawable.music_note),
        libraryItem("favorites:albums", "Alben", false, browsableStyle = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM, iconResource = R.drawable.album),
        libraryItem("favorites:playlists", "Playlists", false, browsableStyle = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM, iconResource = R.drawable.playlist_play),
        libraryItem("favorites:artists", "Artists", false, browsableStyle = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM, iconResource = R.drawable.artist),
    )

    private fun discoverItems(): List<MediaItem> = listOf(
        libraryItem("discover:exploration", "Entdecke etwas Neues", false, iconResource = R.drawable.music_note_2),
        libraryItem("discover:albums", "Alben der Woche", false, browsableStyle = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM, iconResource = R.drawable.album),
        libraryItem("discover:similarUsers", "Andere Nutzer hören", false, iconResource = R.drawable.groups),
        libraryItem("discover:artists", "Artists der Woche", false, browsableStyle = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM, iconResource = R.drawable.artist),
        libraryItem("discover:tracks", "Songs der Woche", false, iconResource = R.drawable.music_note),
    )

    private fun playlistItems(): LibraryChildren = privatePlaylistItems()

    // Exploration and similar-user recommendations contain playlists rather than direct tracks.
    private fun discoverRecommendationItems(
        parentId: String,
        folderId: String,
        mediaType: String,
        expandPlaylistTracks: Boolean = false,
    ): LibraryChildren {
        val snapshot = JSONObject(catalogValue(parentId, LibraryRead(LibraryReadKind.ExploreRecommendations))
            ?: return LibraryChildren(emptyList(), true))
        val folders = snapshot.getJSONArray("folders")
        val selected = JSONArray()
        var hasMatchingFolder = false
        var isLoading = false
        for (folderIndex in 0 until folders.length()) {
            val folder = folders.optJSONObject(folderIndex) ?: continue
            if (folder.optString("item_id") != folderId) continue
            hasMatchingFolder = true
            isLoading = isLoading || folder.optBoolean("is_loading")
            val items = folder.optJSONArray("items") ?: JSONArray()
            if (expandPlaylistTracks) {
                val tracks = folder.optJSONArray("playlist_tracks") ?: JSONArray()
                for (trackIndex in 0 until tracks.length()) {
                    tracks.optJSONObject(trackIndex)?.let(selected::put)
                }
            }
            for (itemIndex in 0 until items.length()) {
                val item = items.optJSONObject(itemIndex) ?: continue
                if (item.optString("item_id").isBlank() || item.optString("provider").isBlank()) continue
                if (item.optString("media_type") == mediaType) selected.put(item)
            }
        }
        return LibraryChildren(
            mediaItemsFromJson(selected.toString(), includeTrackListActions = mediaType == "track", mediaType = mediaType),
            recommendationIsLoading(parentId, if (hasMatchingFolder) isLoading else snapshot.optBoolean("is_loading"),
                hasMatchingFolder, selected.length()),
        )
    }

    private fun recommendationItems(
        parentId: String,
        folderId: String,
        mediaType: String,
        limit: Int = Int.MAX_VALUE,
        normalizePlaylistNames: Boolean = false,
    ): LibraryChildren {
        val snapshot = JSONObject(catalogValue(parentId, LibraryRead(LibraryReadKind.HomeRecommendations))
            ?: return LibraryChildren(emptyList(), true))
        val folders = snapshot.getJSONArray("folders")
        val folder = (0 until folders.length()).mapNotNull(folders::optJSONObject).firstOrNull {
            it.optString("item_id").equals(folderId, ignoreCase = true)
        } ?: return LibraryChildren(emptyList(), recommendationIsLoading(parentId, snapshot.optBoolean("is_loading"), false, 0))
        val items = folder.optJSONArray("items") ?: JSONArray()
        val selected = JSONArray()
        for (index in 0 until items.length()) {
            val item = items.optJSONObject(index) ?: continue
            if (item.optString("media_type") != mediaType || selected.length() >= limit) continue
            if (normalizePlaylistNames) {
                val name = item.optString("name")
                if (name.startsWith("Radio: ", ignoreCase = true)) {
                    item.put("name", name.substring(7).trimStart())
                }
            }
            selected.put(item)
        }
        return LibraryChildren(mediaItemsFromJson(
            selected.toString(),
            includeTrackListActions = mediaType == "track",
            fallbackIconResource = if (folderId == "user_top_artists_monthly") R.drawable.favorite else null,
        ), recommendationIsLoading(parentId, folder.optBoolean("is_loading"), true, selected.length()))
    }

    private fun recommendationIsLoading(parentId: String, isLoading: Boolean, hasMatchingFolder: Boolean, count: Int): Boolean {
        val read = parentReads.getValue(parentId)
        if (count == 0 && isLoading) catalogReads.failure(read)?.let { throw IllegalStateException("Recommendation read failed", it) }
        return isLoading && (hasMatchingFolder || !catalogReads.hasObservedFolders(read))
    }

    // Private playlist tracks share the session snapshot instead of triggering a second download.
    private fun dynamicLibraryChildren(parentId: String): LibraryChildren {
        if (parentId.startsWith("browse:playlist:")) {
            val reference = serializedMediaItems[parentId]?.let(::JSONObject)
            if (reference?.optString("provider") == "library") {
                val data = JSONObject(userDataSnapshot())
                val playlists = data.optJSONArray("playlists") ?: JSONArray()
                val playlist = (0 until playlists.length()).mapNotNull(playlists::optJSONObject).firstOrNull {
                    it.optString("item_id") == reference.optString("item_id")
                }
                if (playlist != null) {
                    return LibraryChildren(
                        mediaItemsFromJson(playlist.optJSONArray("items")?.toString(), true, mediaType = "track"),
                        playlist.optBoolean("is_loading_tracks"),
                    )
                }
                if (data.optBoolean("is_loading_playlists")) return LibraryChildren(emptyList(), true)
            }
        }
        return dynamicChildren(parentId)
    }

    private fun dynamicChildren(parentId: String): LibraryChildren = when {
        parentId.startsWith("browse:album:") -> referencedItems(parentId, parentId, LibraryReadKind.AlbumTracks)
        parentId.startsWith("browse:playlist:") -> referencedItems(parentId, parentId, LibraryReadKind.PlaylistTracks)
        parentId.startsWith("browse:artist:") -> LibraryChildren(artistChildren(parentId))
        parentId.startsWith("artistAlbums:") -> referencedItems(parentId, parentId.removePrefix("artistAlbums:"), LibraryReadKind.ArtistAlbums)
        parentId.startsWith("artistTopTracks:") -> referencedItems(parentId, parentId.removePrefix("artistTopTracks:"), LibraryReadKind.ArtistTopTracks)
        parentId.startsWith("similarArtists:") -> referencedItems(parentId, parentId.removePrefix("similarArtists:"), LibraryReadKind.SimilarArtists)
        else -> LibraryChildren(emptyList())
    }

    private fun searchItems(query: String): List<MediaItem> {
        if (query.isBlank()) return emptyList()
        search.start(query)
        return SearchMediaCategory.entries.map { category ->
            val iconResource = when (category) {
                SearchMediaCategory.Tracks -> R.drawable.music_note
                SearchMediaCategory.Albums -> R.drawable.album
                SearchMediaCategory.Playlists -> R.drawable.playlist_play
                SearchMediaCategory.Artists -> R.drawable.artist
            }
            libraryItem("search:${category.resultKey}?$query", category.title, false,
                iconResource = iconResource)
        }
    }

    private fun searchCategoryItems(
        parentId: String,
        page: Int,
        pageSize: Int,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> {
        val categoryKey = parentId.substringAfter(':').substringBefore('?')
        val category = SearchMediaCategory.entries.firstOrNull { it.resultKey == categoryKey }
            ?: return Futures.immediateFuture(LibraryResult.ofError(LibraryResult.RESULT_ERROR_BAD_VALUE))
        if (page < 0 || pageSize <= 0) return Futures.immediateFuture(LibraryResult.ofError(LibraryResult.RESULT_ERROR_BAD_VALUE))
        val query = parentId.substringAfter('?', "")
        val result: ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> = Futures.transform(
            Futures.nonCancellationPropagating(search.results(query, category)), { serializedItems ->
            val items = mediaItemsFromJson(serializedItems,
                includeTrackListActions = category == SearchMediaCategory.Tracks, mediaType = category.mediaType)
            if (!isClosed) deliveredParents[parentId] = items.map { it.mediaId }
            LibraryResult.ofItemList(items.drop(
                (page.toLong() * pageSize).coerceAtMost(Int.MAX_VALUE.toLong()).toInt()).take(pageSize), params)
        }, MoreExecutors.directExecutor())
        return Futures.catching(result, Exception::class.java, {
            LibraryResult.ofError(LibraryResult.RESULT_ERROR_UNKNOWN)
        }, MoreExecutors.directExecutor())
    }

    private fun userDataItems(category: String, mediaType: String, includeTrackListActions: Boolean = false): LibraryChildren {
        val data = JSONObject(userDataSnapshot())
        val items = data.optJSONObject("favorites")?.optJSONArray(category)
        return LibraryChildren(mediaItemsFromJson(items?.toString(), includeTrackListActions, mediaType = mediaType),
            data.optBoolean("is_loading_favorites"))
    }

    private fun privatePlaylistItems(): LibraryChildren {
        val data = JSONObject(userDataSnapshot())
        return LibraryChildren(mediaItemsFromJson(data.optJSONArray("playlists")?.toString(), mediaType = "playlist"),
            data.optBoolean("is_loading_playlists"))
    }

    private fun userDataSnapshot(): String = query("getUserData", null)
        ?: JSONObject().put("is_loading_favorites", true).put("is_loading_playlists", true).toString()

    private fun referencedItems(parentId: String, referenceId: String, kind: LibraryReadKind): LibraryChildren {
        val reference = serializedMediaItems[referenceId] ?: return LibraryChildren(emptyList())
        val item = JSONObject(reference)
        val fallbackIconResource = when (kind) {
            LibraryReadKind.ArtistAlbums -> R.drawable.playlist_play
            LibraryReadKind.SimilarArtists -> R.drawable.favorite
            else -> null
        }
        val value = catalogValue(parentId, LibraryRead(kind, item.getString("item_id"), item.getString("provider")))
            ?: return LibraryChildren(emptyList(), true)
        return LibraryChildren(mediaItemsFromJson(
            value,
            includeTrackListActions = kind.mediaType == "track",
            fallbackIconResource = fallbackIconResource,
            mediaType = kind.mediaType,
        ))
    }

    private fun artistChildren(parentId: String): List<MediaItem> {
        if (!serializedMediaItems.containsKey(parentId)) return emptyList()
        return listOf(
            libraryItem("artistRadio:$parentId", "Artist-Radio", true, iconResource = R.drawable.radio),
            libraryItem("artistAlbums:$parentId", "Alben", false, iconResource = R.drawable.album),
            libraryItem("similarArtists:$parentId", "Ähnliche Artists", false, iconResource = R.drawable.artist),
            libraryItem("artistTopTracks:$parentId", "Top-Tracks", false, iconResource = R.drawable.music_note),
        )
    }

    private fun mediaItemsFromJson(
        serializedItems: String?,
        includeTrackListActions: Boolean = false,
        fallbackIconResource: Int? = null,
        mediaType: String? = null,
    ): List<MediaItem> {
        val items = JSONArray(serializedItems ?: "[]")
        val parsedItems = (0 until items.length()).mapNotNull(items::optJSONObject).onEach { item ->
            if (mediaType != null && !item.has("media_type")) item.put("media_type", mediaType)
        }
        return buildList {
            if (includeTrackListActions) {
                val tracks = JSONArray().apply {
                    parsedItems.filter { it.optString("media_type") == "track" }.forEach(::put)
                }
                if (tracks.length() > 0) {
                    addAll(createTrackListActions(tracks))
                }
            }
            for (item in parsedItems) {
                val itemId = item.optString("item_id").ifEmpty { item.optString("uri") }
                if (itemId.isEmpty()) continue
                val mediaType = item.optString("media_type")
                val mediaId = "browse:$mediaType:${item.optString("provider")}:$itemId"
                serializedMediaItems[mediaId] = item.toString()
                add(
                    MediaItem.Builder()
                        .setMediaId(mediaId)
                        .setMediaMetadata(
                            MediaMetadata.Builder()
                                .setTitle(item.optString("name", itemId))
                                .setArtist(itemArtists(item))
                                .setArtworkUri(MediaArtworkContentProvider.retainRevision(libraryItemsById[mediaId]?.mediaMetadata?.artworkUri, artworkUri(item)?.let(android.net.Uri::parse)) ?: resourceUri(fallbackIconResource ?: when (mediaType) {
                                    "track" -> R.drawable.play_arrow
                                    "album" -> R.drawable.album
                                    "playlist" -> R.drawable.playlist_play
                                    "artist" -> R.drawable.artist
                                    else -> R.drawable.music_note
                                }))
                                .setIsPlayable(mediaType == "track")
                                .setIsBrowsable(mediaType != "track")
                                .build(),
                        )
                        .build(),
                )
                libraryItemsById[mediaId] = last()
            }
        }
    }

    private fun createTrackListActions(tracks: JSONArray): List<MediaItem> {
        val playId = "action:play:${identifierSequence.incrementAndGet()}"
        val shuffleId = "action:shuffle:${identifierSequence.incrementAndGet()}"
        serializedTrackLists[playId] = tracks.toString()
        serializedTrackLists[shuffleId] = tracks.toString()
        return listOf(
            libraryItem(playId, "Abspielen", true, "${tracks.length()} Tracks", iconResource = R.drawable.play_arrow),
            libraryItem(shuffleId, "Zufällige Wiedergabe", true, "${tracks.length()} Tracks", iconResource = R.drawable.shuffle),
        )
    }

    private fun itemArtists(item: JSONObject): String? = item.optJSONArray("artists")
        ?.let { artists -> (0 until artists.length()).mapNotNull(artists::optJSONObject) }
        ?.joinToString(", ") { artist -> artist.optString("name") }
        ?.takeIf(String::isNotBlank)

    private fun artworkUri(item: JSONObject): String? {
        return MediaArtworkContentProvider.buildUri(item)
    }
}

