package com.example.mashin_flutter.media.session

import android.os.Bundle
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import com.example.mashin_flutter.media.artwork.MediaArtworkContentProvider
import org.json.JSONObject

@UnstableApi
internal object PlaybackSnapshotMapper {
    const val SERIALIZED_MEDIA_ITEM_KEY = "mashin.serializedMediaItem"

    // Canonical Rust state is projected without native calls or a second queue owner.
    fun fromPlayerState(values: JSONObject, queue: JSONObject, musicAssistantUrl: String = "", userData: JSONObject? = null): PlaybackSnapshot {
        val queueItemId = values.optString("currentQueueItemId")
        val queueItems = queue.optJSONArray("items")
            ?.let { items -> (0 until items.length()).mapNotNull(items::optJSONObject) }
            ?: emptyList()
        val currentQueueItem = queueItems.firstOrNull { it.optString("queue_item_id") == queueItemId }
        val currentMediaItem = currentQueueItem?.optJSONObject("media_item")
        val currentUri = currentMediaItem?.optString("uri")?.trim()?.takeIf { it.isNotEmpty() && it != "null" }
        val favoriteCategory = when (currentMediaItem?.optString("media_type")) {
            "track" -> "tracks"
            "album" -> "albums"
            "playlist" -> "playlists"
            "artist" -> "artists"
            else -> null
        }
        val favorites = favoriteCategory?.let { userData?.optJSONObject("favorites")?.optJSONArray(it) }
        val isCurrentItemFavorite = currentUri != null && favorites != null && (0 until favorites.length()).any { index ->
            favorites.optJSONObject(index)?.optString("uri")?.trim()?.equals(currentUri, ignoreCase = true) == true
        }
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
                        .setArtworkUri(currentItem.artworkUri(musicAssistantUrl))
                        .setDurationMs(if (currentItem.optString("queue_item_id") == queueItemId) durationMillis else mediaItem?.optLong("duration")?.times(1_000L))
                        .setExtras(mediaItem?.let { Bundle().apply { putString(SERIALIZED_MEDIA_ITEM_KEY, it.toString()) } })
                        .build(),
                )
                .build()
        }
        return PlaybackSnapshot(
            status = when (values.optString("playbackState", "idle")) {
                "playing" -> PlaybackStatus.Playing
                "paused" -> PlaybackStatus.Paused
                "buffering" -> PlaybackStatus.Buffering
                "seeking" -> PlaybackStatus.Seeking
                "uninitialized" -> PlaybackStatus.Uninitialized
                "error" -> PlaybackStatus.Error
                else -> PlaybackStatus.Idle
            },
            positionMillis = positionMillis,
            durationMillis = durationMillis,
            bufferedPositionMillis = bufferedPositionMillis.coerceAtLeast(positionMillis),
            volume = values.optInt("volume", 50).coerceIn(0, 100),
            isMuted = values.optBoolean("muted", false),
            isShuffleEnabled = queue.optBoolean("shuffleEnabled", false),
            repeatMode = when (queue.optString("repeatMode")) {
                "one" -> Player.REPEAT_MODE_ONE
                "all" -> Player.REPEAT_MODE_ALL
                else -> Player.REPEAT_MODE_OFF
            },
            mediaItems = items,
            currentIndex = queueItems.indexOfFirst { it.optString("queue_item_id") == queueItemId }.coerceAtLeast(0),
            isLocalPlayback = values.optString("activeMode") == "local",
            serializedCurrentMediaItem = currentMediaItem?.takeIf { userData != null && currentUri != null && favoriteCategory != null }?.toString(),
            isCurrentItemFavorite = isCurrentItemFavorite,
        )
    }

    // Boundary metadata mapping.
    private fun JSONObject.artistNames(): String? = optJSONArray("artists")
        ?.let { artists -> (0 until artists.length()).mapNotNull(artists::optJSONObject) }
        ?.map { artist -> artist.optString("name") }
        ?.filter(String::isNotBlank)
        ?.joinToString(", ")
        ?.takeIf(String::isNotBlank)

    private fun JSONObject.artworkUri(musicAssistantUrl: String): android.net.Uri? {
        val mediaItem = optJSONObject("media_item")
        val album = mediaItem?.optJSONObject("album")
        val images = album?.artworkImages().orEmpty() + artworkImages() + mediaItem?.artworkImages().orEmpty()
        val baseUrl = musicAssistantUrl.trim().trimEnd('/')
        for (image in images) {
            val proxyId = image.optString("proxy_id").trim().takeUnless { it == "null" }
            val path = image.optString("path").trim().takeUnless { it == "null" }.orEmpty()
            val source = if (!proxyId.isNullOrEmpty() && baseUrl.isNotEmpty()) {
                android.net.Uri.parse(baseUrl).buildUpon().appendPath("imageproxy").appendPath(proxyId).build().toString()
            } else {
                path
            }
            if (source.isNotEmpty() && android.net.Uri.parse(source).scheme?.lowercase() in listOf("http", "https", "data", "file")) {
                return android.net.Uri.parse(MediaArtworkContentProvider.buildUri(source))
            }
        }
        return null
    }

    // Album artwork has the same precedence as in Flutter; unusable entries must not hide later covers.
    private fun JSONObject.artworkImages(): List<JSONObject> {
        val metadataImages = optJSONObject("metadata")?.optJSONArray("images")
        val images = metadataImages?.let { values ->
            (0 until values.length()).mapNotNull(values::optJSONObject)
                .sortedBy { if (it.optString("type", "thumb") == "thumb") 0 else 1 }
        }.orEmpty()
        return listOfNotNull(optJSONObject("image")) + images
    }
}
