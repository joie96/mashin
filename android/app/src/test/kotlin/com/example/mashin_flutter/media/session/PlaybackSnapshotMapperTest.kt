package com.example.mashin_flutter.media.session

import android.os.Bundle
import androidx.media3.common.Player
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PlaybackSnapshotMapperTest {
    @Test
    fun failedSessionResultIsNotAPlayerAcknowledgement() {
        val result = androidx.media3.session.SessionResult(-1,
            android.os.Bundle().apply { putString(PlaybackSessionCommands.ERROR_KEY, "audio focus denied") })
        val error = org.junit.Assert.assertThrows(IllegalStateException::class.java) {
            PlaybackSessionCommands.requireSuccess(result)
        }
        assertEquals("audio focus denied", error.message)
    }

    @Test
    fun proxyArtworkUsesTheProvidedServerWithoutCallingRust() {
        val state = JSONObject("""{"playbackState":"playing","currentQueueItemId":"queue:1"}""")
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","image":{"proxy_id":"cover","path":"fallback"}}]}""")
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(state, queue, "http://server:8095/")
        val expected = com.example.mashin_flutter.media.artwork.MediaArtworkContentProvider.buildUri("http://server:8095/imageproxy/cover")
        assertEquals(expected, snapshot.mediaItems.single().mediaMetadata.artworkUri.toString())
    }

    @Test
    fun activeLocalPlaybackProjectsItsCanonicalQueueAndBufferedPosition() {
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(
            JSONObject("""{"activeEngineKind":"sendspin","playbackState":"playing","currentQueueItemId":"second","positionMillis":2500,"bufferedPositionMillis":1000,"volume":150,"muted":true}"""),
            JSONObject("""{"items":[{"queue_item_id":"first","name":"First"},{"queue_item_id":"second","name":"Second","duration":60}],"repeatMode":"one","shuffleEnabled":true}"""),
        )
        assertEquals(PlaybackStatus.Playing, snapshot.status)
        assertTrue(snapshot.hasPlaybackIntent)
        assertTrue(snapshot.isLocalPlayback)
        assertEquals(Player.STATE_READY, snapshot.playerState)
        assertEquals(1, snapshot.currentIndex)
        assertEquals(60_000L, snapshot.durationMillis)
        assertEquals(2500L, snapshot.bufferedPositionMillis)
        assertEquals(100, snapshot.volume)
        assertTrue(snapshot.isMuted)
        assertTrue(snapshot.isShuffleEnabled)
    }

    @Test
    fun albumMetadataCoverHasTheSamePrecedenceAsFlutter() {
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","image":{"path":"https://images/track.jpg"},"media_item":{"album":{"metadata":{"images":[{"type":"thumb","proxy_id":"album-cover","path":"album.jpg"}]}}}}]}""")
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(JSONObject("{}"), queue, "http://server:8095")
        val source = snapshot.mediaItems.single().mediaMetadata.artworkUri?.getQueryParameter("src")
        assertEquals("http://server:8095/imageproxy/album-cover", source)
    }

    @Test
    fun thumbnailIsPreferredOverOtherMetadataImages() {
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","media_item":{"metadata":{"images":[{"type":"fanart","path":"https://images/fanart.jpg"},{"type":"thumb","path":"https://images/cover.jpg"}]}}}]}""")
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(JSONObject("{}"), queue)
        assertEquals("https://images/cover.jpg", snapshot.mediaItems.single().mediaMetadata.artworkUri?.getQueryParameter("src"))
    }

    @Test
    fun unusableAlbumAndQueueImagesDoNotHideTheTrackCover() {
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","image":{"path":"null"},"media_item":{"album":{"metadata":{"images":[{}, {"path":"provider-only-cover.jpg"}]}},"image":{"path":"https://images/track.jpg"}}}]}""")
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(JSONObject("{}"), queue)
        assertEquals("https://images/track.jpg", snapshot.mediaItems.single().mediaMetadata.artworkUri?.getQueryParameter("src"))
    }

    @Test
    fun absentArtworkDoesNotInventAnImageUri() {
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","media_item":{"album":{"metadata":{"images":[]}}}}]}""")
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(JSONObject("{}"), queue)
        assertNull(snapshot.mediaItems.single().mediaMetadata.artworkUri)
    }

    @Test
    fun remoteAndEmptyBufferingStatesNeverInventALocalTimeline() {
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(
            JSONObject("""{"activeEngineKind":"remote","playbackState":"buffering"}"""), JSONObject("{}"),
        )
        assertFalse(snapshot.isLocalPlayback)
        assertTrue(snapshot.hasPlaybackIntent)
        assertEquals(Player.STATE_IDLE, snapshot.playerState)
        assertTrue(snapshot.mediaItems.isEmpty())
    }

    @Test
    fun sessionCommandRoundTripRejectsRemovedActionNames() {
        val request = PlaybackRequest(PlaybackCommandKind.TransportSeek, """{"seconds":2.5}""")
        assertEquals(request, PlaybackSessionCommands.decode(PlaybackSessionCommands.encode(request)))
        assertThrows(IllegalArgumentException::class.java) {
            PlaybackSessionCommands.decode(Bundle().apply { putString(PlaybackSessionCommands.COMMAND_KEY, "seekSeconds") })
        }
    }
}