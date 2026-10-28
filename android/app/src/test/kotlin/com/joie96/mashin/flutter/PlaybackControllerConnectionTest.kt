package com.joie96.mashin.flutter

import android.os.Looper
import androidx.media3.session.MediaController
import com.google.common.util.concurrent.SettableFuture
import java.time.Duration
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeoutException
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PlaybackControllerConnectionTest {
    @Test
    fun concurrentRequestsShareAConnectionAndTimeoutReleasesIt() {
        val pending = SettableFuture.create<MediaController>()
        var connectionCount = 0
        var releaseCount = 0
        val connection = PlaybackControllerConnection(RuntimeEnvironment.getApplication(),
            createController = { connectionCount++; pending },
            releaseController = { releaseCount++ })
        val first = connection.connect()
        assertSame(first, connection.connect())
        assertEquals(1, connectionCount)
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofSeconds(10))
        val failure = assertThrows(ExecutionException::class.java) { first.get() }
        assertTrue(failure.cause is TimeoutException)
        assertEquals(1, releaseCount)
    }

    @Test
    fun oldConnectionFailureCannotDisconnectItsReplacement() {
        val firstPending = SettableFuture.create<MediaController>()
        val secondPending = SettableFuture.create<MediaController>()
        var connectionCount = 0
        var releaseCount = 0
        val connection = PlaybackControllerConnection(RuntimeEnvironment.getApplication(),
            createController = { if (++connectionCount == 1) firstPending else secondPending },
            releaseController = { releaseCount++ })
        connection.connect()
        connection.disconnect()
        val replacement = connection.connect()
        firstPending.setException(IllegalStateException("old connection failed"))
        shadowOf(Looper.getMainLooper()).idle()
        assertSame(replacement, connection.connect())
        assertEquals(1, releaseCount)
        connection.disconnect()
        assertEquals(2, releaseCount)
    }
}