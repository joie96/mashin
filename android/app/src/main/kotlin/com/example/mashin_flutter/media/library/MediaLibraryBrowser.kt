package com.example.mashin_flutter.media.library

import android.content.ContentResolver
import android.content.Context
import android.os.Bundle
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaConstants
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession
import com.example.mashin_flutter.core.RustCoreBridge
import com.example.mashin_flutter.media.artwork.MediaArtworkContentProvider
import com.example.mashin_flutter.media.session.PlaybackCommandKind
import com.example.mashin_flutter.media.session.PlaybackRequest
import com.example.mashin_flutter.R
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.MoreExecutors
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import org.json.JSONArray
import org.json.JSONObject

// Media-library browsing and boundary media-item caching.
@UnstableApi
internal class MediaLibraryBrowser(
    private val context: Context,
    private val dispatchCommand: (PlaybackRequest) -> ListenableFuture<*>,
) : MediaLibraryService.MediaLibrarySession.Callback {
    private val browseExecutor = MoreExecutors.listeningDecorator(Executors.newSingleThreadExecutor())
    private val libraryItemsById = ConcurrentHashMap<String, MediaItem>()
    private val serializedMediaItems = ConcurrentHashMap<String, String>()
    private val serializedTrackLists = ConcurrentHashMap<String, String>()
    private val identifierSequence = AtomicLong()

    // Browser worker lifecycle.
    fun close() {
        browseExecutor.shutdown()
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
            return dispatchCommand(PlaybackRequest(PlaybackCommandKind.StartSendspinRadio,
                JSONObject().put("mediaItems", JSONArray().put(JSONObject(artist))).toString()))
        }
        serializedTrackLists[mediaId]?.let { serializedItems ->
            return dispatchCommand(PlaybackRequest(
                PlaybackCommandKind.QueuePlayMedia,
                JSONObject().put("request", JSONObject()
                    .put("mediaItems", JSONArray(serializedItems))
                    .put("option", "play")
                    .put("shuffle", mediaId.startsWith("action:shuffle:"))
                    .put("startDownload", true))
                    .toString(),
            ))
        }
        serializedMediaItems[mediaId]?.let { serializedItem ->
            if (JSONObject(serializedItem).optString("media_type") == "track") {
                return dispatchCommand(PlaybackRequest(PlaybackCommandKind.StartSendspinRadio,
                    JSONObject().put("mediaItems", JSONArray().put(JSONObject(serializedItem))).toString()))
            }
            return dispatchCommand(PlaybackRequest(
                PlaybackCommandKind.QueuePlayMedia,
                JSONObject().put("request", JSONObject().put("mediaItems", JSONArray().put(JSONObject(serializedItem)))
                    .put("option", "play").put("startDownload", true)).toString(),
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
        if (parentId == "root") {
            return Futures.immediateFuture(LibraryResult.ofItemList(libraryItems(parentId), params))
        }
        return browseExecutor.submit<LibraryResult<com.google.common.collect.ImmutableList<MediaItem>>> {
            LibraryResult.ofItemList(libraryItems(parentId), params)
        }
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
        return browseExecutor.submit<LibraryResult<Void>> {
            session.notifySearchResultChanged(browser, query, searchItems(query).size, params)
            LibraryResult.ofVoid(params)
        }
    }

    override fun onGetSearchResult(
        session: MediaLibraryService.MediaLibrarySession,
        browser: MediaSession.ControllerInfo,
        query: String,
        page: Int,
        pageSize: Int,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<com.google.common.collect.ImmutableList<MediaItem>>> {
        return browseExecutor.submit<LibraryResult<com.google.common.collect.ImmutableList<MediaItem>>> {
            LibraryResult.ofItemList(searchItems(query), params)
        }
    }

    fun queueEntry(mediaId: String): JSONObject? = serializedMediaItems[mediaId]
        ?.let { JSONObject().put("mediaItem", JSONObject(it)) }

    // Library boundary mapping and serialized media caching.
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

    private fun libraryItems(parentId: String): List<MediaItem> = when (parentId) {
        "root" -> listOf(
            libraryItem("node:home", "Home", false, iconResource = R.drawable.home),
            libraryItem("node:discover", "Entdecken", false, iconResource = R.drawable.explore),
            libraryItem("node:playlists", "Playlists", false, browsableStyle = MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM, iconResource = R.drawable.playlist_play),
            libraryItem("node:favorites", "Favoriten", false, iconResource = R.drawable.favorite),
        )
        "node:home" -> homeItems()
        "node:discover" -> emptyList()
        "node:playlists" -> playlistItems()
        "node:favorites" -> favoriteCategoryItems()
        "home:recommendations" -> recommendationItems("recommendations", "track")
        "home:favoriteGenres" -> recommendationItems("genre_playlists", "playlist", 9, true)
        "home:playlistSuggestions" -> recommendationItems("artist_playlists", "playlist", normalizePlaylistNames = true)
        "home:topTracks" -> recommendationItems("top_tracks", "track")
        "home:topArtists" -> recommendationItems("top_artists", "artist")
        "home:recent" -> mediaItemsFromJson(RustCoreBridge.query("recentlyPlayed", null), true, mediaType = "track")
        "favorites:tracks" -> userDataItems("tracks", "track", true)
        "favorites:albums" -> userDataItems("albums", "album")
        "favorites:playlists" -> userDataItems("playlists", "playlist")
        "favorites:artists" -> userDataItems("artists", "artist")
        else -> dynamicChildren(parentId)
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

    private fun playlistItems(): List<MediaItem> = localPlaylistItems()

    private fun recommendationItems(
        folderId: String,
        mediaType: String,
        limit: Int = Int.MAX_VALUE,
        normalizePlaylistNames: Boolean = false,
    ): List<MediaItem> {
        val folders = JSONArray(RustCoreBridge.query("recommendations", null) ?: "[]")
        val folder = (0 until folders.length()).mapNotNull(folders::optJSONObject).firstOrNull {
            it.optString("item_id").equals(folderId, ignoreCase = true) &&
                it.optString("provider").startsWith("listenbrainz_recommendations--", ignoreCase = true)
        } ?: return emptyList()
        val items = folder.optJSONArray("items") ?: return emptyList()
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
        return mediaItemsFromJson(
            selected.toString(),
            includeTrackListActions = mediaType == "track",
            fallbackIconResource = if (folderId == "top_artists") R.drawable.favorite else null,
        )
    }

    private fun dynamicChildren(parentId: String): List<MediaItem> = when {
        parentId.startsWith("search:") -> searchCategoryItems(parentId)
        parentId.startsWith("browse:album:") -> referencedItems(parentId, "albumTracks")
        parentId.startsWith("browse:playlist:") -> referencedItems(parentId, "playlistTracks")
        parentId.startsWith("browse:artist:") -> artistChildren(parentId)
        parentId.startsWith("artistAlbums:") -> referencedItems(parentId.removePrefix("artistAlbums:"), "artistAlbums")
        parentId.startsWith("artistTopTracks:") -> referencedItems(parentId.removePrefix("artistTopTracks:"), "artistTopTracks")
        parentId.startsWith("similarArtists:") -> referencedItems(parentId.removePrefix("similarArtists:"), "similarArtists")
        else -> emptyList()
    }

    private fun searchItems(query: String): List<MediaItem> {
        if (query.isBlank()) return emptyList()
        val results = JSONObject(RustCoreBridge.query("search", query) ?: "{}")
        return listOf("tracks" to "Tracks", "albums" to "Alben", "playlists" to "Playlists", "artists" to "Artists")
            .filter { (category, _) -> (results.optJSONArray(category)?.length() ?: 0) > 0 }
            .map { (category, title) ->
                val iconResource = when (category) {
                    "tracks" -> R.drawable.music_note
                    "albums" -> R.drawable.album
                    "playlists" -> R.drawable.playlist_play
                    else -> R.drawable.artist
                }
                libraryItem("search:$category?$query", title, false, iconResource = iconResource)
            }
    }

    private fun searchCategoryItems(parentId: String): List<MediaItem> {
        val category = parentId.substringAfter(':').substringBefore('?')
        val query = parentId.substringAfter('?', "")
        return mediaItemsFromJson(
            searchResults(query, category),
            includeTrackListActions = category == "tracks",
            mediaType = when (category) {
                "tracks" -> "track"
                "albums" -> "album"
                "playlists" -> "playlist"
                "artists" -> "artist"
                else -> null
            },
        )
    }

    private fun searchResults(query: String, key: String): String {
        val results = JSONObject(RustCoreBridge.query("search", query) ?: "{}")
        return results.optJSONArray(key)?.toString() ?: "[]"
    }

    private fun userDataItems(category: String, mediaType: String, includeTrackListActions: Boolean = false): List<MediaItem> {
        val data = JSONObject(userDataSnapshot())
        val items = data.optJSONObject("favorites")?.optJSONArray(category)
        return mediaItemsFromJson(items?.toString(), includeTrackListActions, mediaType = mediaType)
    }

    private fun localPlaylistItems(): List<MediaItem> {
        val data = JSONObject(userDataSnapshot())
        return mediaItemsFromJson(data.optJSONArray("playlists")?.toString(), mediaType = "playlist")
    }

    private fun userDataSnapshot(): String = RustCoreBridge.query("userData", null) ?: "{}"

    private fun referencedItems(referenceId: String, queryKind: String): List<MediaItem> {
        val reference = serializedMediaItems[referenceId] ?: return emptyList()
        val item = JSONObject(reference)
        if (queryKind == "playlistTracks" && item.optString("provider") == "mashin") {
            return mediaItemsFromJson(item.optJSONArray("items")?.toString(), true, mediaType = "track")
        }
        val fallbackIconResource = when (queryKind) {
            "artistAlbums" -> R.drawable.playlist_play
            "similarArtists" -> R.drawable.favorite
            else -> null
        }
        return mediaItemsFromJson(
            RustCoreBridge.query(queryKind, itemReference(JSONObject(reference))),
            includeTrackListActions = queryKind != "artistAlbums" && queryKind != "similarArtists",
            fallbackIconResource = fallbackIconResource,
            mediaType = when (queryKind) {
                "artistAlbums" -> "album"
                "similarArtists" -> "artist"
                else -> "track"
            },
        )
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
                                .setArtworkUri(artworkUri(item)?.let(android.net.Uri::parse) ?: resourceUri(fallbackIconResource ?: when (mediaType) {
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

    private fun itemReference(item: JSONObject): String = JSONObject()
        .put("itemId", item.optString("item_id"))
        .put("provider", item.optString("provider"))
        .toString()

    private fun itemArtists(item: JSONObject): String? = item.optJSONArray("artists")
        ?.let { artists -> (0 until artists.length()).mapNotNull(artists::optJSONObject) }
        ?.joinToString(", ") { artist -> artist.optString("name") }
        ?.takeIf(String::isNotBlank)

    private fun artworkUri(item: JSONObject): String? {
        val image = item.optJSONObject("image")
            ?: item.optJSONObject("album")?.optJSONObject("image")
            ?: item.optJSONObject("metadata")?.optJSONArray("images")?.optJSONObject(0)
            ?: item.optJSONObject("album")?.optJSONObject("metadata")?.optJSONArray("images")?.optJSONObject(0)
            ?: return null
        val proxyId = image.optString("proxy_id").takeUnless { it == "null" }
        val path = image.optString("path")
        val source = if (!proxyId.isNullOrEmpty()) {
            val settings = JSONObject(RustCoreBridge.query("sessionSettings", null) ?: "{}")
            val baseUrl = settings.optString("musicAssistantUrl").trimEnd('/')
            if (baseUrl.isNotEmpty()) "$baseUrl/imageproxy/$proxyId" else path
        } else path
        return source.takeIf(String::isNotEmpty)?.let(MediaArtworkContentProvider::buildUri)
    }
}

