package com.example.mashin_flutter.media.session

import android.os.Bundle
import androidx.media3.common.Player
import androidx.media3.session.MediaLibraryService.MediaLibrarySession
import androidx.media3.session.MediaSession
import androidx.media3.session.SessionResult
import com.example.mashin_flutter.media.library.MediaLibraryBrowser
import com.google.common.util.concurrent.Futures
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PlaybackSessionCallbackTest {
    @Test
    fun appAndNotificationControllersReceivePlaybackAndMetadataCommands() {
        val context = RuntimeEnvironment.getApplication()
        val browser = MediaLibraryBrowser(context) { Futures.immediateVoidFuture() }
        val player = RustSessionPlayer({ Futures.immediateVoidFuture() }, browser::startPlayback, browser::queueEntry)
        val callback = PlaybackSessionCallback(context.packageName, browser) {
            Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
        }
        val session = MediaLibrarySession.Builder(context, player, callback).build()
        try {
            for (packageName in listOf(context.packageName, "com.android.systemui")) {
                val controller = MediaSession.ControllerInfo.createTestOnlyControllerInfo(
                    packageName, 1, 1000, 0, 0, true, Bundle.EMPTY, true)
                val result = callback.onConnectAsync(session, controller).get()
                assertTrue(result.isAccepted)
                for (command in listOf(Player.COMMAND_PLAY_PAUSE, Player.COMMAND_SEEK_TO_NEXT,
                    Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_GET_TIMELINE,
                    Player.COMMAND_GET_CURRENT_MEDIA_ITEM, Player.COMMAND_GET_METADATA)) {
                    assertTrue("$packageName missing player command $command", result.availablePlayerCommands.contains(command))
                }
                assertEquals(packageName == context.packageName,
                    result.availableSessionCommands.contains(PlaybackSessionCommands.execute))
                assertFalse(result.availablePlayerCommands == Player.Commands.EMPTY)
            }
        } finally {
            session.release()
            player.release()
            browser.close()
        }
    }
}