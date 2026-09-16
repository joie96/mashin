package com.example.mashin_flutter.media

import android.app.Notification
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
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession
import androidx.media3.session.LibraryResult
import com.google.common.util.concurrent.MoreExecutors
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import org.json.JSONArray
import org.json.JSONObject

@UnstableApi
class MashinMediaLibraryService : MediaLibraryService() {
    private lateinit var sessionPlayer: MashinSessionPlayer
    private var mediaLibrarySession: MediaLibraryService.MediaLibrarySession? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        setMediaNotificationProvider(
            DefaultMediaNotificationProvider(this).apply {
                setSmallIcon(R.drawable.ic_media_notification)
            },
        )
        startForeground(
            DefaultMediaNotificationProvider.DEFAULT_NOTIFICATION_ID,
            Notification.Builder(this, DefaultMediaNotificationProvider.DEFAULT_CHANNEL_ID)
                .setSmallIcon(R.drawable.ic_media_notification)
                .setContentTitle(getString(R.string.media_playback_notification_title))
                .setOngoing(true)
                .build(),
        )
            NativeCoreBridge.initializeCore(applicationContext)
        if (NativeCoreBridge.restorePersistedSession()) {
            NativeCoreBridge.command("activate", null)
        }
            sessionPlayer = MashinSessionPlayer(::dispatchCoreCommand)
        mediaLibrarySession = MediaLibraryService.MediaLibrarySession.Builder(
            this,
            sessionPlayer,
            AndroidAutoLibraryCallback(),
        ).build()
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
        sessionPlayer.release()
        super.onDestroy()
    }

    private fun applyCoreEvent(payload: String) {
        val event = JSONObject(payload)
        if (event.optString("type") == "player_state_changed") {
            sessionPlayer.updateState(SystemMediaSnapshot.fromPlayerState(event.getJSONObject("player_state")))
            mediaLibrarySession?.let(::onUpdateNotification)
        }
    }

    private fun updatePlayerState() {
        NativeCoreBridge.query("playerState", null)?.let { playerState ->
            sessionPlayer.updateState(SystemMediaSnapshot.fromPlayerState(JSONObject(playerState)))
        }
        mediaLibrarySession?.let(::onUpdateNotification)
    }

    private fun dispatchCoreCommand(command: String, argument: String? = null) {
        NativeCoreBridge.command(command, argument)
        updatePlayerState()
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            DefaultMediaNotificationProvider.DEFAULT_CHANNEL_ID,
            getString(R.string.media_playback_notification_channel_name),
            NotificationManager.IMPORTANCE_LOW,
        )
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }
}

@UnstableApi
private class AndroidAutoLibraryCallback : MediaLibraryService.MediaLibrarySession.Callback {
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
        mediaItems.getOrNull(startIndex)?.requestMetadata?.mediaUri?.toString()?.let { serializedItem ->
            NativeCoreBridge.command("playMediaItems", "[$serializedItem]")
        }
        return Futures.immediateFuture(
            MediaSession.MediaItemsWithStartPosition(mediaItems, startIndex, startPositionMs),
        )
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
            libraryItem("recommendations", "Empfehlungen", false),
            libraryItem("recentlyPlayed", "Zuletzt gehoert", false),
        )
        "recommendations" -> recommendationItems()
        "recentlyPlayed" -> mediaItemsFromJson(NativeCoreBridge.query("recentlyPlayed", null))
        else -> emptyList()
    }

    private fun recommendationItems(): List<MediaItem> {
        val folders = JSONArray(NativeCoreBridge.query("recommendations", null) ?: "[]")
        return buildList {
            for (index in 0 until folders.length()) {
                val folder = folders.optJSONObject(index) ?: continue
                val items = folder.optJSONArray("items") ?: continue
                addAll(mediaItemsFromJson(items.toString()))
            }
        }
    }

    private fun searchItems(query: String): List<MediaItem> {
        val results = JSONObject(NativeCoreBridge.query("search", query) ?: "{}")
        return sequenceOf("tracks", "albums", "artists", "playlists")
            .flatMap { key -> mediaItemsFromJson(results.optJSONArray(key)?.toString()).asSequence() }
            .toList()
    }

    private fun mediaItemsFromJson(serializedItems: String?): List<MediaItem> {
        val items = JSONArray(serializedItems ?: "[]")
        return buildList {
            for (index in 0 until items.length()) {
                val item = items.optJSONObject(index) ?: continue
                val itemId = item.optString("itemId").ifEmpty { item.optString("uri") }
                if (itemId.isEmpty()) continue
                add(
                    MediaItem.Builder()
                        .setMediaId(itemId)
                        .setRequestMetadata(
                            MediaItem.RequestMetadata.Builder()
                                .setMediaUri(android.net.Uri.parse(item.toString()))
                                .build(),
                        )
                        .setMediaMetadata(
                            MediaMetadata.Builder()
                                .setTitle(item.optString("name", itemId))
                                .setArtist(item.optString("artist"))
                                .setArtworkUri(item.optString("image").takeIf(String::isNotEmpty)?.let(android.net.Uri::parse))
                                .setIsPlayable(item.optString("mediaType") == "track")
                                .setIsBrowsable(item.optString("mediaType") != "track")
                                .build(),
                        )
                        .build(),
                )
            }
        }
    }
}

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
                        .setIsSeekable(snapshot.durationMillis != null)
                        .setDurationUs(durationUs)
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

        fun fromPlayerState(values: JSONObject): SystemMediaSnapshot {
            val mediaId = values.optString("currentItemId").takeIf(String::isNotEmpty)
            val item = mediaId?.let {
                MediaItem.Builder()
                    .setMediaId(it)
                    .setMediaMetadata(
                        MediaMetadata.Builder()
                            .setTitle(values.optString("currentItemName"))
                            .build(),
                    )
                    .build()
            }
            return SystemMediaSnapshot(
                status = values.optString("playbackState", "idle"),
                positionMillis = values.optDouble("positionSeconds", 0.0).times(1_000).toLong(),
                durationMillis = values.optDouble("durationSeconds", -1.0).takeIf { it >= 0 }?.times(1_000)?.toLong(),
                shuffleEnabled = values.optBoolean("shuffleEnabled", false),
                repeatMode = when (values.optString("repeatMode")) {
                    "one" -> Player.REPEAT_MODE_ONE
                    "all" -> Player.REPEAT_MODE_ALL
                    else -> Player.REPEAT_MODE_OFF
                },
                mediaItem = item,
            )
        }
    }
}