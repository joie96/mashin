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
    fun playerMetadataSeparatesTrackTitleFromQueueDisplayNameAndArtists() {
        val state = JSONObject("""{"currentQueueItemId":"queue:1","playbackState":"playing"}""")
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","name":"First Artist, Second Artist - Song - Live","media_item":{"media_type":"track","name":"Song - Live","artists":[{"name":"First Artist"},{"name":"Second Artist"}]}}]}""")
        val metadata = PlaybackSnapshotMapper.fromPlayerState(state, queue).mediaItems.single().mediaMetadata
        assertEquals("Song - Live", metadata.title)
        assertEquals("First Artist, Second Artist", metadata.artist)
        assertEquals("queue:1", PlaybackSnapshotMapper.fromPlayerState(state, queue).mediaItems.single().mediaId)
    }

    @Test
    fun requestedTrackMetadataKeepsTitleAndArtistSeparateWithoutAQueue() {
        val state = JSONObject("""{"currentQueueItemId":"queue:1","playbackState":"buffering","requestedMedia":{"media_type":"track","name":"Chosen Song","artists":[{"name":"Chosen Artist"}]}}""")
        val metadata = PlaybackSnapshotMapper.fromPlayerState(state, JSONObject("{}")).mediaItems.single().mediaMetadata
        assertEquals("Chosen Song", metadata.title)
        assertEquals("Chosen Artist", metadata.artist)
    }

    @Test
    fun missingTrackTitleRetainsQueueNameWithoutInventingAnArtist() {
        for (mediaItem in listOf("{}", """{"name":""}""", """{"name":"   "}""")) {
            val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","name":"Queue title","media_item":$mediaItem}]}""")
            val metadata = PlaybackSnapshotMapper.fromPlayerState(JSONObject("{}"), queue).mediaItems.single().mediaMetadata
            assertEquals("Queue title", metadata.title)
            assertNull(metadata.artist)
        }
    }

    @Test
    fun playerStateUpdatesRetainQueueMetadataAndFavorites() {
        val state = JSONObject("""{"activeMode":"local","playbackState":"playing","currentQueueItemId":"queue:1","durationMillis":60000,"positionMillis":1000}""")
        val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1","name":"Track","media_item":{"media_type":"track","item_id":"1","uri":"library://track/1"}}],"shuffleEnabled":true,"repeatMode":"one"}""")
        val userData = JSONObject("""{"favorites":{"tracks":[{"uri":"library://track/1"}]}}""")
        val previous = PlaybackSnapshotMapper.fromPlayerState(state, queue, userData)
        for (status in listOf("playing", "paused", "seeking", "buffering")) {
            state.put("playbackState", status).put("positionMillis", 2500).put("bufferedPositionMillis", 3000)
                .put("volume", 75).put("muted", true)
            val updated = requireNotNull(PlaybackSnapshotMapper.updatePlayerState(previous, state))
            assertSame(previous.mediaItems, updated.mediaItems)
            assertEquals(2500L, updated.positionMillis)
            assertEquals(3000L, updated.bufferedPositionMillis)
            assertEquals(PlaybackSnapshotMapper.fromPlayerState(state, queue, userData), updated)
        }
    }

    @Test
    fun presentationChangesRequireFullSnapshotMapping() {
        val state = JSONObject("""{"currentQueueItemId":"queue:1","durationMillis":60000,"requestedMedia":{"name":"Chosen"}}""")
        val previous = PlaybackSnapshotMapper.fromPlayerState(state, JSONObject("{}"))
        assertNotNull(PlaybackSnapshotMapper.updatePlayerState(previous, state))
        for (change in listOf(
            JSONObject(state.toString()).put("currentQueueItemId", "queue:2"),
            JSONObject(state.toString()).put("durationMillis", 90000),
            JSONObject(state.toString()).apply { remove("durationMillis") },
            JSONObject(state.toString()).put("requestedMedia", JSONObject("""{"name":"Other"}""")),
            JSONObject(state.toString()).apply { remove("requestedMedia") },
        )) assertNull(PlaybackSnapshotMapper.updatePlayerState(previous, change))
    }

    @Test
    fun absentBufferValuesUseQueueCacheOrCurrentPositionInsteadOfPreviousReports() {
        for (cache in listOf("", """, "cache":{"cached_duration_millis":4000}""")) {
            val state = JSONObject("""{"currentQueueItemId":"queue:1","bufferedPositionMillis":10000}""")
            val queue = JSONObject("""{"items":[{"queue_item_id":"queue:1"$cache}]}""")
            val previous = PlaybackSnapshotMapper.fromPlayerState(state, queue)
            state.remove("bufferedPositionMillis")
            state.put("positionMillis", 2500)
            assertEquals(PlaybackSnapshotMapper.fromPlayerState(state, queue),
                PlaybackSnapshotMapper.updatePlayerState(previous, state))
        }
    }

    @Test
    fun requestedTrackReplacesOnlyPresentationMetadataWhileBuffering() {
        val queue = JSONObject("""{"items":[{"queue_item_id":"old","name":"Old","media_item":{"media_type":"track","item_id":"old","provider":"source","uri":"source://track/old","duration":60}}]}""")
        val before = queue.toString()
        val state = JSONObject("""{"playbackState":"buffering","currentQueueItemId":"old","positionMillis":0,"durationMillis":180000,"requestedMedia":{"media_type":"track","item_id":"chosen","provider":"source","uri":"source://track/chosen","name":"Chosen","duration":180}}""")
        val snapshot = PlaybackSnapshotMapper.fromPlayerState(state, queue, JSONObject("{}"))
        assertEquals(PlaybackStatus.Buffering, snapshot.status)
        assertEquals("Chosen", snapshot.mediaItems.single().mediaMetadata.title)
        assertEquals("old", snapshot.mediaItems.single().mediaId)
        assertEquals(0L, snapshot.positionMillis)
        assertEquals(180000L, snapshot.durationMillis)
        assertEquals("chosen", JSONObject(snapshot.serializedCurrentMediaItem!!).getString("item_id"))
        assertEquals(before, queue.toString())
        val empty = PlaybackSnapshotMapper.fromPlayerState(state, JSONObject("{}"))
        assertEquals("Chosen", empty.mediaItems.single().mediaMetadata.title)
        assertEquals(Player.STATE_BUFFERING, empty.playerState)
        state.remove("requestedMedia")
        assertEquals("Old", PlaybackSnapshotMapper.fromPlayerState(state, queue).mediaItems.single().mediaMetadata.title)
    }

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