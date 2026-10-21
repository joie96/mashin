package com.example.mashin_flutter.media.library

import android.os.Bundle
import androidx.media3.common.MediaItem
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaConstants
import androidx.media3.session.MediaLibraryService.MediaLibrarySession
import androidx.media3.session.MediaSession
import com.example.mashin_flutter.media.session.PlaybackCommandKind
import com.example.mashin_flutter.media.session.PlaybackRequest
import com.example.mashin_flutter.media.session.RustSessionPlayer
import com.google.common.util.concurrent.Futures
import java.util.concurrent.TimeUnit
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
@LooperMode(LooperMode.Mode.PAUSED)
class MediaLibraryBrowserTest {
    private val folders = JSONArray()
    private val requests = mutableListOf<PlaybackRequest>()
    private val references = mutableListOf<Pair<String, JSONObject>>()
    private val results = mutableMapOf<String, JSONArray>()
    private lateinit var browser: MediaLibraryBrowser
    private lateinit var player: RustSessionPlayer
    private lateinit var session: MediaLibrarySession
    private val controller = MediaSession.ControllerInfo.createTestOnlyControllerInfo(
        "com.google.android.projection.gearhead", 1, 1000, 0, 0, true, Bundle.EMPTY, true)

    // The native query boundary is replaced without changing browsing or playback dispatch.
    @Before
    fun initialize() {
        val context = RuntimeEnvironment.getApplication()
        browser = MediaLibraryBrowser(context, query = { kind, argument ->
            if (kind == "recommendations") folders.toString() else {
                val reference = JSONObject(checkNotNull(argument))
                references.add(kind to reference)
                results["$kind:${reference.getString("itemId")}"]?.toString() ?: "[]"
            }
        }) { request -> requests.add(request); Futures.immediateVoidFuture() }
        player = RustSessionPlayer({ Futures.immediateVoidFuture() }, browser::startPlayback, browser::resolveLibraryMediaItem)
        session = MediaLibrarySession.Builder(context, player, browser).build()
    }

    @After
    fun shutdown() {
        session.release()
        player.release()
        browser.close()
    }

    // Discover categories preserve the Flutter feed order and expose their actual media.
    @Test
    fun discoverListsTheFiveRequestedSectionsInOrder() {
        val sections = children("node:discover")
        assertEquals(listOf("Entdecke etwas Neues", "Alben der Woche", "Andere Nutzer hören",
            "Artists der Woche", "Songs der Woche"), sections.map { it.mediaMetadata.title.toString() })
        assertTrue(sections.all { it.mediaMetadata.isBrowsable == true && it.mediaMetadata.isPlayable == false })
        assertEquals(children("node:home").first().mediaMetadata.artworkUri, sections[0].mediaMetadata.artworkUri)
        assertEquals("android.resource://com.example.mashin_flutter/drawable/groups",
            sections[2].mediaMetadata.artworkUri.toString())
        assertEquals(MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM,
            sections[1].mediaMetadata.extras?.getInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE))
        assertEquals(MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM,
            sections[3].mediaMetadata.extras?.getInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE))
        assertTrue(references.isEmpty())
    }

    @Test
    fun explorationAndSimilarUsersExpandEveryPlaylistAndKeepPlaybackActions() {
        for ((section, folderId) in listOf("discover:exploration" to "user_exploration_weekly",
            "discover:similarUsers" to "similarusers_jam_weekly")) {
            folders.put(folder(folderId, item("first", "playlist"), item("ignored", "track")))
            folders.put(folder(folderId, item("second", "playlist"), item("", "playlist")))
            results["playlistTracks:first"] = JSONArray().put(item("first-track", "track"))
            results["playlistTracks:second"] = JSONArray().put(item("second-track", "track"))
            references.clear()
            val tracks = children(section)
            assertEquals(listOf("Abspielen", "Zufällige Wiedergabe", "first-track", "second-track"),
                tracks.map { it.mediaMetadata.title.toString() })
            assertEquals(listOf("first", "second"), references.map { it.second.getString("itemId") })
            assertTrue(references.all { it.first == "playlistTracks" && it.second.getString("provider") == "test" })
            assertTrue(tracks.all { it.mediaMetadata.isPlayable == true })
            browser.startPlayback(tracks[0].mediaId).get()
            browser.startPlayback(tracks[1].mediaId).get()
            val playRequest = JSONObject(requests[requests.size - 2].arguments).getJSONObject("request")
            assertEquals(2, playRequest.getJSONArray("mediaItems").length())
            assertFalse(playRequest.getBoolean("shuffle"))
            assertTrue(JSONObject(requests.last().arguments).getJSONObject("request").getBoolean("shuffle"))
            browser.startPlayback(tracks.last().mediaId).get()
            assertEquals(PlaybackCommandKind.StartRadio, requests.last().kind)
        }
    }

    @Test
    fun weeklyAlbumsAndArtistsExposeTheirNestedContentsAndPlayback() {
        folders.put(folder("sitewide_top_albums_weekly", item("album", "album"), item("wrong", "artist")))
        folders.put(folder("sitewide_top_artists_weekly", item("artist", "artist")))
        results["albumTracks:album"] = JSONArray().put(item("album-track", "track"))
        results["artistAlbums:artist"] = JSONArray().put(item("album", "album"))
        results["artistTopTracks:artist"] = JSONArray().put(item("artist-track", "track"))
        results["similarArtists:artist"] = JSONArray().put(item("similar", "artist"))
        val album = children("discover:albums").single()
        assertTrue(album.mediaMetadata.isBrowsable == true)
        val tracks = children(album.mediaId)
        assertEquals("album-track", tracks.last().mediaMetadata.title)
        browser.startPlayback(tracks.first().mediaId).get()
        assertEquals(PlaybackCommandKind.QueuePlayMedia, requests.last().kind)

        val artist = children("discover:artists").single()
        val artistSections = children(artist.mediaId)
        assertEquals(listOf("Artist-Radio", "Alben", "Ähnliche Artists", "Top-Tracks"),
            artistSections.map { it.mediaMetadata.title.toString() })
        browser.startPlayback(artistSections[0].mediaId).get()
        assertEquals(PlaybackCommandKind.StartRadio, requests.last().kind)
        assertEquals(album.mediaId, children(artistSections[1].mediaId).single().mediaId)
        assertEquals("similar", children(artistSections[2].mediaId).single().mediaMetadata.title)
        assertEquals("artist-track", children(artistSections[3].mediaId).last().mediaMetadata.title)
    }

    @Test
    fun weeklySongsFilterInvalidEntriesAndMissingRecommendationsStayEmpty() {
        assertTrue(children("discover:tracks").isEmpty())
        folders.put(folder("sitewide_top_tracks_weekly", item("song", "track"), item("wrong", "album"),
            item("", "track"), item("missing-provider", "track").put("provider", "")))
        val songs = children("discover:tracks")
        assertEquals(listOf("Abspielen", "Zufällige Wiedergabe", "song"),
            songs.map { it.mediaMetadata.title.toString() })
        assertTrue(children("discover:exploration").isEmpty())
        folders.put(folder("user_exploration_weekly", item("empty-playlist", "playlist")))
        assertTrue(children("discover:exploration").isEmpty())
    }

    // Fixtures cross the same JSON boundary as Rust query results.
    private fun children(parentId: String): List<MediaItem> {
        val result = browser.onGetChildren(session, controller, parentId, 0, Int.MAX_VALUE, null)
            .get(5, TimeUnit.SECONDS)
        assertEquals(LibraryResult.RESULT_SUCCESS, result.resultCode)
        return checkNotNull(result.value)
    }

    private fun item(itemId: String, mediaType: String): JSONObject = JSONObject()
        .put("item_id", itemId).put("media_type", mediaType).put("provider", "test").put("name", itemId)

    private fun folder(folderId: String, vararg items: JSONObject): JSONObject = JSONObject()
        .put("item_id", folderId).put("items", JSONArray(items.toList()))
}