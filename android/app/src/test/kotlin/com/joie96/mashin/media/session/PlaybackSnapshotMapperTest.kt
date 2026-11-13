package com.joie96.mashin.media.session

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
    // Canonical state and metadata projection.
    @Test
    fun favoriteStateUsesUserDataUriInsteadOfStaleQueueFlags() {
        val state = JSONObject("""{"currentQueueItemId":"queue:1"}""")
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","media_item":{"media_type":"track","item_id":"track-1","uri":" library://track/track-1 ","favorite":false}}]}""")
        val userData = JSONObject("""{"favorites":{"tracks":[{"uri":"LIBRARY://TRACK/TRACK-1"}]}}""")
        val favorite = PlaybackSnapshotMapper.fromPlayerState(state, queue, userData = userData)
        assertTrue(favorite.isCurrentItemFavorite)
        assertEquals("track-1", JSONObject(favorite.serializedCurrentMediaItem!!).getString("item_id"))
        val removed = PlaybackSnapshotMapper.fromPlayerState(state, queue, userData = JSONObject("{}"))
        assertFalse(removed.isCurrentItemFavorite)
        assertNotNull(removed.serializedCurrentMediaItem)
        assertNull(PlaybackSnapshotMapper.fromPlayerState(state, queue).serializedCurrentMediaItem)
        assertNull(PlaybackSnapshotMapper.fromPlayerState(JSONObject("{}"), queue, userData = userData).serializedCurrentMediaItem)
        queue.getJSONArray("items").getJSONObject(0).getJSONObject("media_item").remove("uri")
        assertNull(PlaybackSnapshotMapper.fromPlayerState(state, queue, userData = userData).serializedCurrentMediaItem)
    }

    @Test
    fun favoriteStateMatchesProviderAliasesWithoutMixingInstancesOrMediaTypes() {
        val state = JSONObject("""{"currentQueueItemId":"queue:1"}""")
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","media_item":{"media_type":"track","item_id":"remote","provider":"source-a","uri":"source-a://track/remote"}}]}""")
        val favorite = JSONObject("""{"media_type":"track","item_id":"42","provider":"library","uri":"library://track/42","provider_mappings":[{"provider_instance":"source-a","item_id":"remote"}]}""")
        val userData = JSONObject().put("favorites", JSONObject().put("tracks", org.json.JSONArray().put(favorite)))
        assertTrue(PlaybackSnapshotMapper.fromPlayerState(state, queue, userData).isCurrentItemFavorite)
        favorite.getJSONArray("provider_mappings").getJSONObject(0).put("provider_instance", "source-b")
        assertFalse(PlaybackSnapshotMapper.fromPlayerState(state, queue, userData).isCurrentItemFavorite)
        favorite.getJSONArray("provider_mappings").getJSONObject(0).put("provider_instance", "source-a")
        favorite.put("media_type", "album")
        assertFalse(PlaybackSnapshotMapper.fromPlayerState(state, queue, userData).isCurrentItemFavorite)
    }

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
    fun proxyArtworkUsesTheNativeImageRequestWithoutCallingRust() {
        val state = JSONObject("""{"playbackState":"playing","currentQueueItemId":"queue:1"}""")
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","image":{"proxy_id":"${"a".repeat(64)}","path":"fallback"}}]}""")
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(state, queue)
        val expected = com.joie96.mashin.media.artwork.MediaArtworkContentProvider.buildUri(queue.getJSONArray("items").getJSONObject(0))
        assertEquals(expected, snapshot.mediaItems.single().mediaMetadata.artworkUri.toString())
    }

    @Test
    fun activeLocalPlaybackProjectsItsCanonicalQueueAndBufferedPosition() {
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(
            JSONObject("""{"activeMode":"local","playbackState":"playing","currentQueueItemId":"second","positionMillis":2500,"bufferedPositionMillis":1000,"volume":150,"muted":true}"""),
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
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","image":{"path":"https://images/track.jpg"},"media_item":{"album":{"metadata":{"images":[{"type":"thumb","proxy_id":"${"b".repeat(64)}","path":"album.jpg"}]}}}}]}""")
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(JSONObject("{}"), queue)
        val source = JSONObject(snapshot.mediaItems.single().mediaMetadata.artworkUri!!.getQueryParameter("request")!!)
        assertEquals("b".repeat(64), source.getJSONObject("image").getString("proxy_id"))
    }

    @Test
    fun thumbnailIsPreferredOverOtherMetadataImages() {
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","media_item":{"metadata":{"images":[{"type":"fanart","path":"data:image/png;base64,AQID"},{"type":"thumb","path":"data:image/png;base64,BAUG"}]}}}]}""")
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(JSONObject("{}"), queue)
        val request = JSONObject(snapshot.mediaItems.single().mediaMetadata.artworkUri!!.getQueryParameter("request")!!)
        assertEquals("data:image/png;base64,BAUG", request.getJSONObject("image").getString("path"))
        assertEquals(1, request.getInt("position"))
    }

    @Test
    fun unusableAlbumAndQueueImagesDoNotHideTheTrackCover() {
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","image":{"path":"null"},"media_item":{"album":{"metadata":{"images":[{}, {"path":"provider-only-cover.jpg"}]}},"image":{"cached_inline":{"owner_uri":"library://playlist/42","position":0}}}}]}""")
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(JSONObject("{}"), queue)
        val request = JSONObject(snapshot.mediaItems.single().mediaMetadata.artworkUri!!.getQueryParameter("request")!!)
        assertEquals("library://playlist/42", request.getJSONObject("image").getJSONObject("cached_inline").getString("owner_uri"))
    }

    @Test
    fun inlineContentChangesUpdateArtworkUriWithoutChangingItsModelSlot() {
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","image":{"proxy_id":"${"a".repeat(64)}","cached_inline":{"owner_uri":"library://playlist/42","position":0,"content_hash":"old-content"}}}]}""")
        val previous = PlaybackSnapshotMapper.fromPlayerState(JSONObject("{}"), queue)
            .mediaItems.single().mediaMetadata.artworkUri!!
        val unchanged = PlaybackSnapshotMapper.fromPlayerState(JSONObject("{}"), queue)
            .mediaItems.single().mediaMetadata.artworkUri!!
        assertEquals(previous, unchanged)
        queue.getJSONArray("items").getJSONObject(0).getJSONObject("image")
            .getJSONObject("cached_inline").put("content_hash", "new-content")
        val replacement = PlaybackSnapshotMapper.fromPlayerState(JSONObject("{}"), queue)
            .mediaItems.single().mediaMetadata.artworkUri!!
        assertNotEquals(previous, replacement)
        val image = JSONObject(replacement.getQueryParameter("request")!!).getJSONObject("image")
        assertEquals("a".repeat(64), image.getString("proxy_id"))
        val reference = image.getJSONObject("cached_inline")
        assertEquals("library://playlist/42", reference.getString("owner_uri"))
        assertEquals(0, reference.getInt("position"))
        assertEquals("new-content", reference.getString("content_hash"))
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
            JSONObject("""{"activeMode":"remote","playbackState":"buffering"}"""), JSONObject("{}"),
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

    @Test
    fun unknownModesNeverAcquireLocalPlaybackOwnership() {
        for (mode in listOf("mashin", "sendspin", "offline", "")) {
            val snapshot = PlaybackSnapshotMapper.fromPlayerState(
                JSONObject().put("activeMode", mode), JSONObject("{}"),
            )
            assertFalse(snapshot.isLocalPlayback)
        }
    }
}