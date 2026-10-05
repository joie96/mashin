package com.example.mashin_flutter.media.notification

import android.content.Intent
import androidx.media3.session.MediaLibraryService.MediaLibrarySession
import androidx.media3.session.SessionResult
import com.example.mashin_flutter.MainActivity
import com.example.mashin_flutter.media.library.MediaLibraryBrowser
import com.example.mashin_flutter.media.session.PlaybackSessionCallback
import com.example.mashin_flutter.media.session.RustSessionPlayer
import com.google.common.util.concurrent.Futures
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PlaybackNotificationProviderTest {
    // Notification navigation and activity reuse.
    @Test
    fun startupNotificationOpensSessionActivity() {
        val context = RuntimeEnvironment.getApplication()
        val provider = PlaybackNotificationProvider(context)
        val browser = MediaLibraryBrowser(context) { Futures.immediateVoidFuture() }
        val player = RustSessionPlayer({ Futures.immediateVoidFuture() }, browser::startPlayback, browser::queueEntry)
        val callback = PlaybackSessionCallback(context.packageName, browser) {
            Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
        }
        val session = MediaLibrarySession.Builder(context, player, callback)
            .setSessionActivity(provider.sessionActivity)
            .build()
        try {
            val notification = provider.createStartupNotification(session)
            assertEquals(provider.sessionActivity, session.sessionActivity)
            assertEquals(session.sessionActivity, notification.contentIntent)
            assertTrue(notification.contentIntent.isImmutable)

            notification.contentIntent.send()
            val startedIntent = shadowOf(context).nextStartedActivity
            assertEquals(MainActivity::class.java.name, startedIntent.component?.className)
            assertEquals(Intent.ACTION_MAIN, startedIntent.action)
            assertTrue(startedIntent.hasCategory(Intent.CATEGORY_LAUNCHER))
            val activityFlags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
            assertEquals(activityFlags, startedIntent.flags and activityFlags)
        } finally {
            session.release()
            player.release()
            browser.close()
        }
    }
}