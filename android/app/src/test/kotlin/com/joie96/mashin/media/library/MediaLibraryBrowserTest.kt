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
    private val queryFixture = BlockingSearchFixture()
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
        val queryCore: (String, String?) -> String? = { kind, argument ->
            if (kind == "recommendations") {
                recommendationViews.add(JSONObject(checkNotNull(argument)).getString("view"))
                JSONObject().put("folders", folders).put("is_loading", isLoadingRecommendations).toString()
            } else if (kind == "getUserData") {
                userData.toString()
            } else if (kind == "recentlyPlayedTracks") {
                "[]"
            } else {
                val reference = JSONObject(checkNotNull(argument))
                references.add(kind to reference)
                results["$kind:${reference.getString("itemId")}"]?.toString() ?: "[]"
            }
        }
        browser = MediaLibraryBrowser(context, query = queryCore,
            catalogQuery = { kind, argument -> Futures.immediateFuture(checkNotNull(queryCore(kind, argument))) }) {
            request -> requests.add(request); Futures.immediateVoidFuture()
        }
        player = RustSessionPlayer({ Futures.immediateVoidFuture() }, browser::startPlayback, browser::resolveLibraryMediaItem)
        session = MediaLibrarySession.Builder(context, player, browser).build()
    }

    @After
    fun shutdown() {
        session.release()
        player.release()
        browser.close()
        queryFixture.close()
    }

    // Discover categories preserve the Flutter feed order and expose their actual media.
    @Test
    fun failedSearchWithoutCachedValuesReportsAnErrorInsteadOfEmptySuccess() {
        val search = queryFixture.search { _, _ -> error("Offline search") }
        val searchBrowser = MediaLibraryBrowser(RuntimeEnvironment.getApplication(), search = search) {
            Futures.immediateVoidFuture()
        }
        try {
            searchBrowser.onSearch(session, controller, "jazz", null).get(5, TimeUnit.SECONDS)
            val result = searchBrowser.onGetChildren(session, controller, "search:tracks?jazz", 0, 10, null)
                .get(5, TimeUnit.SECONDS)
            assertEquals(LibraryResult.RESULT_ERROR_UNKNOWN, result.resultCode)
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
        } finally {
            searchBrowser.close()
        }
    }

    @Test
    fun cachedSearchTracksReachMedia3BeforeRefreshAndLaterValuesReplaceThem() {
        val started = CountDownLatch(4)
        val releaseRefresh = CountDownLatch(1)
        val search = queryFixture.search { _, _ ->
            started.countDown()
            check(releaseRefresh.await(5, TimeUnit.SECONDS))
            error("Offline refresh")
        }
        val searchBrowser = MediaLibraryBrowser(RuntimeEnvironment.getApplication(), search = search) {
            Futures.immediateVoidFuture()
        }
        try {
            val canceled = searchBrowser.onGetChildren(session, controller, "search:tracks?jazz", 0, Int.MAX_VALUE, null)
            val pending = searchBrowser.onGetChildren(session, controller, "search:tracks?jazz", 0, Int.MAX_VALUE, null)
            assertTrue(started.await(5, TimeUnit.SECONDS))
            canceled.cancel(false)
            assertFalse("One browser cancellation must not cancel another consumer", pending.isCancelled)
            for (name in listOf("Cached", "Refreshed")) {
                val event = JSONObject().put("type", "catalog_updated")
                    .put("query", JSONObject().put("Search", JSONObject()
                        .put("query", "jazz").put("media_type", "track").put("limit", 50)))
                    .put("value", JSONObject().put("Search", JSONObject()
                        .put("tracks", JSONArray().put(item(name, "track")))))
                searchBrowser.onCoreEvent("catalog_updated", catalogEvent = event)
                val result = if (name == "Cached") pending else searchBrowser.onGetChildren(
                    session, controller, "search:tracks?jazz", 0, Int.MAX_VALUE, null)
                assertEquals(listOf("Abspielen", "Zufällige Wiedergabe", name),
                    checkNotNull(result.get(5, TimeUnit.SECONDS).value).map { it.mediaMetadata.title.toString() })
            }
            val page = searchBrowser.onGetChildren(session, controller, "search:tracks?jazz", 1, 2, null)
            assertEquals(listOf("Refreshed"), checkNotNull(page.get(5, TimeUnit.SECONDS).value)
                .map { it.mediaMetadata.title.toString() })
        } finally {
            releaseRefresh.countDown()
            searchBrowser.close()
        }
    }

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
            publishRecommendations()
            val result = pending.get(5, TimeUnit.SECONDS)
            assertEquals(LibraryResult.RESULT_SUCCESS, result.resultCode)
            assertEquals(parent, checkNotNull(result.value).last().mediaMetadata.title)
        }
    }

    @Test
    fun partialRecommendationFoldersShowAvailableItemsAndCompletedEmptyFoldersDoNotWait() {
        isLoadingRecommendations = true
        val loadingFolder = folder("user_recommendations", item("partial", "track")).put("is_loading", true)
        folders.put(loadingFolder)
        val pending = browser.onGetChildren(session, controller, "home:recommendations", 0, Int.MAX_VALUE, null)
        assertEquals("partial", checkNotNull(pending.get(5, TimeUnit.SECONDS).value).last().mediaMetadata.title)
        loadingFolder.put("is_loading", false)
        publishRecommendations()
        assertEquals("partial", checkNotNull(pending.get(5, TimeUnit.SECONDS).value).last().mediaMetadata.title)

        folders.put(folder("sitewide_top_albums_weekly"))
        publishRecommendations()
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
        val remote = com.google.common.util.concurrent.SettableFuture.create<String>()
        val blockedBrowser = MediaLibraryBrowser(RuntimeEnvironment.getApplication(), catalogQuery = { _, _ ->
            queryStarted.countDown()
            remote
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
            remote.set("[]")
            pending.get(5, TimeUnit.SECONDS)
        } finally {
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
        publishRecommendations()
        val songs = children("discover:tracks")
        assertEquals(listOf("Abspielen", "Zufällige Wiedergabe", "song"),
            songs.map { it.mediaMetadata.title.toString() })
        assertTrue(children("discover:exploration").isEmpty())
        folders.put(folder("user_exploration_weekly", item("empty-playlist", "playlist")))
        publishRecommendations()
        assertTrue(children("discover:exploration").isEmpty())
    }

    // Fixtures cross the same JSON boundary as Rust query results.
    private fun publishRecommendations() {
        listOf("home", "explore").forEach { view ->
            browser.onCoreEvent("recommendations_updated", catalogEvent = JSONObject()
                .put("type", "recommendations_updated").put("view", view)
                .put("snapshot", JSONObject().put("folders", folders).put("is_loading", isLoadingRecommendations)))
        }
    }

    @Test
    fun nativeRecentReadPublishesBeforeCompletionAndLastControllerDisconnectCancelsIt() {
        val started = CountDownLatch(1)
        val nativeIds = mutableListOf<Long>()
        val canceledIds = mutableListOf<Long>()
        val nativeRequests = com.joie96.mashin.core.NativeQueryRequests({ id, kind, _ ->
            assertEquals("recentlyPlayedTracks", kind)
            nativeIds.add(id); started.countDown(); true
        }, { id -> canceledIds.add(id); true })
        val recentBrowser = MediaLibraryBrowser(RuntimeEnvironment.getApplication(), queryRequests = nativeRequests) {
            Futures.immediateVoidFuture()
        }
        val other = MediaSession.ControllerInfo.createTestOnlyControllerInfo(
            "other", 2, 1001, 0, 0, true, Bundle.EMPTY, true)
        try {
            val first = recentBrowser.onGetChildren(session, controller, "home:recent", 0, 10, null)
            assertTrue(started.await(5, TimeUnit.SECONDS))
            val second = recentBrowser.onGetChildren(session, other, "home:recent", 1, 2, null)
            first.cancel(false)
            assertTrue(canceledIds.isEmpty())
            val event = JSONObject().put("type", "catalog_updated")
                .put("query", JSONObject().put("RecentlyPlayed", JSONObject().put("limit", 50)))
                .put("value", JSONObject().put("Tracks", JSONArray().put(item("cached", "track").apply { remove("media_type") })))
            recentBrowser.onCoreEvent("catalog_updated", catalogEvent = event)
            assertEquals(listOf("cached"), checkNotNull(second.get(5, TimeUnit.SECONDS).value)
                .map { it.mediaMetadata.title.toString() })
            assertTrue(canceledIds.isEmpty())
            recentBrowser.onDisconnected(session, controller)
            assertTrue(canceledIds.isEmpty())
            recentBrowser.onDisconnected(session, other)
            assertEquals(nativeIds, canceledIds)
            recentBrowser.onCoreEvent("catalog_updated", catalogEvent = event)
        } finally { recentBrowser.close() }
    }

    @Test
    fun partialFavoriteAndPrivateTrackSnapshotsAreVisibleWhileTheirRefreshIsLoading() {
        userData = JSONObject().put("is_loading_favorites", true).put("favorites", JSONObject()
            .put("albums", JSONArray().put(item("cached-album", "album"))))
        assertEquals("cached-album", children("favorites:albums").single().mediaMetadata.title)
        val playlist = item("private", "playlist").put("provider", "library").put("is_loading_tracks", true)
            .put("items", JSONArray().put(item("cached-track", "track")))
        userData.put("playlists", JSONArray().put(playlist)).put("is_loading_playlists", true)
        val parent = children("node:playlists").single().mediaId
        assertEquals("cached-track", children(parent).last().mediaMetadata.title)
        assertTrue(references.isEmpty())
    }

    @Test
    fun subscriptionSurvivesCanceledPageUntilUnsubscribeAndSessionChangeRetiresRequests() {
        val started = java.util.concurrent.LinkedBlockingQueue<Long>()
        val canceled = java.util.concurrent.LinkedBlockingQueue<Long>()
        val nativeRequests = com.joie96.mashin.core.NativeQueryRequests({ id, _, _ -> started.add(id); true },
            { id -> canceled.add(id); true })
        val scoped = MediaLibraryBrowser(RuntimeEnvironment.getApplication(), queryRequests = nativeRequests) { Futures.immediateVoidFuture() }
        try {
            scoped.onSubscribe(session, controller, "home:recent", null).get()
            val page = scoped.onGetChildren(session, controller, "home:recent", 0, 10, null)
            val first = checkNotNull(started.poll(5, TimeUnit.SECONDS))
            page.cancel(false)
            assertTrue(canceled.isEmpty())
            scoped.onUnsubscribe(session, controller, "home:recent").get()
            assertEquals(first, canceled.poll(5, TimeUnit.SECONDS))
            val pending = scoped.onGetChildren(session, controller, "home:recent", 0, 10, null)
            val second = checkNotNull(started.poll(5, TimeUnit.SECONDS))
            scoped.onCoreEvent("session_changed")
            assertTrue(pending.isCancelled)
            assertEquals(second, canceled.poll(5, TimeUnit.SECONDS))
            scoped.onCoreEvent("android_query_completed", catalogEvent = JSONObject()
                .put("type", "android_query_completed").put("request_id", second).put("result", JSONArray()))
            assertTrue(pending.isCancelled)
        } finally { scoped.close() }
    }

    @Test
    fun newControllerSearchStartsImmediatelyAndRetainsOtherControllersOldSearch() {
        val started = mutableListOf<Long>()
        val canceled = mutableListOf<Long>()
        val nativeRequests = com.joie96.mashin.core.NativeQueryRequests({ id, _, _ -> started.add(id); true },
            { id -> canceled.add(id); true })
        val scoped = MediaLibraryBrowser(RuntimeEnvironment.getApplication(), queryRequests = nativeRequests) { Futures.immediateVoidFuture() }
        val other = MediaSession.ControllerInfo.createTestOnlyControllerInfo("other", 2, 1001, 0, 0, true, Bundle.EMPTY, true)
        try {
            scoped.onSearch(session, controller, "old", null).get()
            scoped.onSearch(session, other, "old", null).get()
            assertEquals(4, started.size)
            val oldPage = scoped.onGetChildren(session, controller, "search:tracks?old", 0, 10, null)
            scoped.onSearch(session, controller, "new", null).get()
            assertEquals(8, started.size)
            assertTrue(oldPage.isCancelled)
            assertTrue(canceled.isEmpty())
            scoped.onDisconnected(session, other)
            assertEquals(started.take(4).toSet(), canceled.toSet())
        } finally { scoped.close() }
        assertEquals(started.toSet(), canceled.toSet())
    }

    @Test
    fun failedNativeMissFinishesWaitingBrowserWithErrorNotEmptySuccess() {
        val remote = com.google.common.util.concurrent.SettableFuture.create<String>()
        val started = CountDownLatch(1)
        val scoped = MediaLibraryBrowser(RuntimeEnvironment.getApplication(), catalogQuery = { _, _ -> started.countDown(); remote }) {
            Futures.immediateVoidFuture()
        }
        try {
            val pending = scoped.onGetChildren(session, controller, "home:recent", 0, 10, null)
            assertTrue(started.await(5, TimeUnit.SECONDS))
            remote.setException(IllegalStateException("offline"))
            assertEquals(LibraryResult.RESULT_ERROR_UNKNOWN, pending.get(5, TimeUnit.SECONDS).resultCode)
        } finally { scoped.close() }
    }

    @Test
    fun albumPlaylistAndArtistSectionsDisplayTypedCacheBeforeEveryRemoteCompletion() {
        val started = java.util.concurrent.LinkedBlockingQueue<Pair<String, com.google.common.util.concurrent.SettableFuture<String>>>()
        val snapshot = JSONObject().put("favorites", JSONObject()
            .put("albums", JSONArray().put(item("album", "album")))
            .put("playlists", JSONArray().put(item("playlist", "playlist")))
            .put("artists", JSONArray().put(item("artist", "artist"))))
        val scoped = MediaLibraryBrowser(RuntimeEnvironment.getApplication(), query = { _, _ -> snapshot.toString() },
            catalogQuery = { kind, _ -> com.google.common.util.concurrent.SettableFuture.create<String>().also { started.add(kind to it) } }) {
            Futures.immediateVoidFuture()
        }
        try {
            listOf("albums", "playlists", "artists").forEach { category ->
                scoped.onGetChildren(session, controller, "favorites:$category", 0, 10, null).get(5, TimeUnit.SECONDS)
            }
            for ((parent, kind, id) in listOf(
                Triple("browse:album:test:album", LibraryReadKind.AlbumTracks, "album"),
                Triple("browse:playlist:test:playlist", LibraryReadKind.PlaylistTracks, "playlist"),
                Triple("artistAlbums:browse:artist:test:artist", LibraryReadKind.ArtistAlbums, "artist"),
                Triple("artistTopTracks:browse:artist:test:artist", LibraryReadKind.ArtistTopTracks, "artist"),
                Triple("similarArtists:browse:artist:test:artist", LibraryReadKind.SimilarArtists, "artist"),
            )) {
                val pending = scoped.onGetChildren(session, controller, parent, 0, Int.MAX_VALUE, null)
                val (command, remote) = checkNotNull(started.poll(5, TimeUnit.SECONDS))
                assertEquals(kind.command, command)
                val event = JSONObject().put("type", "catalog_updated")
                    .put("query", JSONObject().put(kind.catalog, JSONObject().put("item_id", id).put("provider", "test").put("limit", 50)))
                    .put("value", JSONObject().put(kind.value, JSONArray().put(item("cached", kind.mediaType).apply { remove("media_type") })))
                scoped.onCoreEvent("catalog_updated", catalogEvent = event)
                assertEquals("cached", checkNotNull(pending.get(5, TimeUnit.SECONDS).value).last().mediaMetadata.title)
                assertFalse(remote.isDone)
                event.put("value", JSONObject().put(kind.value, JSONArray()))
                scoped.onCoreEvent("catalog_updated", catalogEvent = event)
                remote.setException(IllegalStateException("offline"))
                assertTrue(checkNotNull(scoped.onGetChildren(session, controller, parent, 0, 10, null)
                    .get(5, TimeUnit.SECONDS).value).isEmpty())
            }
        } finally { scoped.close() }
    }

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