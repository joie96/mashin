package com.joie96.mashin.media.session

import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi

internal enum class PlaybackStatus {
    Idle, Uninitialized, Playing, Paused, Buffering, Seeking, Error,
}

@UnstableApi
internal data class PlaybackSnapshot(
    val status: PlaybackStatus,
    val positionMillis: Long,
    val durationMillis: Long?,
    val bufferedPositionMillis: Long,
    val volume: Int,
    val isMuted: Boolean,
    val isShuffleEnabled: Boolean,
    val repeatMode: Int,
    val mediaItems: List<MediaItem>,
    val currentIndex: Int,
    val isLocalPlayback: Boolean = true,
    val serializedCurrentMediaItem: String? = null,
    val isCurrentItemFavorite: Boolean = false,
) {
    val hasPlaybackIntent: Boolean
        get() = status == PlaybackStatus.Playing || status == PlaybackStatus.Buffering || status == PlaybackStatus.Seeking

    val playerState: Int
        get() = when (status) {
            PlaybackStatus.Buffering, PlaybackStatus.Seeking -> if (mediaItems.isEmpty()) Player.STATE_IDLE else Player.STATE_BUFFERING
            PlaybackStatus.Playing, PlaybackStatus.Paused -> Player.STATE_READY
            PlaybackStatus.Idle, PlaybackStatus.Uninitialized, PlaybackStatus.Error -> Player.STATE_IDLE
        }

    companion object {
        // An empty projection does not invent a Media3 timeline.
        fun empty() = PlaybackSnapshot(PlaybackStatus.Idle, 0, null, 0, 50, false, false, Player.REPEAT_MODE_OFF, emptyList(), 0)
    }
}
