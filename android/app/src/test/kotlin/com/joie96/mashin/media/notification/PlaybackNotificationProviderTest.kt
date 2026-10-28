package com.joie96.mashin.media.notification

import android.content.Intent
import android.app.PendingIntent
import android.os.Bundle
import androidx.core.app.NotificationCompat
import androidx.core.graphics.drawable.IconCompat
import androidx.media3.session.CommandButton
import androidx.media3.session.MediaNotification
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaLibraryService.MediaLibrarySession
import androidx.media3.session.SessionResult
import com.joie96.mashin.MainActivity
import com.joie96.mashin.media.library.MediaLibraryBrowser
import com.joie96.mashin.media.session.PlaybackSessionCallback
import com.joie96.mashin.media.session.PlaybackSessionActions
import com.joie96.mashin.media.session.PlaybackSnapshot
import com.joie96.mashin.media.session.RustSessionPlayer
import com.google.common.util.concurrent.Futures
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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
        val player = RustSessionPlayer({ Futures.immediateVoidFuture() }, browser::startPlayback, browser::resolveLibraryMediaItem)
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

    @Test
    fun notificationCreatesAndForwardsAllThreeSessionActions() {
        val context = RuntimeEnvironment.getApplication()
        val provider = PlaybackNotificationProvider(context)
        val browser = MediaLibraryBrowser(context) { Futures.immediateVoidFuture() }
        val player = RustSessionPlayer({ Futures.immediateVoidFuture() }, browser::startPlayback, browser::resolveLibraryMediaItem)
        val callback = PlaybackSessionCallback(context.packageName, browser) {
            Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
        }
        val session = MediaLibrarySession.Builder(context, player, callback).build()
        val actions = mutableListOf<String>()
        val factory = object : MediaNotification.ActionFactory {
            override fun createMediaAction(mediaSession: MediaSession, icon: IconCompat,
                title: CharSequence, command: Int): NotificationCompat.Action =
                NotificationCompat.Action.Builder(icon, title, provider.sessionActivity).build()

            override fun createCustomAction(mediaSession: MediaSession, icon: IconCompat,
                title: CharSequence, customAction: String, extras: Bundle): NotificationCompat.Action {
                actions.add(customAction)
                return NotificationCompat.Action.Builder(icon, title, provider.sessionActivity).build()
            }

            override fun createCustomActionFromCustomCommandButton(mediaSession: MediaSession,
                customCommandButton: CommandButton): NotificationCompat.Action = createCustomAction(mediaSession,
                IconCompat.createWithResource(context, customCommandButton.iconResId), customCommandButton.displayName,
                customCommandButton.sessionCommand!!.customAction, Bundle.EMPTY)

            override fun createMediaActionPendingIntent(mediaSession: MediaSession, command: Int): PendingIntent =
                provider.sessionActivity
        }
        try {
            for (isEnabled in listOf(false, true)) {
                val snapshot = PlaybackSnapshot.empty().copy(serializedCurrentMediaItem = "{}",
                    isCurrentItemFavorite = isEnabled, isShuffleEnabled = isEnabled)
                session.setMediaButtonPreferences(PlaybackSessionActions.buttons(snapshot))
                actions.clear()
                val notification = provider.createNotification(session, session.mediaButtonPreferences, factory) {}.notification
                assertEquals(PlaybackSessionActions.commands.map { it.customAction }, actions)
                assertEquals(PlaybackSessionActions.buttons(snapshot).map { it.displayName.toString() },
                    notification.actions.map { it.title.toString() }.filter { title ->
                        PlaybackSessionActions.buttons(snapshot).any { it.displayName.toString() == title }
                    })
                actions.forEach { assertFalse(provider.handleCustomCommand(session, it, Bundle.EMPTY)) }
            }
        } finally {
            session.release()
            player.release()
            browser.close()
        }
    }
}