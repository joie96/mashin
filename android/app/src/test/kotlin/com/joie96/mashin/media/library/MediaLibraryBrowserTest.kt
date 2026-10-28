package com.joie96.mashin.media.library

import android.os.Bundle
import androidx.media3.common.MediaItem
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaConstants
import androidx.media3.session.MediaLibraryService.MediaLibrarySession
import androidx.media3.session.MediaSession
import com.joie96.mashin.media.session.PlaybackCommandKind
import com.joie96.mashin.media.session.PlaybackRequest
import com.joie96.mashin.media.session.RustSessionPlayer
import com.google.common.util.concurrent.Futures
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
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
    private val recommendationViews = mutableListOf<String>()
    private val results = mutableMapOf<String, JSONArray>()
    @Volatile private var isLoadingRecommendations = false
    @Volatile private var userData = JSONObject()
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
            if (kind == "recommendations") {
                recommendationViews.add(JSONObject(checkNotNull(argument)).getString("view"))
                JSONObject().put("folders", folders).put("is_loading", isLoadingRecommendations).toString()
            } else if (kind == "userData") {
                userData.toString()
            } else {
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
    fun allRecommendationSubmenusWaitForTheirDataAndFinishOnCoreEvents() {
        for ((parent, folderId, mediaType) in listOf(
            Triple("home:recommendations", "user_recommendations", "track"),
            Triple("home:favoriteGenres", "user_genre_playlists_monthly", "playlist"),
            Triple("home:playlistSuggestions", "user_artist_playlists_monthly", "playlist"),
            Triple("home:topTracks", "user_top_tracks_monthly", "track"),
            Triple("home:topArtists", "user_top_artists_monthly", "artist"),
            Triple("discover:exploration", "user_exploration_weekly", "track"),
            Triple("discover:albums", "sitewide_top_albums_weekly", "album"),
            Triple("discover:similarUsers", "similarusers_jam_weekly", "track"),
            Triple("discover:artists", "sitewide_top_artists_weekly", "artist"),
            Triple("discover:tracks", "sitewide_top_tracks_weekly", "track"),
        )) {
            isLoadingRecommendations = true
            val pending = browser.onGetChildren(session, controller, parent, 0, Int.MAX_VALUE, null)
            assertThrows(TimeoutException::class.java) { pending.get(100, TimeUnit.MILLISECONDS) }
            folders.put(folder(folderId, item(parent, mediaType)))
            browser.onCoreEvent("recommendations_changed")
            val result = pending.get(5, TimeUnit.SECONDS)
            assertEquals(LibraryResult.RESULT_SUCCESS, result.resultCode)
            assertEquals(parent, checkNotNull(result.value).last().mediaMetadata.title)
        }
    }

    @Test
    fun partialRecommendationFoldersWaitButCompletedEmptyFoldersDoNot() {
        isLoadingRecommendations = true
        val loadingFolder = folder("user_recommendations", item("partial", "track")).put("is_loading", true)
        folders.put(loadingFolder)
        val pending = browser.onGetChildren(session, controller, "home:recommendations", 0, Int.MAX_VALUE, null)
        assertThrows(TimeoutException::class.java) { pending.get(100, TimeUnit.MILLISECONDS) }
        loadingFolder.put("is_loading", false)
        browser.onCoreEvent("recommendations_changed")
        assertEquals("partial", checkNotNull(pending.get(5, TimeUnit.SECONDS).value).last().mediaMetadata.title)

        folders.put(folder("sitewide_top_albums_weekly"))
        assertTrue(children("discover:albums").isEmpty())
    }

    @Test
    fun favoriteSubmenusWaitWithoutHidingTheirCategories() {
        userData = JSONObject().put("is_loading_favorites", true)
        val categories = listOf("tracks" to "track", "albums" to "album", "playlists" to "playlist", "artists" to "artist")
        val pending = categories.map { (category, _) ->
            browser.onGetChildren(session, controller, "favorites:$category", 0, Int.MAX_VALUE, null)
        }
        pending.forEach { result ->
            assertThrows(TimeoutException::class.java) { result.get(100, TimeUnit.MILLISECONDS) }
        }
        assertEquals(categories.map { "favorites:${it.first}" }, children("node:favorites").map { it.mediaId })
        val favorites = JSONObject()
        categories.forEach { (category, mediaType) -> favorites.put(category, JSONArray().put(item(category, mediaType))) }
        userData = JSONObject().put("is_loading_favorites", false).put("favorites", favorites)
        browser.onCoreEvent("user_data_changed")
        pending.zip(categories).forEach { (result, category) ->
            assertEquals(category.first, checkNotNull(result.get(5, TimeUnit.SECONDS).value).last().mediaMetadata.title)
        }
    }

    @Test
    fun privatePlaylistsExposeMetadataBeforeTheirTracksFinishLoading() {
        userData = JSONObject().put("is_loading_playlists", true)
        val metadata = browser.onGetChildren(session, controller, "node:playlists", 0, Int.MAX_VALUE, null)
        assertThrows(TimeoutException::class.java) { metadata.get(100, TimeUnit.MILLISECONDS) }

        val playlist = item("private", "playlist").put("provider", "library").put("is_loading_tracks", true)
        userData = JSONObject().put("is_loading_playlists", false).put("playlists", JSONArray().put(playlist))
        browser.onCoreEvent("user_data_changed")
        val mediaId = checkNotNull(metadata.get(5, TimeUnit.SECONDS).value).single().mediaId
        val tracks = browser.onGetChildren(session, controller, mediaId, 0, Int.MAX_VALUE, null)
        assertThrows(TimeoutException::class.java) { tracks.get(100, TimeUnit.MILLISECONDS) }

        playlist.put("is_loading_tracks", false).put("items", JSONArray().put(item("private-track", "track")))
        browser.onCoreEvent("user_data_changed")
        assertEquals(listOf("Abspielen", "Zufällige Wiedergabe", "private-track"),
            checkNotNull(tracks.get(5, TimeUnit.SECONDS).value).map { it.mediaMetadata.title.toString() })
        assertTrue(references.isEmpty())
    }

    @Test
    fun finishedEmptyFavoritesDoNotRemainLoading() {
        userData = JSONObject().put("is_loading_favorites", true)
        val pending = browser.onGetChildren(session, controller, "favorites:tracks", 0, Int.MAX_VALUE, null)
        assertThrows(TimeoutException::class.java) { pending.get(100, TimeUnit.MILLISECONDS) }
        userData = JSONObject().put("is_loading_favorites", false)
        browser.onCoreEvent("user_data_changed")
        assertTrue(checkNotNull(pending.get(5, TimeUnit.SECONDS).value).isEmpty())
    }

    @Test
    fun closingBrowserCancelsUnfinishedLoadingRequests() {
        isLoadingRecommendations = true
        val pending = browser.onGetChildren(session, controller, "discover:tracks", 0, Int.MAX_VALUE, null)
        assertThrows(TimeoutException::class.java) { pending.get(100, TimeUnit.MILLISECONDS) }
        browser.close()
        assertTrue(pending.isCancelled)
    }

    @Test
    fun staticMenusRemainImmediateWhileDataQueryIsBlocked() {
        val queryStarted = CountDownLatch(1)
        val releaseQuery = CountDownLatch(1)
        val blockedBrowser = MediaLibraryBrowser(RuntimeEnvironment.getApplication(), query = { _, _ ->
            queryStarted.countDown()
            check(releaseQuery.await(5, TimeUnit.SECONDS))
            "[]"
        }) { Futures.immediateVoidFuture() }
        try {
            val pending = blockedBrowser.onGetChildren(session, controller, "home:recent", 0, Int.MAX_VALUE, null)
            assertTrue(queryStarted.await(5, TimeUnit.SECONDS))
            for ((parent, expected) in listOf(
                "root" to listOf("node:home", "node:discover", "node:playlists", "node:favorites"),
                "node:home" to listOf("home:recommendations", "home:favoriteGenres", "home:playlistSuggestions",
                    "home:topTracks", "home:topArtists", "home:recent"),
                "node:discover" to listOf("discover:exploration", "discover:albums", "discover:similarUsers",
                    "discover:artists", "discover:tracks"),
                "node:favorites" to listOf("favorites:tracks", "favorites:albums", "favorites:playlists", "favorites:artists"),
            )) {
                val result = blockedBrowser.onGetChildren(session, controller, parent, 0, Int.MAX_VALUE, null)
                assertTrue("$parent must not wait for data queries", result.isDone)
                assertEquals(expected, checkNotNull(result.get().value).map { it.mediaId })
            }
            releaseQuery.countDown()
            pending.get(5, TimeUnit.SECONDS)
        } finally {
            releaseQuery.countDown()
            blockedBrowser.close()
        }
    }

    @Test
    fun discoverListsTheFiveRequestedSectionsInOrder() {
        val sections = children("node:discover")
        assertEquals(listOf("Entdecke etwas Neues", "Alben der Woche", "Andere Nutzer hören",
            "Artists der Woche", "Songs der Woche"), sections.map { it.mediaMetadata.title.toString() })
        assertTrue(sections.all { it.mediaMetadata.isBrowsable == true && it.mediaMetadata.isPlayable == false })
        assertEquals(children("node:home").first().mediaMetadata.artworkUri, sections[0].mediaMetadata.artworkUri)
        assertEquals("android.resource://com.joie96.mashin/drawable/groups",
            sections[2].mediaMetadata.artworkUri.toString())
        assertEquals(MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM,
            sections[1].mediaMetadata.extras?.getInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE))
        assertEquals(MediaConstants.EXTRAS_VALUE_CONTENT_STYLE_GRID_ITEM,
            sections[3].mediaMetadata.extras?.getInt(MediaConstants.EXTRAS_KEY_CONTENT_STYLE_BROWSABLE))
        assertTrue(references.isEmpty())
    }

    @Test
    fun recommendationQueriesSelectTheRequiredView() {
        folders.put(folder("user_recommendations", item("home-track", "track")))
        folders.put(folder("sitewide_top_tracks_weekly", item("explore-track", "track")))
        assertEquals("home-track", children("home:recommendations").last().mediaMetadata.title)
        assertEquals("explore-track", children("discover:tracks").last().mediaMetadata.title)
        assertEquals(listOf("home", "explore"), recommendationViews)
    }

    @Test
    fun explorationAndSimilarUsersReadCachedPlaylistTracksAndKeepPlaybackActions() {
        for ((section, folderId) in listOf("discover:exploration" to "user_exploration_weekly",
            "discover:similarUsers" to "similarusers_jam_weekly")) {
            folders.put(folder(folderId, item("first", "playlist"), item("ignored", "album"))
                .put("playlist_tracks", JSONArray().put(item("first-track", "track"))))
            folders.put(folder(folderId, item("second", "playlist"), item("", "playlist"))
                .put("playlist_tracks", JSONArray().put(item("second-track", "track"))))
            references.clear()
            val tracks = children(section)
            assertEquals(listOf("Abspielen", "Zufällige Wiedergabe", "first-track", "second-track"),
                tracks.map { it.mediaMetadata.title.toString() })
            assertTrue(references.isEmpty())
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