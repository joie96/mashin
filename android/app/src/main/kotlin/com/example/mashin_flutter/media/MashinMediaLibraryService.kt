package com.example.mashin_flutter.media

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.os.Looper
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
import androidx.core.app.NotificationCompat
import com.google.common.util.concurrent.Futures
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.ListenableFuture
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

@UnstableApi
class MashinMediaLibraryService : MediaLibraryService() {
    private var sessionPlayer: MashinSessionPlayer? = null
    private var mediaLibrarySession: MediaLibraryService.MediaLibrarySession? = null

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
        val initializedSessionPlayer = MashinSessionPlayer(::dispatchCoreCommand)
        sessionPlayer = initializedSessionPlayer
        val initializedMediaLibrarySession = MediaLibraryService.MediaLibrarySession.Builder(
            this,
            initializedSessionPlayer,
            AndroidAutoLibraryCallback(),
        ).build()
        mediaLibrarySession = initializedMediaLibrarySession
        addSession(initializedMediaLibrarySession)
        NativeCoreBridge.observeEvents(::applyCoreEvent)
        updatePlayerState()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibraryService.MediaLibrarySession? =
        mediaLibrarySession

    override fun onTaskRemoved(rootIntent: android.content.Intent?) {
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onDestroy() {
        NativeCoreBridge.stopObservingEvents()
        mediaLibrarySession?.release()
        mediaLibrarySession = null
        sessionPlayer?.release()
        sessionPlayer = null
        super.onDestroy()
    }

    // Core state synchronization.
    private fun applyCoreEvent(payload: String) {
        val event = JSONObject(payload)
        val eventType = event.optString("type")
        if (eventType == "player_state_changed" || eventType == "queue_changed") {
            updatePlayerState()
        }
    }

    private fun updatePlayerState() {
        val playerState = NativeCoreBridge.query("playerState", null)
        val queue = NativeCoreBridge.query("queue", null)
        if (playerState == null || queue == null) {
            return
        }

        val playerStateJson = JSONObject(playerState)
        val queueJson = JSONObject(queue)
        val snapshot = SystemMediaSnapshot.fromPlayerState(playerStateJson, queueJson)
        sessionPlayer?.updateState(snapshot)
        if (sessionPlayer != null) triggerNotificationUpdate()
    }

    private fun dispatchCoreCommand(command: String, argument: String? = null) {
        NativeCoreBridge.command(command, argument)
        updatePlayerState()
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
private class AndroidAutoLibraryCallback : MediaLibraryService.MediaLibrarySession.Callback {
    private val serializedMediaItems = ConcurrentHashMap<String, String>()
    private val serializedTrackLists = ConcurrentHashMap<String, String>()
    private val identifierSequence = AtomicLong()

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
        return Futures.immediateFuture(
            LibraryResult.ofItemList(libraryItems(parentId), params),
        )
    }

    override fun onGetSearchResult(
        session: MediaLibraryService.MediaLibrarySession,
        browser: MediaSession.ControllerInfo,
        query: String,
        page: Int,
        pageSize: Int,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<com.google.common.collect.ImmutableList<MediaItem>>> {
        return Futures.immediateFuture(
            LibraryResult.ofItemList(searchItems(query), params),
        )
    }

    override fun onSetMediaItems(
        mediaSession: MediaSession,
        controller: MediaSession.ControllerInfo,
        mediaItems: List<MediaItem>,
        startIndex: Int,
        startPositionMs: Long,
    ): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
        mediaItems.getOrNull(startIndex)?.mediaId?.let(::playMediaId)
        return Futures.immediateFuture(
            MediaSession.MediaItemsWithStartPosition(mediaItems, startIndex, startPositionMs),
        )
    }

    private fun playMediaId(mediaId: String) {
        serializedTrackLists[mediaId]?.let { serializedItems ->
            NativeCoreBridge.command("playMediaItems", serializedItems)
            return
        }
        serializedMediaItems[mediaId]?.let { serializedItem ->
            NativeCoreBridge.command("playMediaItems", "[$serializedItem]")
        }
    }

    private fun libraryItem(values: Map<String, Any?>): MediaItem = libraryItem(
        values["id"] as? String ?: "",
        values["title"] as? String ?: "Mashin",
        values["playable"] as? Boolean ?: false,
        values["subtitle"] as? String,
        values["artworkUri"] as? String,
    )

    private fun libraryItem(
        id: String,
        title: String,
        playable: Boolean,
        subtitle: String? = null,
        artworkUri: String? = null,
    ): MediaItem = MediaItem.Builder()
        .setMediaId(id)
        .setMediaMetadata(
            MediaMetadata.Builder()
                .setTitle(title)
                .setSubtitle(subtitle)
                .setArtworkUri(artworkUri?.let(android.net.Uri::parse))
                .setIsBrowsable(!playable)
                .setIsPlayable(playable)
                .build(),
        )
        .build()

    private fun libraryItems(parentId: String): List<MediaItem> = when (parentId) {
        "root" -> listOf(
            libraryItem("node:home", "Startseite", false),
            libraryItem("node:playlists", "Playlists", false),
            libraryItem("node:favorites", "Favoriten", false),
        )
        "node:home" -> homeItems()
        "node:playlists" -> playlistItems()
        "node:favorites" -> favoriteCategoryItems()
        "home:recommendations" -> recommendationFolders()
        "home:recent" -> mediaItemsFromJson(NativeCoreBridge.query("recentlyPlayed", null))
        "favorites:tracks" -> userDataItems("tracks")
        "favorites:albums" -> userDataItems("albums")
        "favorites:playlists" -> userDataItems("playlists")
        "favorites:artists" -> userDataItems("artists")
        "local:playlists" -> localPlaylistItems()
        "search:tracks", "search:albums", "search:artists", "search:playlists" -> searchCategoryItems(parentId)
        else -> dynamicChildren(parentId)
    }

    private fun homeItems(): List<MediaItem> = listOf(
        libraryItem("home:recommendations", "Empfehlungen", false),
        libraryItem("home:recent", "Zuletzt gehört", false),
    )

    private fun favoriteCategoryItems(): List<MediaItem> = listOf(
        libraryItem("favorites:tracks", "Tracks", false),
        libraryItem("favorites:albums", "Alben", false),
        libraryItem("favorites:playlists", "Playlists", false),
        libraryItem("favorites:artists", "Artists", false),
    )

    private fun playlistItems(): List<MediaItem> = listOf(
        libraryItem("local:playlists", "Meine Playlists", false),
    ) + mediaItemsFromJson(searchResults("", "playlists"))

    private fun recommendationFolders(): List<MediaItem> {
        val folders = JSONArray(NativeCoreBridge.query("recommendations", null) ?: "[]")
        return buildList {
            for (index in 0 until folders.length()) {
                val folder = folders.optJSONObject(index) ?: continue
                val folderId = folder.optString("item_id").ifEmpty { continue }
                val id = "recommendation:$folderId"
                serializedMediaItems[id] = folder.toString()
                add(libraryItem(id, folder.optString("name", "Empfehlungen"), false, artworkUri = artworkUri(folder)))
            }
        }
    }

    private fun dynamicChildren(parentId: String): List<MediaItem> = when {
        parentId.startsWith("recommendation:") -> recommendationFolderItems(parentId)
        parentId.startsWith("album:") -> referencedItems(parentId, "albumTracks")
        parentId.startsWith("playlist:") -> referencedItems(parentId, "playlistTracks")
        parentId.startsWith("artist:") -> artistChildren(parentId)
        parentId.startsWith("artistAlbums:") -> referencedItems(parentId.removePrefix("artistAlbums:"), "artistAlbums")
        parentId.startsWith("artistTopTracks:") -> referencedItems(parentId.removePrefix("artistTopTracks:"), "artistTopTracks")
        parentId.startsWith("similarArtists:") -> referencedItems(parentId.removePrefix("similarArtists:"), "similarArtists")
        else -> emptyList()
    }

    private fun recommendationFolderItems(parentId: String): List<MediaItem> {
        val folderId = parentId.removePrefix("recommendation:")
        val folders = JSONArray(NativeCoreBridge.query("recommendations", null) ?: "[]")
        val items = (0 until folders.length())
            .mapNotNull(folders::optJSONObject)
            .firstOrNull { it.optString("item_id") == folderId }
            ?.optJSONArray("items")
            ?: return emptyList()
        return mediaItemsFromJson(items.toString(), includeTrackListActions = true)
    }

    private fun searchItems(query: String): List<MediaItem> = listOf(
        libraryItem("search:tracks?$query", "Tracks", false),
        libraryItem("search:albums?$query", "Alben", false),
        libraryItem("search:artists?$query", "Artists", false),
        libraryItem("search:playlists?$query", "Playlists", false),
    )

    private fun searchCategoryItems(parentId: String): List<MediaItem> {
        val category = parentId.substringBefore(':').let { parentId.substringAfter(':').substringBefore('?') }
        val query = parentId.substringAfter('?', "")
        return mediaItemsFromJson(searchResults(query, category), includeTrackListActions = category == "tracks")
    }

    private fun searchResults(query: String, key: String): String {
        val results = JSONObject(NativeCoreBridge.query("search", query) ?: "{}")
        return results.optJSONArray(key)?.toString() ?: "[]"
    }

    private fun userDataItems(category: String): List<MediaItem> {
        val data = JSONObject(NativeCoreBridge.query("userData", null) ?: "{}")
        return mediaItemsFromJson(data.optJSONObject("favorites")?.optJSONArray(category)?.toString())
    }

    private fun localPlaylistItems(): List<MediaItem> {
        val data = JSONObject(NativeCoreBridge.query("userData", null) ?: "{}")
        return mediaItemsFromJson(data.optJSONArray("playlists")?.toString())
    }

    private fun referencedItems(referenceId: String, queryKind: String): List<MediaItem> {
        val reference = serializedMediaItems[referenceId] ?: return emptyList()
        val item = JSONObject(reference)
        if (queryKind == "playlistTracks" && item.optString("provider") == "mashin") {
            return mediaItemsFromJson(item.optJSONArray("items")?.toString(), true)
        }
        return mediaItemsFromJson(NativeCoreBridge.query(queryKind, itemReference(JSONObject(reference))), true)
    }

    private fun artistChildren(parentId: String): List<MediaItem> {
        val reference = serializedMediaItems[parentId] ?: return emptyList()
        return listOf(
            libraryItem("artistAlbums:$parentId", "Alben", false),
            libraryItem("artistTopTracks:$parentId", "Top Tracks", false),
            libraryItem("similarArtists:$parentId", "Ähnliche Artists", false),
        )
    }

    private fun mediaItemsFromJson(serializedItems: String?, includeTrackListActions: Boolean = false): List<MediaItem> {
        val items = JSONArray(serializedItems ?: "[]")
        val parsedItems = (0 until items.length()).mapNotNull(items::optJSONObject)
        return buildList {
            if (includeTrackListActions) {
                val tracks = JSONArray(parsedItems.filter { it.optString("media_type") == "track" }.map(JSONObject::toString))
                if (tracks.length() > 0) {
                    addAll(createTrackListActions(tracks))
                }
            }
            for (item in parsedItems) {
                val itemId = item.optString("item_id").ifEmpty { item.optString("uri") }
                if (itemId.isEmpty()) continue
                val mediaType = item.optString("media_type")
                val mediaId = "$mediaType:$itemId:${item.optString("provider")}:${identifierSequence.incrementAndGet()}"
                serializedMediaItems[mediaId] = item.toString()
                add(
                    MediaItem.Builder()
                        .setMediaId(mediaId)
                        .setMediaMetadata(
                            MediaMetadata.Builder()
                                .setTitle(item.optString("name", itemId))
                                .setArtist(itemArtists(item))
                                .setArtworkUri(artworkUri(item)?.let(android.net.Uri::parse))
                                .setIsPlayable(mediaType == "track")
                                .setIsBrowsable(mediaType != "track")
                                .build(),
                        )
                        .build(),
                )
            }
        }
    }

    private fun createTrackListActions(tracks: JSONArray): List<MediaItem> {
        val playId = "action:play:${identifierSequence.incrementAndGet()}"
        val shuffleId = "action:shuffle:${identifierSequence.incrementAndGet()}"
        serializedTrackLists[playId] = tracks.toString()
        val shuffled = (0 until tracks.length()).mapNotNull(tracks::optJSONObject).shuffled()
        serializedTrackLists[shuffleId] = JSONArray(shuffled).toString()
        return listOf(
            libraryItem(playId, "Abspielen", true, "${tracks.length()} Tracks"),
            libraryItem(shuffleId, "Zufällige Wiedergabe", true, "${tracks.length()} Tracks"),
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
    private val dispatchCommand: (String, String?) -> Unit,
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
            .add(Player.COMMAND_PLAY_PAUSE)
            .add(Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM)
            .add(Player.COMMAND_SEEK_TO_NEXT)
            .add(Player.COMMAND_SEEK_TO_PREVIOUS)
            .add(Player.COMMAND_SET_REPEAT_MODE)
            .add(Player.COMMAND_SET_SHUFFLE_MODE)
            .build()
        val builder = State.Builder()
            .setAvailableCommands(commands)
            .setPlayWhenReady(
                snapshot.status == "playing",
                Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST,
            )
            .setPlaybackState(snapshot.playerState)
            .setContentPositionMs(snapshot.positionMillis)
            .setContentBufferedPositionMs(
                PositionSupplier.getConstant(snapshot.positionMillis),
            )
            .setRepeatMode(snapshot.repeatMode)
            .setShuffleModeEnabled(snapshot.shuffleEnabled)

        snapshot.mediaItem?.let { item ->
            val durationUs = snapshot.durationMillis?.let { durationMillis ->
                durationMillis * 1_000L
            } ?: C.TIME_UNSET
            builder.setPlaylist(
                listOf(
                    MediaItemData.Builder(item.mediaId)
                        .setMediaItem(item)
                        .setMediaMetadata(item.mediaMetadata)
                        .setIsSeekable(snapshot.durationMillis != null)
                        .setDurationUs(durationUs)
                        .setPeriods(
                            listOf(
                                PeriodData.Builder(item.mediaId)
                                    .setDurationUs(durationUs)
                                    .build(),
                            ),
                        )
                        .build(),
                ),
            )
            builder.setCurrentMediaItemIndex(0)
        }
        return builder.build()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        dispatchCommand(if (playWhenReady) "play" else "pause", null)
        return Futures.immediateVoidFuture()
    }

    override fun handleSeek(
        mediaItemIndex: Int,
        positionMs: Long,
        seekCommand: Int,
    ): ListenableFuture<*> {
        when (seekCommand) {
            Player.COMMAND_SEEK_TO_NEXT -> dispatchCommand("next", null)
            Player.COMMAND_SEEK_TO_PREVIOUS -> dispatchCommand("previous", null)
            else -> dispatchCommand("seekSeconds", (positionMs / 1_000.0).toString())
        }
        return Futures.immediateVoidFuture()
    }

    override fun handleSetRepeatMode(repeatMode: Int): ListenableFuture<*> {
        val value = when (repeatMode) {
            Player.REPEAT_MODE_ONE -> "one"
            Player.REPEAT_MODE_ALL -> "all"
            else -> "off"
        }
        dispatchCommand("setRepeatMode", value)
        return Futures.immediateVoidFuture()
    }

    override fun handleSetShuffleModeEnabled(shuffleModeEnabled: Boolean): ListenableFuture<*> {
        dispatchCommand("setShuffle", shuffleModeEnabled.toString())
        return Futures.immediateVoidFuture()
    }

}

@UnstableApi
internal data class SystemMediaSnapshot(
    val status: String,
    val positionMillis: Long,
    val durationMillis: Long?,
    val shuffleEnabled: Boolean,
    val repeatMode: Int,
    val mediaItem: MediaItem?,
) {
    val playerState: Int
        get() = when (status) {
            "buffering" -> Player.STATE_BUFFERING
            "playing", "paused" -> Player.STATE_READY
            "idle", "uninitialized", "error" -> Player.STATE_IDLE
            else -> Player.STATE_IDLE
        }

    companion object {
        fun empty() = SystemMediaSnapshot("idle", 0, null, false, Player.REPEAT_MODE_OFF, null)

        fun fromPlayerState(values: JSONObject, queue: JSONObject): SystemMediaSnapshot {
            val durationMillis = values.optLong("durationMillis", -1L).takeIf { it >= 0 }
            val queueItemId = values.optString("currentQueueItemId")
            val queueItem = queue.optJSONArray("items")
                ?.let { items -> (0 until items.length()).mapNotNull(items::optJSONObject) }
                ?.firstOrNull { item -> item.optString("queue_item_id") == queueItemId }
            val item = queueItem?.let { currentItem ->
                val mediaItem = currentItem.optJSONObject("media_item")
                MediaItem.Builder()
                    .setMediaId(queueItemId)
                    .setMediaMetadata(
                        MediaMetadata.Builder()
                            .setTitle(currentItem.optString("name"))
                            .setArtist(mediaItem?.artistNames())
                            .setAlbumTitle(mediaItem?.optJSONObject("album")?.optString("name"))
                            .setArtworkUri(currentItem.artworkUri())
                            .setDurationMs(durationMillis)
                            .build(),
                    )
                    .build()
            }
            return SystemMediaSnapshot(
                status = values.optString("playbackState", "idle"),
                positionMillis = values.optLong("positionMillis", 0L),
                durationMillis = durationMillis,
                shuffleEnabled = queue.optBoolean("shuffleEnabled", false),
                repeatMode = when (queue.optString("repeatMode")) {
                    "one" -> Player.REPEAT_MODE_ONE
                    "all" -> Player.REPEAT_MODE_ALL
                    else -> Player.REPEAT_MODE_OFF
                },
                mediaItem = item,
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