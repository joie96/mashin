package com.example.mashin_flutter.media

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ContentResolver
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import android.os.Bundle
import android.util.Log
import com.example.mashin_flutter.R
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.DefaultMediaNotificationProvider
import androidx.media3.session.CommandButton
import androidx.media3.session.MediaNotification
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaConstants
import androidx.core.app.NotificationCompat
import com.google.common.util.concurrent.Futures
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicLong
import com.google.common.util.concurrent.MoreExecutors

@UnstableApi
class MashinMediaLibraryService : MediaLibraryService() {
    private var sessionPlayer: MashinSessionPlayer? = null
    private var mediaLibrarySession: MediaLibraryService.MediaLibrarySession? = null
    private var libraryCallback: AndroidAutoLibraryCallback? = null
    private var isStoppingUnownedService = false
    @Volatile private var isShuttingDownCore = false
    @Volatile private var hasShutDownCore = false
    private val mainHandler = Handler(Looper.getMainLooper())
    private val stopUnownedServiceRunnable = Runnable { stopUnownedService() }
    private val commandExecutor = Executors.newSingleThreadExecutor()
    private val audioManager by lazy { getSystemService(AudioManager::class.java) }
    private val audioFocusRequest by lazy {
        AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                .build())
            .setOnAudioFocusChangeListener({ focusChange ->
                if (focusChange == AudioManager.AUDIOFOCUS_LOSS || focusChange == AudioManager.AUDIOFOCUS_LOSS_TRANSIENT) {
                    dispatchCoreCommand("pause", null)
                }
            }, Handler(Looper.getMainLooper()))
            .build()
    }
    private var hasAudioFocus = false

    // Service lifecycle.
    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        setMediaNotificationProvider(
            MashinMediaNotificationProvider(this).apply {
                setSmallIcon(R.drawable.ic_media_notification)
            },
        )
        NativeCoreBridge.initializeCore(applicationContext)
        if (NativeCoreBridge.restorePersistedSession()) {
            NativeCoreBridge.command("activate", null)
        }
        val initializedLibraryCallback = AndroidAutoLibraryCallback(
            applicationContext,
            ::dispatchCoreCommand,
            ::onAndroidAutoDisconnected,
        )
        libraryCallback = initializedLibraryCallback
        val initializedSessionPlayer = MashinSessionPlayer(
            ::dispatchCoreCommand,
            initializedLibraryCallback::startPlayback,
            initializedLibraryCallback::queueEntry,
        )
        sessionPlayer = initializedSessionPlayer
        val initializedMediaLibrarySession = MediaLibraryService.MediaLibrarySession.Builder(
            this,
            initializedSessionPlayer,
            initializedLibraryCallback,
        ).build()
        mediaLibrarySession = initializedMediaLibrarySession
        addSession(initializedMediaLibrarySession)
        NativeCoreBridge.observeEvents(::applyCoreEvent)
        updatePlayerState()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibraryService.MediaLibrarySession? =
        mediaLibrarySession?.takeUnless { isShuttingDownCore || hasShutDownCore }

    override fun onTaskRemoved(rootIntent: android.content.Intent?) {
        if (libraryCallback?.hasAndroidAutoControllers != true) {
            if (NativeCoreBridge.hasFlutterActivity()) {
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            } else {
                stopUnownedService()
            }
        }
    }

    private fun onAndroidAutoDisconnected() {
        mainHandler.postDelayed(stopUnownedServiceRunnable, 1_000L)
    }

    private fun stopUnownedService() {
        if (!NativeCoreBridge.hasFlutterActivity() && libraryCallback?.hasAndroidAutoControllers == false &&
            !isStoppingUnownedService) {
            isStoppingUnownedService = true
            isShuttingDownCore = true
            commandExecutor.execute {
                if (!NativeCoreBridge.hasFlutterActivity() &&
                    libraryCallback?.hasAndroidAutoControllers == false) {
                    shutDownCore()
                } else {
                    isShuttingDownCore = false
                }
                mainHandler.post {
                    if (hasShutDownCore) {
                        stopForeground(STOP_FOREGROUND_REMOVE)
                        stopSelf()
                    } else {
                        isStoppingUnownedService = false
                        if (!NativeCoreBridge.hasFlutterActivity() &&
                            libraryCallback?.hasAndroidAutoControllers == false) {
                            mainHandler.postDelayed(stopUnownedServiceRunnable, 5_000L)
                        }
                    }
                }
            }
        }
    }

    override fun onDestroy() {
        mainHandler.removeCallbacks(stopUnownedServiceRunnable)
        NativeCoreBridge.stopObservingEvents()
        abandonPlaybackFocus()
        libraryCallback?.close()
        libraryCallback = null
        val shutdown = if (!NativeCoreBridge.hasFlutterActivity() && !hasShutDownCore) {
            commandExecutor.submit {
                if (!NativeCoreBridge.hasFlutterActivity() && !hasShutDownCore) {
                    shutDownCore()
                }
            }
        } else null
        commandExecutor.shutdown()
        mediaLibrarySession?.release()
        mediaLibrarySession = null
        sessionPlayer?.release()
        sessionPlayer = null
        if (shutdown != null) {
            try {
                shutdown.get(4, TimeUnit.SECONDS)
                if (!NativeCoreBridge.hasFlutterActivity() && !hasShutDownCore) {
                    Log.e("MashinAutoPlayback", "Core shutdown incomplete on service destroy")
                }
            } catch (error: TimeoutException) {
                Log.e("MashinAutoPlayback", "Core shutdown timed out on service destroy", error)
            } catch (error: InterruptedException) {
                Thread.currentThread().interrupt()
                Log.e("MashinAutoPlayback", "Core shutdown interrupted on service destroy", error)
            } catch (error: Exception) {
                Log.e("MashinAutoPlayback", "Core shutdown failed on service destroy", error)
            }
        }
        super.onDestroy()
    }

    private fun shutDownCore() {
        try {
            hasShutDownCore = NativeCoreBridge.command("shutdown", null)
            if (!hasShutDownCore) Log.e("MashinAutoPlayback", "Core shutdown failed")
        } catch (error: Exception) {
            Log.e("MashinAutoPlayback", "Core shutdown failed", error)
        } finally {
            isShuttingDownCore = false
        }
    }

    // Core state synchronization.
    private fun applyCoreEvent(payload: String) {
        val event = JSONObject(payload)
        val eventType = event.optString("type")
        if (eventType == "player_state_changed" || eventType == "queue_changed") {
            updatePlayerState()
        }
    }

    private fun updatePlayerState(): Boolean {
        val state = NativeCoreBridge.query("mediaSessionState", null) ?: return false
        val stateJson = JSONObject(state)
        val snapshot = SystemMediaSnapshot.fromPlayerState(
            stateJson.getJSONObject("playerState"),
            stateJson.getJSONObject("queue"),
        )
        if (snapshot.status == "playing" && !requestPlaybackFocus()) {
            dispatchCoreCommand("pause", null)
        } else if (snapshot.status == "paused" || snapshot.status == "idle" || snapshot.status == "error") {
            abandonPlaybackFocus()
        }
        sessionPlayer?.updateState(snapshot)
        if (sessionPlayer != null) triggerNotificationUpdate()
        return true
    }

    private fun dispatchCoreCommand(command: String, argument: String?): ListenableFuture<*> {
        val completion = SettableFuture.create<Unit>()
        try {
            commandExecutor.execute {
                try {
                    if (command == "play" || (command == "queuePlayMedia" && argument?.let { JSONObject(it).optString("option") == "play" } == true) ||
                        command == "startSendspinRadio" || command == "playQueueItem") {
                        check(requestPlaybackFocus()) { "Audio focus is required to start playback" }
                    }
                    check(NativeCoreBridge.command(command, argument)) { "Core command failed: $command" }
                    mainHandler.post {
                        try {
                            check(updatePlayerState()) { "Core media session state unavailable" }
                            completion.set(Unit)
                        } catch (error: Exception) {
                            completion.setException(error)
                        }
                    }
                } catch (error: Exception) {
                    Log.e("MashinAutoPlayback", "Core command failed: $command", error)
                    mainHandler.post { completion.setException(error) }
                }
            }
        } catch (error: RejectedExecutionException) {
            completion.setException(error)
        }
        return completion
    }

    // Audio focus belongs to the service because Flutter and Android Auto share the native output.
    @Synchronized
    private fun requestPlaybackFocus(): Boolean {
        if (hasAudioFocus) return true
        hasAudioFocus = audioManager.requestAudioFocus(audioFocusRequest) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
        if (!hasAudioFocus) Log.w("MashinAutoPlayback", "Audio focus was not granted")
        return hasAudioFocus
    }

    @Synchronized
    private fun abandonPlaybackFocus() {
        if (hasAudioFocus) {
            audioManager.abandonAudioFocusRequest(audioFocusRequest)
            hasAudioFocus = false
        }
    }

    // Notification setup.
    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            DefaultMediaNotificationProvider.DEFAULT_CHANNEL_ID,
            getString(R.string.media_playback_notification_channel_name),
            NotificationManager.IMPORTANCE_LOW,
        )
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }
}

// Media notification presentation.
@UnstableApi
private class MashinMediaNotificationProvider(context: Context) : DefaultMediaNotificationProvider(context) {
    override fun addNotificationActions(
        session: MediaSession,
        mediaButtons: ImmutableList<CommandButton>,
        builder: NotificationCompat.Builder,
        actionFactory: MediaNotification.ActionFactory,
    ): IntArray {
        val compactViewActionIndices = super.addNotificationActions(
            session,
            mediaButtons,
            builder,
            actionFactory,
        )
        val durationMillis = session.player.duration
        val positionMillis = session.player.currentPosition
        if (durationMillis > 0 && durationMillis != C.TIME_UNSET && positionMillis != C.TIME_UNSET) {
            builder.setProgress(
                durationMillis.coerceAtMost(Int.MAX_VALUE.toLong()).toInt(),
                positionMillis.coerceIn(0, durationMillis).toInt(),
                false,
            )
        }
        return compactViewActionIndices
    }
}

// Android Auto media-library browsing and media-item caching.
@UnstableApi
internal class AndroidAutoLibraryCallback(
    private val context: Context,
    private val dispatchCommand: (String, String?) -> ListenableFuture<*>,
    private val onAndroidAutoDisconnected: () -> Unit,
) : MediaLibraryService.MediaLibrarySession.Callback {
    private val androidAutoControllers = mutableSetOf<MediaSession.ControllerInfo>()
    private val browseExecutor = MoreExecutors.listeningDecorator(Executors.newSingleThreadExecutor())
    private val libraryItemsById = ConcurrentHashMap<String, MediaItem>()
    private val serializedMediaItems = ConcurrentHashMap<String, String>()
    private val serializedTrackLists = ConcurrentHashMap<String, String>()
    private val identifierSequence = AtomicLong()

    @Volatile var hasAndroidAutoControllers = false
        private set

    fun close() {
        browseExecutor.shutdown()
    }

    override fun onPostConnect(session: MediaSession, controller: MediaSession.ControllerInfo) {
        if (controller.packageName == "com.google.android.projection.gearhead") {
            androidAutoControllers.add(controller)
            hasAndroidAutoControllers = true
        }
    }

    override fun onDisconnected(session: MediaSession, controller: MediaSession.ControllerInfo) {
        if (androidAutoControllers.remove(controller) && androidAutoControllers.isEmpty()) {
            hasAndroidAutoControllers = false
            onAndroidAutoDisconnected()
        }
    }

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
            return dispatchCommand("startSendspinRadio", "[$artist]")
        }
        serializedTrackLists[mediaId]?.let { serializedItems ->
            return dispatchCommand(
                "queuePlayMedia",
                JSONObject()
                    .put("mediaItems", JSONArray(serializedItems))
                    .put("option", "play")
                    .put("shuffle", mediaId.startsWith("action:shuffle:"))
                    .put("startDownload", true)
                    .toString(),
            )
        }
        serializedMediaItems[mediaId]?.let { serializedItem ->
            if (JSONObject(serializedItem).optString("media_type") == "track") {
                return dispatchCommand("startSendspinRadio", "[$serializedItem]")
            }
            return dispatchCommand(
                "queuePlayMedia",
                JSONObject().put("mediaItems", JSONArray().put(JSONObject(serializedItem)))
                    .put("option", "play").put("startDownload", true).toString(),
            )
        }
        return Futures.immediateFailedFuture<Any>(IllegalArgumentException("unknown Android media ID: $mediaId"))
    }

    fun queueEntry(mediaId: String): JSONObject? = serializedMediaItems[mediaId]
        ?.let { JSONObject().put("mediaItem", JSONObject(it)) }

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
        "home:recent" -> mediaItemsFromJson(NativeCoreBridge.query("recentlyPlayed", null), true, mediaType = "track")
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
        val folders = JSONArray(NativeCoreBridge.query("recommendations", null) ?: "[]")
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
        val results = JSONObject(NativeCoreBridge.query("search", query) ?: "{}")
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
        val results = JSONObject(NativeCoreBridge.query("search", query) ?: "{}")
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

    private fun userDataSnapshot(): String = NativeCoreBridge.query("userData", null) ?: "{}"

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
            NativeCoreBridge.query(queryKind, itemReference(JSONObject(reference))),
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
            val settings = JSONObject(NativeCoreBridge.query("sessionSettings", null) ?: "{}")
            val baseUrl = settings.optString("musicAssistantUrl").trimEnd('/')
            if (baseUrl.isNotEmpty()) "$baseUrl/imageproxy/$proxyId" else path
        } else path
        return source.takeIf(String::isNotEmpty)?.let(MediaArtworkContentProvider::buildUri)
    }
}

// Android Auto player-state adaptation.
@UnstableApi
internal class MashinSessionPlayer(
    private val dispatchCommand: (String, String?) -> ListenableFuture<*>,
    private val startPlayback: (String) -> ListenableFuture<*>,
    private val queueEntry: (String) -> JSONObject?,
) : SimpleBasePlayer(Looper.getMainLooper()) {
    private var snapshot = SystemMediaSnapshot.empty()

    fun updateState(nextSnapshot: SystemMediaSnapshot) {
        snapshot = nextSnapshot
        invalidateState()
    }

    override fun getState(): State {
        val commands = Player.Commands.Builder()
            .add(Player.COMMAND_GET_CURRENT_MEDIA_ITEM)
            .add(Player.COMMAND_GET_TIMELINE)
            .add(Player.COMMAND_GET_METADATA)
            .add(Player.COMMAND_GET_VOLUME)
            .add(Player.COMMAND_SET_MEDIA_ITEM)
            .add(Player.COMMAND_CHANGE_MEDIA_ITEMS)
            .add(Player.COMMAND_PLAY_PAUSE)
            .add(Player.COMMAND_STOP)
            .add(Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM)
            .add(Player.COMMAND_SEEK_TO_MEDIA_ITEM)
            .add(Player.COMMAND_SEEK_TO_NEXT)
            .add(Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM)
            .add(Player.COMMAND_SEEK_TO_PREVIOUS)
            .add(Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM)
            .add(Player.COMMAND_SET_REPEAT_MODE)
            .add(Player.COMMAND_SET_SHUFFLE_MODE)
            .add(Player.COMMAND_SET_VOLUME)
            .build()
        val builder = State.Builder()
            .setAvailableCommands(commands)
            .setPlayWhenReady(
                snapshot.status == "playing" || snapshot.status == "buffering" || snapshot.status == "seeking",
                Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST,
            )
            .setPlaybackState(snapshot.playerState)
            .setIsLoading(snapshot.playerState == Player.STATE_BUFFERING && snapshot.mediaItems.isNotEmpty())
            .setContentPositionMs(snapshot.positionMillis)
            .setContentBufferedPositionMs(
                PositionSupplier.getConstant(snapshot.bufferedPositionMillis),
            )
            .setRepeatMode(snapshot.repeatMode)
            .setShuffleModeEnabled(snapshot.shuffleEnabled)
            .setVolume(if (snapshot.muted) 0f else snapshot.volume / 100f)
            .setUnmuteVolume((snapshot.volume.takeIf { it > 0 } ?: 100) / 100f)

        if (snapshot.mediaItems.isNotEmpty()) {
            builder.setPlaylist(snapshot.mediaItems.mapIndexed { index, item ->
                val durationUs = if (index == snapshot.currentIndex) {
                    snapshot.durationMillis?.times(1_000L) ?: C.TIME_UNSET
                } else {
                    item.mediaMetadata.durationMs?.times(1_000L) ?: C.TIME_UNSET
                }
                MediaItemData.Builder(item.mediaId)
                    .setMediaItem(item)
                    .setMediaMetadata(item.mediaMetadata)
                    .setIsSeekable(durationUs != C.TIME_UNSET)
                    .setDurationUs(durationUs)
                    .setPeriods(listOf(PeriodData.Builder(item.mediaId).setDurationUs(durationUs).build()))
                    .build()
            })
            builder.setCurrentMediaItemIndex(snapshot.currentIndex)
        }
        return builder.build()
    }

    override fun handleSetMediaItems(
        mediaItems: List<MediaItem>,
        startIndex: Int,
        startPositionMs: Long,
    ): ListenableFuture<*> {
        val selectedItem = mediaItems.getOrNull(if (startIndex == C.INDEX_UNSET) 0 else startIndex)
        if (mediaItems.size == 1 && selectedItem?.mediaId?.let {
            it.startsWith("action:") || it.startsWith("artistRadio:") || it.startsWith("browse:track:")
        } == true) {
            val selection = startPlayback(selectedItem.mediaId)
            if (startPositionMs <= 0L) return selection
            return Futures.transformAsync(
                selection,
                { dispatchCommand("seekSeconds", (startPositionMs / 1_000.0).toString()) },
                MoreExecutors.directExecutor(),
            )
        }
        val selectedIndex = if (mediaItems.isEmpty()) null else if (startIndex == C.INDEX_UNSET) {
            if (mediaItems.any { it.mediaId == snapshot.mediaItems.getOrNull(snapshot.currentIndex)?.mediaId }) null else 0
        } else startIndex
        if (selectedIndex != null && selectedIndex !in mediaItems.indices) {
            return Futures.immediateFailedFuture<Any>(IndexOutOfBoundsException("unknown queue item index: $selectedIndex"))
        }
        val previousCurrentId = snapshot.mediaItems.getOrNull(snapshot.currentIndex)?.mediaId
        val existingIds = snapshot.mediaItems.map { it.mediaId }
        val retainedIds = mediaItems.mapNotNull { it.mediaId.takeIf(existingIds::contains) }
        if (retainedIds.size != retainedIds.distinct().size) {
            return Futures.immediateFailedFuture<Any>(IllegalArgumentException("queue item IDs must be distinct"))
        }
        val newItems = mutableMapOf<Int, JSONObject>()
        for ((index, item) in mediaItems.withIndex()) {
            if (item.mediaId !in existingIds) {
                newItems[index] = queueEntry(item.mediaId)?.getJSONObject("mediaItem")
                    ?: return Futures.immediateFailedFuture<Any>(IllegalArgumentException("unknown queue media ID: ${item.mediaId}"))
            }
        }

        val commands = mutableListOf<Pair<String, String>>()
        val removedIds = existingIds.filterNot(retainedIds::contains)
        if (removedIds.isNotEmpty()) {
            commands += "deleteQueueItems" to JSONArray(removedIds).toString()
        }
        val workingIds = mutableListOf<String?>()
        workingIds.addAll(existingIds.filter(retainedIds::contains))
        for ((index, item) in mediaItems.withIndex()) {
            val newItem = newItems[index]
            if (newItem != null) {
                commands += "insertQueueItems" to JSONObject()
                    .put("index", index).put("mediaItems", JSONArray().put(newItem)).toString()
                workingIds.add(index, null)
            } else {
                val previousIndex = workingIds.indexOf(item.mediaId)
                if (previousIndex != index) {
                    commands += "moveQueueItems" to JSONObject()
                        .put("itemIds", JSONArray().put(item.mediaId)).put("targetIndex", index).toString()
                    workingIds.removeAt(previousIndex)
                    workingIds.add(index, item.mediaId)
                }
            }
        }
        var completion: ListenableFuture<*> = Futures.immediateVoidFuture()
        for ((command, argument) in commands) {
            completion = Futures.transformAsync(completion, { dispatchCommand(command, argument) }, MoreExecutors.directExecutor())
        }
        if (selectedIndex != null) {
            completion = Futures.transformAsync(completion, {
                val itemId = snapshot.mediaItems.getOrNull(selectedIndex)?.mediaId
                    ?: throw IndexOutOfBoundsException("unknown queue item index: $selectedIndex")
                if (itemId == previousCurrentId && itemId == snapshot.mediaItems.getOrNull(snapshot.currentIndex)?.mediaId) {
                    Futures.immediateVoidFuture()
                } else {
                    dispatchCommand("selectQueueItem", itemId)
                }
            }, MoreExecutors.directExecutor())
            if (startPositionMs > 0L && startPositionMs != C.TIME_UNSET) {
                completion = Futures.transformAsync(completion, {
                    dispatchCommand("seekSeconds", (startPositionMs / 1_000.0).toString())
                }, MoreExecutors.directExecutor())
            }
        }
        return completion
    }

    override fun handleAddMediaItems(index: Int, mediaItems: List<MediaItem>): ListenableFuture<*> {
        val items = JSONArray()
        for (item in mediaItems) {
            val mediaItem = queueEntry(item.mediaId)?.getJSONObject("mediaItem")
                ?: return Futures.immediateFailedFuture<Any>(IllegalArgumentException("unknown queue media ID: ${item.mediaId}"))
            items.put(mediaItem)
        }
        return if (index == snapshot.mediaItems.size) {
            dispatchCommand("appendQueueItems", items.toString())
        } else {
            dispatchCommand("insertQueueItems", JSONObject().put("index", index).put("mediaItems", items).toString())
        }
    }

    override fun handleRemoveMediaItems(fromIndex: Int, toIndex: Int): ListenableFuture<*> {
        val itemIds = JSONArray()
        snapshot.mediaItems.subList(fromIndex, toIndex).forEach { itemIds.put(it.mediaId) }
        return dispatchCommand("deleteQueueItems", itemIds.toString())
    }

    override fun handleMoveMediaItems(fromIndex: Int, toIndex: Int, newIndex: Int): ListenableFuture<*> {
        val itemIds = JSONArray()
        snapshot.mediaItems.subList(fromIndex, toIndex).forEach { itemIds.put(it.mediaId) }
        return dispatchCommand("moveQueueItems", JSONObject().put("itemIds", itemIds).put("targetIndex", newIndex).toString())
    }

    override fun handleReplaceMediaItems(fromIndex: Int, toIndex: Int, mediaItems: List<MediaItem>): ListenableFuture<*> {
        val items = snapshot.mediaItems.toMutableList()
        items.subList(fromIndex, toIndex).clear()
        items.addAll(fromIndex, mediaItems)
        return handleSetMediaItems(items, C.INDEX_UNSET, 0)
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        if (playWhenReady && (snapshot.mediaItems.isEmpty() || snapshot.status == "buffering" || snapshot.status == "seeking" || snapshot.status == "playing")) {
            return Futures.immediateVoidFuture()
        }
        return dispatchCommand(if (playWhenReady) "play" else "pause", null)
    }

    override fun handleStop(): ListenableFuture<*> = dispatchCommand("stop", null)

    override fun handleSeek(
        mediaItemIndex: Int,
        positionMs: Long,
        seekCommand: Int,
    ): ListenableFuture<*> {
        if (mediaItemIndex == C.INDEX_UNSET) return Futures.immediateVoidFuture()
        if (seekCommand == Player.COMMAND_SEEK_TO_NEXT || seekCommand == Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM ||
            seekCommand == Player.COMMAND_SEEK_TO_PREVIOUS || seekCommand == Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM) {
            if (mediaItemIndex != snapshot.currentIndex) {
                val command = if (seekCommand == Player.COMMAND_SEEK_TO_NEXT ||
                    seekCommand == Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM) "next" else "previous"
                return dispatchCommand(command, null)
            }
            if (seekCommand == Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM ||
                seekCommand == Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM) return Futures.immediateVoidFuture()
        }
        if (mediaItemIndex != snapshot.currentIndex) {
            val item = snapshot.mediaItems.getOrNull(mediaItemIndex)
                ?: return Futures.immediateFailedFuture<Any>(IndexOutOfBoundsException("unknown queue item index: $mediaItemIndex"))
            val selection = dispatchCommand("playQueueItem", item.mediaId)
            if (positionMs <= 0L || positionMs == C.TIME_UNSET) return selection
            return Futures.transformAsync(
                selection,
                { dispatchCommand("seekSeconds", (positionMs / 1_000.0).toString()) },
                MoreExecutors.directExecutor(),
            )
        }
        return dispatchCommand("seekSeconds", ((if (positionMs == C.TIME_UNSET) 0L else positionMs) / 1_000.0).toString())
    }

    override fun handleSetRepeatMode(repeatMode: Int): ListenableFuture<*> {
        val value = when (repeatMode) {
            Player.REPEAT_MODE_ONE -> "one"
            Player.REPEAT_MODE_ALL -> "all"
            else -> "off"
        }
        return dispatchCommand("setRepeatMode", value)
    }

    override fun handleSetShuffleModeEnabled(shuffleModeEnabled: Boolean): ListenableFuture<*> {
        return dispatchCommand("setShuffle", shuffleModeEnabled.toString())
    }

    override fun handleSetVolume(volume: Float, volumeOperationType: Int): ListenableFuture<*> =
        dispatchCommand("setVolume", (volume.coerceIn(0f, 1f) * 100).toInt().toString())

}

@UnstableApi
internal data class SystemMediaSnapshot(
    val status: String,
    val positionMillis: Long,
    val durationMillis: Long?,
    val bufferedPositionMillis: Long,
    val volume: Int,
    val muted: Boolean,
    val shuffleEnabled: Boolean,
    val repeatMode: Int,
    val mediaItems: List<MediaItem>,
    val currentIndex: Int,
) {
    val playerState: Int
        get() = when (status) {
            "buffering", "seeking" -> if (mediaItems.isEmpty()) Player.STATE_IDLE else Player.STATE_BUFFERING
            "playing", "paused" -> Player.STATE_READY
            "idle", "uninitialized", "error" -> Player.STATE_IDLE
            else -> Player.STATE_IDLE
        }

    companion object {
        fun empty() = SystemMediaSnapshot("idle", 0, null, 0, 50, false, false, Player.REPEAT_MODE_OFF, emptyList(), 0)

        fun fromPlayerState(values: JSONObject, queue: JSONObject): SystemMediaSnapshot {
            val queueItemId = values.optString("currentQueueItemId")
            val queueItems = queue.optJSONArray("items")
                ?.let { items -> (0 until items.length()).mapNotNull(items::optJSONObject) }
                ?: emptyList()
            val currentQueueItem = queueItems.firstOrNull { it.optString("queue_item_id") == queueItemId }
            val durationMillis = values.optLong("durationMillis", -1L).takeIf { it > 0 }
                ?: currentQueueItem?.optLong("duration")?.takeIf { it > 0 }?.times(1_000L)
            val positionMillis = values.optLong("positionMillis", 0L)
            val bufferedPositionMillis = values.optLong("bufferedPositionMillis", -1L).takeIf { it >= 0 }
                ?: currentQueueItem?.optJSONObject("cache")?.optLong("cached_duration_millis", -1L)?.takeIf { it >= 0 }
                ?: positionMillis
            val items = queueItems.map { currentItem ->
                val mediaItem = currentItem.optJSONObject("media_item")
                MediaItem.Builder()
                    .setMediaId(currentItem.optString("queue_item_id"))
                    .setMediaMetadata(
                        MediaMetadata.Builder()
                            .setTitle(currentItem.optString("name"))
                            .setArtist(mediaItem?.artistNames())
                            .setAlbumTitle(mediaItem?.optJSONObject("album")?.optString("name"))
                            .setArtworkUri(currentItem.artworkUri())
                            .setDurationMs(if (currentItem.optString("queue_item_id") == queueItemId) durationMillis else mediaItem?.optLong("duration")?.times(1_000L))
                            .build(),
                    )
                    .build()
            }
            return SystemMediaSnapshot(
                status = values.optString("playbackState", "idle"),
                positionMillis = positionMillis,
                durationMillis = durationMillis,
                bufferedPositionMillis = bufferedPositionMillis.coerceAtLeast(positionMillis),
                volume = values.optInt("volume", 50).coerceIn(0, 100),
                muted = values.optBoolean("muted", false),
                shuffleEnabled = queue.optBoolean("shuffleEnabled", false),
                repeatMode = when (queue.optString("repeatMode")) {
                    "one" -> Player.REPEAT_MODE_ONE
                    "all" -> Player.REPEAT_MODE_ALL
                    else -> Player.REPEAT_MODE_OFF
                },
                mediaItems = items,
                currentIndex = queueItems.indexOfFirst { it.optString("queue_item_id") == queueItemId }.coerceAtLeast(0),
            )
        }

        private fun JSONObject.artistNames(): String? = optJSONArray("artists")
            ?.let { artists -> (0 until artists.length()).mapNotNull(artists::optJSONObject) }
            ?.map { artist -> artist.optString("name") }
            ?.filter(String::isNotBlank)
            ?.joinToString(", ")
            ?.takeIf(String::isNotBlank)

        private fun JSONObject.artworkUri(): android.net.Uri? {
            val mediaItem = optJSONObject("media_item")
            val image = optJSONObject("image")
                ?: mediaItem?.optJSONObject("image")
                ?: mediaItem?.optJSONObject("metadata")?.optJSONArray("images")?.optJSONObject(0)
                ?: return null
            val proxyId = image.optString("proxy_id").takeUnless { it == "null" }
            val path = image.optString("path")
            val source = if (!proxyId.isNullOrEmpty()) {
                val settings = JSONObject(NativeCoreBridge.query("sessionSettings", null) ?: "{}")
                val baseUrl = settings.optString("musicAssistantUrl").trimEnd('/')
                if (baseUrl.isNotEmpty()) "$baseUrl/imageproxy/$proxyId" else path
            } else {
                path
            }
            return source.takeIf(String::isNotEmpty)
                ?.let(MediaArtworkContentProvider::buildUri)
                ?.let(android.net.Uri::parse)
        }
    }
}