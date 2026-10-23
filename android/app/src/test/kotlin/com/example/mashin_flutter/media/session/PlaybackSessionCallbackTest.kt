package com.example.mashin_flutter.media.session

import android.os.Bundle
import android.os.Looper
import androidx.media3.common.Player
import androidx.media3.session.MediaLibraryService.MediaLibrarySession
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaSession
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionResult
import com.example.mashin_flutter.media.library.MediaLibraryBrowser
import com.example.mashin_flutter.media.library.MediaLibrarySearch
import com.example.mashin_flutter.media.library.SearchMediaCategory
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.SettableFuture
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.time.Duration
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
@LooperMode(LooperMode.Mode.PAUSED)
class PlaybackSessionCallbackTest {
    // Controller browsing and search contracts.
    @Test
    fun legacySelectionPublishesBufferingThenCanonicalPlayback() {
        val context = RuntimeEnvironment.getApplication()
        val browser = MediaLibraryBrowser(context) { Futures.immediateVoidFuture() }
        val completion = SettableFuture.create<Void?>()
        val selections = mutableListOf<String>()
        val requests = mutableListOf<PlaybackRequest>()
        val player = RustSessionPlayer(
            { request -> requests.add(request); Futures.immediateVoidFuture() },
            { mediaId -> selections.add(mediaId); completion },
            browser::resolveLibraryMediaItem,
        )
        val callback = PlaybackSessionCallback(context.packageName, browser) {
            Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
        }
        val session = MediaLibrarySession.Builder(context, player, callback).build()
        val controller = MediaSession.ControllerInfo.createTestOnlyControllerInfo(
            "com.google.android.projection.gearhead", 1, 1000, 0, 0, true, Bundle.EMPTY, true)
        try {
            val resolved = callback.onSetMediaItems(session, controller,
                listOf(androidx.media3.common.MediaItem.Builder().setMediaId("browse:track:library:track-1").build()),
                androidx.media3.common.C.INDEX_UNSET, androidx.media3.common.C.TIME_UNSET).get()
            player.setMediaItems(resolved.mediaItems, true)
            player.prepare()
            player.play()
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals(listOf("browse:track:library:track-1"), selections)
            assertEquals(Player.STATE_BUFFERING, player.playbackState)
            assertTrue(player.playWhenReady)
            assertFalse(completion.isDone)

            player.updateState(PlaybackSnapshotMapper.fromPlayerState(
                JSONObject("""{"playbackState":"playing","currentQueueItemId":"queue-1","activeMode":"local"}"""),
                JSONObject("""{"items":[{"queue_item_id":"queue-1","name":"Track","media_item":
                    {"media_type":"track","item_id":"track-1","provider":"library","name":"Track"}}]}"""),
            ))
            completion.set(null)
            shadowOf(Looper.getMainLooper()).idle()
            assertTrue(player.isPlaying)
            assertEquals("queue-1", player.currentMediaItem?.mediaId)
            assertEquals("Track", player.mediaMetadata.title)
            assertTrue(requests.isEmpty())
        } finally {
            session.release()
            player.release()
            browser.close()
        }
    }

    @Test
    fun controllersCanBrowseAndControlPlayback() {
        val context = RuntimeEnvironment.getApplication()
        val browser = MediaLibraryBrowser(context) { Futures.immediateVoidFuture() }
        val player = RustSessionPlayer({ Futures.immediateVoidFuture() }, browser::startPlayback, browser::resolveLibraryMediaItem)
        val callback = PlaybackSessionCallback(context.packageName, browser) {
            Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
        }
        val session = MediaLibrarySession.Builder(context, player, callback).build()
        try {
            for (packageName in listOf(context.packageName, "com.android.systemui", "com.google.android.projection.gearhead")) {
                val controller = MediaSession.ControllerInfo.createTestOnlyControllerInfo(
                    packageName, 1, 1000, 0, 0, true, Bundle.EMPTY, true)
                val result = callback.onConnect(session, controller)
                assertTrue(result.isAccepted)
                callback.onPostConnect(session, controller)
                assertTrue("$packageName cannot browse the library root", result.availableSessionCommands.contains(
                    SessionCommand.COMMAND_CODE_LIBRARY_GET_LIBRARY_ROOT))
                val libraryRoot = callback.onGetLibraryRoot(session, controller, null).get()
                assertEquals(LibraryResult.RESULT_SUCCESS, libraryRoot.resultCode)
                assertEquals("root", libraryRoot.value?.mediaId)
                val children = callback.onGetChildren(session, controller, "root", 0, 10, null).get()
                assertEquals(LibraryResult.RESULT_SUCCESS, children.resultCode)
                assertEquals(listOf("node:home", "node:discover", "node:playlists", "node:favorites"),
                    children.value?.map { it.mediaId })
                assertEquals("node:home", callback.onGetItem(session, controller, "node:home").get().value?.mediaId)
                val resolved = callback.onSetMediaItems(session, controller, listOf(children.value!!.first()), 0, 0).get()
                assertEquals("mashin", resolved.mediaItems.first().localConfiguration?.uri?.scheme)
                for (command in listOf(Player.COMMAND_PLAY_PAUSE, Player.COMMAND_SEEK_TO_NEXT,
                    Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_GET_TIMELINE,
                    Player.COMMAND_GET_CURRENT_MEDIA_ITEM, Player.COMMAND_GET_METADATA)) {
                    assertTrue("$packageName missing player command $command", result.availablePlayerCommands.contains(command))
                }
                assertEquals(packageName == context.packageName,
                    result.availableSessionCommands.contains(PlaybackSessionCommands.execute))
                PlaybackSessionActions.commands.forEach { command ->
                    assertTrue("$packageName missing ${command.customAction}", result.availableSessionCommands.contains(command))
                }
                assertFalse(result.availablePlayerCommands == Player.Commands.EMPTY)
                callback.onDisconnected(session, controller)
                shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMinutes(10))
                assertEquals(player, session.player)
                assertTrue(player.availableCommands.contains(Player.COMMAND_PLAY_PAUSE))
            }
        } finally {
            session.release()
            player.release()
            browser.close()
        }
    }

    @Test
    fun trustedControllersCanToggleActionsButCannotExecuteAppOnlyCommands() {
        val context = RuntimeEnvironment.getApplication()
        val browser = MediaLibraryBrowser(context) { Futures.immediateVoidFuture() }
        val player = RustSessionPlayer({ Futures.immediateVoidFuture() }, browser::startPlayback, browser::resolveLibraryMediaItem)
        val snapshot = PlaybackSnapshot.empty().copy(serializedCurrentMediaItem =
            """{"media_type":"track","item_id":"track-1","provider":"library","uri":"library://track/track-1","name":"Track"}""")
        val requests = mutableListOf<PlaybackRequest>()
        val completion = SettableFuture.create<SessionResult>()
        val callback = PlaybackSessionCallback(context.packageName, browser, readSnapshot = { snapshot }) { request ->
            requests.add(request)
            completion
        }
        val session = MediaLibrarySession.Builder(context, player, callback)
            .setMediaButtonPreferences(PlaybackSessionActions.buttons(snapshot)).build()
        try {
            for (packageName in listOf("com.android.systemui", "com.google.android.projection.gearhead")) {
                val controller = MediaSession.ControllerInfo.createTestOnlyControllerInfo(
                    packageName, 1, 1000, 0, 0, true, Bundle.EMPTY, true)
                for (command in PlaybackSessionActions.commands) {
                    val result = callback.onCustomCommand(session, controller, command, Bundle.EMPTY)
                    assertFalse(result.isDone)
                }
                assertEquals(SessionResult.RESULT_ERROR_PERMISSION_DENIED,
                    callback.onCustomCommand(session, controller, PlaybackSessionCommands.execute,
                        PlaybackSessionCommands.encode(PlaybackRequest(PlaybackCommandKind.Shutdown))).get().resultCode)
            }
            assertEquals(listOf(PlaybackCommandKind.SetUserDataFavorites, PlaybackCommandKind.TransportShuffle,
                PlaybackCommandKind.TransportRepeatMode).let { it + it }, requests.map { it.kind })
            val untrusted = MediaSession.ControllerInfo.createTestOnlyControllerInfo(
                "untrusted", 1, 2000, 0, 0, false, Bundle.EMPTY, true)
            assertFalse(callback.onConnect(session, untrusted).availableSessionCommands.contains(PlaybackSessionActions.toggleShuffle))
            assertEquals(SessionResult.RESULT_ERROR_PERMISSION_DENIED,
                callback.onCustomCommand(session, untrusted, PlaybackSessionActions.toggleShuffle, Bundle.EMPTY).get().resultCode)
            assertEquals(3, session.mediaButtonPreferences.size)
            completion.set(SessionResult(SessionResult.RESULT_ERROR_UNKNOWN))
            assertEquals(SessionResult.RESULT_ERROR_UNKNOWN,
                callback.onCustomCommand(session, MediaSession.ControllerInfo.createTestOnlyControllerInfo(
                    "com.android.systemui", 1, 1000, 0, 0, true, Bundle.EMPTY, true),
                    PlaybackSessionActions.toggleShuffle, Bundle.EMPTY).get().resultCode)
        } finally {
            session.release()
            player.release()
            browser.close()
        }
    }

    @Test
    fun searchCallbacksShowAllCategoriesAndCompleteOpenedListsWhenDataArrives() {
        val started = CountDownLatch(4)
        val releaseResults = CountDownLatch(1)
        val releaseAlbums = CountDownLatch(1)
        val requestCount = AtomicInteger()
        val search = MediaLibrarySearch { _, argument ->
            requestCount.incrementAndGet()
            val request = JSONObject(argument!!)
            val mediaType = request.getString("mediaType")
            started.countDown()
            check(started.await(5, TimeUnit.SECONDS))
            check(releaseResults.await(5, TimeUnit.SECONDS))
            if (mediaType == "album") check(releaseAlbums.await(5, TimeUnit.SECONDS))
            if (mediaType == "artist") error("Provider unavailable")
            val category = SearchMediaCategory.entries.first { it.mediaType == mediaType }
            "{\"${category.resultKey}\":[{\"item_id\":\"$mediaType-1\",\"provider\":\"library\",\"name\":\"Result\"}]}"
        }
        val context = RuntimeEnvironment.getApplication()
        val browser = MediaLibraryBrowser(context, search) { Futures.immediateVoidFuture() }
        val player = RustSessionPlayer({ Futures.immediateVoidFuture() }, browser::startPlayback, browser::resolveLibraryMediaItem)
        val callback = PlaybackSessionCallback(context.packageName, browser) {
            Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
        }
        val session = MediaLibrarySession.Builder(context, player, callback).build()
        val controller = MediaSession.ControllerInfo.createTestOnlyControllerInfo(
            "com.google.android.projection.gearhead", 1, 1000, 0, 0, true, Bundle.EMPTY, true)
        try {
            assertEquals(LibraryResult.RESULT_SUCCESS, callback.onSearch(session, controller, "jazz", null)
                .get(1, TimeUnit.SECONDS).resultCode)
            assertTrue(started.await(5, TimeUnit.SECONDS))
            val expectedCategories = listOf("search:tracks?jazz", "search:albums?jazz",
                "search:playlists?jazz", "search:artists?jazz")
            assertEquals(expectedCategories, callback.onGetSearchResult(session, controller, "jazz", 0, 10, null)
                .get(1, TimeUnit.SECONDS).value?.map { it.mediaId })
            val loadingCategories = callback.onGetSearchResult(session, controller, "jazz", 0, 10, null).get().value!!
            assertTrue(loadingCategories.all { it.mediaMetadata.subtitle == null })
            val openedCategories = expectedCategories.associateWith { category ->
                callback.onGetChildren(session, controller, category, 0, 10, null).also { children ->
                    assertFalse(children.isDone)
                }
            }
            releaseResults.countDown()
            search.results("jazz", SearchMediaCategory.Tracks).get(5, TimeUnit.SECONDS)
            search.results("jazz", SearchMediaCategory.Playlists).get(5, TimeUnit.SECONDS)
            search.results("jazz", SearchMediaCategory.Artists).get(5, TimeUnit.SECONDS)
            assertFalse(search.results("jazz", SearchMediaCategory.Albums).isDone)
            val categories = callback.onGetSearchResult(session, controller, "jazz", 0, 10, null).get()
            assertEquals(expectedCategories, categories.value?.map { it.mediaId })
            assertTrue(categories.value!!.all { it.mediaMetadata.subtitle == null })
            assertFalse(openedCategories.getValue("search:albums?jazz").isDone)
            assertEquals("browse:track:library:track-1", openedCategories.getValue("search:tracks?jazz")
                .get(1, TimeUnit.SECONDS).value?.last()?.mediaId)
            assertEquals("browse:playlist:library:playlist-1", openedCategories.getValue("search:playlists?jazz")
                .get(1, TimeUnit.SECONDS).value?.single()?.mediaId)
            assertTrue(openedCategories.getValue("search:artists?jazz").get(1, TimeUnit.SECONDS).value!!.isEmpty())
            val page = callback.onGetSearchResult(session, controller, "jazz", 1, 1, null).get()
            assertEquals("search:albums?jazz", page.value?.single()?.mediaId)
            repeat(2) {
                val tracks = callback.onGetChildren(session, controller, "search:tracks?jazz", 0, 10, null)
                    .get(1, TimeUnit.SECONDS).value!!
                assertEquals(3, tracks.size)
                assertEquals("browse:track:library:track-1", tracks.last().mediaId)
                assertTrue(tracks.last().mediaMetadata.isPlayable == true)
                assertEquals("track", browser.resolveLibraryMediaItem(tracks.last().mediaId)?.getString("media_type"))
            }
            assertEquals(LibraryResult.RESULT_ERROR_BAD_VALUE,
                callback.onGetSearchResult(session, controller, "jazz", -1, 10, null).get().resultCode)
            assertEquals(4, requestCount.get())
            releaseAlbums.countDown()
            search.results("jazz", SearchMediaCategory.Albums).get(5, TimeUnit.SECONDS)
            assertEquals("browse:album:library:album-1",
                openedCategories.getValue("search:albums?jazz").get(1, TimeUnit.SECONDS).value?.single()?.mediaId)
            val completedCategories = callback.onGetSearchResult(session, controller, "jazz", 0, 10, null).get().value!!
            assertEquals(expectedCategories, completedCategories.map { it.mediaId })
            assertTrue(completedCategories.all { it.mediaMetadata.subtitle == null })
            assertEquals(4, requestCount.get())
            assertTrue(callback.onGetSearchResult(session, controller, "  ", 0, 10, null).get().value!!.isEmpty())
        } finally {
            releaseResults.countDown()
            releaseAlbums.countDown()
            browser.close()
            session.release()
            player.release()
        }
    }
}