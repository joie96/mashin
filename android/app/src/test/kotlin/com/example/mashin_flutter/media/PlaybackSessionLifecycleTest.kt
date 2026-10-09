package com.example.mashin_flutter.media

import android.os.Looper
import java.time.Duration
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
@LooperMode(LooperMode.Mode.PAUSED)
class PlaybackSessionLifecycleTest {
    private val lifecycle = PlaybackSessionLifecycle()
    private var shutdownRequests = 0

    init {
        lifecycle.attachSession { shutdownRequests++ }
    }

    // Activity closure and recreation.
    @Test
    fun closingActivityKeepsSessionWithAndroidAutoConnected() {
        val activity = Any()
        lifecycle.attachActivity(activity)
        lifecycle.attachAndroidAutoController(Any())
        lifecycle.detachActivity(activity, false)
        advanceTime(5_000)
        assertEquals(0, shutdownRequests)
    }

    @Test
    fun closingLastActivityWithoutAndroidAutoEndsSession() {
        val first = Any()
        val second = Any()
        lifecycle.attachActivity(first)
        lifecycle.attachActivity(second)
        lifecycle.detachActivity(first, false)
        assertEquals(0, shutdownRequests)
        lifecycle.detachActivity(second, false)
        assertEquals(1, shutdownRequests)
    }

    @Test
    fun removingActivityTaskEndsSession() {
        lifecycle.attachActivity(Any())
        lifecycle.removeActivityTask()
        assertEquals(1, shutdownRequests)
    }

    @Test
    fun removingActivityTaskKeepsSessionWithAndroidAutoConnected() {
        lifecycle.attachActivity(Any())
        lifecycle.attachAndroidAutoController(Any())
        lifecycle.removeActivityTask()
        advanceTime(5_000)
        assertEquals(0, shutdownRequests)
    }

    @Test
    fun recreationDoesNotEndSessionWhenAndroidAutoDisconnects() {
        val activity = Any()
        val controller = Any()
        lifecycle.attachActivity(activity)
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachActivity(activity, true)
        lifecycle.detachAndroidAutoController(controller)
        advanceTime(5_000)
        assertEquals(0, shutdownRequests)
        val replacement = Any()
        lifecycle.attachActivity(replacement)
        lifecycle.detachActivity(replacement, false)
        assertEquals(1, shutdownRequests)
    }

    // Android Auto disconnect and remaining UI ownership.
    @Test
    fun androidAutoDisconnectWithoutActivityEndsSessionAfterFiveSeconds() {
        val controller = Any()
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachAndroidAutoController(controller)
        advanceTime(4_999)
        assertEquals(0, shutdownRequests)
        advanceTime(1)
        assertEquals(1, shutdownRequests)
        advanceTime(5_000)
        assertEquals(1, shutdownRequests)
    }

    @Test
    fun androidAutoDisconnectKeepsSessionForMinimizedActivity() {
        lifecycle.attachActivity(Any())
        val controller = Any()
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachAndroidAutoController(controller)
        advanceTime(5_000)
        assertEquals(0, shutdownRequests)
    }

    @Test
    fun androidAutoDisconnectWaitsForLastController() {
        val first = Any()
        val second = Any()
        lifecycle.attachAndroidAutoController(first)
        lifecycle.attachAndroidAutoController(second)
        lifecycle.detachAndroidAutoController(first)
        advanceTime(5_000)
        assertEquals(0, shutdownRequests)
        lifecycle.detachAndroidAutoController(second)
        assertEquals(0, shutdownRequests)
        advanceTime(5_000)
        assertEquals(1, shutdownRequests)
    }

    @Test
    fun androidAutoReconnectCancelsPendingShutdown() {
        val controller = Any()
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachAndroidAutoController(controller)
        advanceTime(4_999)
        lifecycle.attachAndroidAutoController(Any())
        advanceTime(5_000)
        assertEquals(0, shutdownRequests)
    }

    @Test
    fun repeatedDisconnectDoesNotExtendGrace() {
        val controller = Any()
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachAndroidAutoController(controller)
        advanceTime(4_000)
        lifecycle.detachAndroidAutoController(controller)
        advanceTime(1_000)
        assertEquals(1, shutdownRequests)
    }

    @Test
    fun anotherDisconnectAfterReconnectGetsFullGrace() {
        val first = Any()
        val second = Any()
        lifecycle.attachAndroidAutoController(first)
        lifecycle.detachAndroidAutoController(first)
        advanceTime(4_000)
        lifecycle.attachAndroidAutoController(second)
        lifecycle.detachAndroidAutoController(second)
        advanceTime(1_000)
        assertEquals(0, shutdownRequests)
        advanceTime(4_000)
        assertEquals(1, shutdownRequests)
    }

    @Test
    fun attachingActivityCancelsPendingShutdown() {
        val controller = Any()
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachAndroidAutoController(controller)
        advanceTime(4_000)
        lifecycle.attachActivity(Any())
        advanceTime(5_000)
        assertEquals(0, shutdownRequests)
    }

    @Test
    fun closingActivityDuringGraceWaitsForOriginalDeadline() {
        val activity = Any()
        val controller = Any()
        lifecycle.attachActivity(activity)
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachAndroidAutoController(controller)
        advanceTime(4_000)
        lifecycle.detachActivity(activity, false)
        assertEquals(0, shutdownRequests)
        advanceTime(1_000)
        assertEquals(1, shutdownRequests)
    }

    @Test
    fun removingActivityTaskDuringGraceWaitsForOriginalDeadline() {
        val controller = Any()
        lifecycle.attachActivity(Any())
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachAndroidAutoController(controller)
        advanceTime(4_000)
        lifecycle.removeActivityTask()
        assertEquals(0, shutdownRequests)
        advanceTime(1_000)
        assertEquals(1, shutdownRequests)
    }

    @Test
    fun closingActivityAfterGraceEndsUnownedSession() {
        val activity = Any()
        val controller = Any()
        lifecycle.attachActivity(activity)
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachAndroidAutoController(controller)
        advanceTime(5_000)
        assertEquals(0, shutdownRequests)
        lifecycle.detachActivity(activity, false)
        assertEquals(1, shutdownRequests)
    }

    @Test
    fun unrelatedControllerDisconnectDoesNotEndSession() {
        lifecycle.detachAndroidAutoController(Any())
        advanceTime(5_000)
        assertEquals(0, shutdownRequests)
    }

    @Test
    fun detachedSessionDoesNotReceiveShutdownRequests() {
        val controller = Any()
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachAndroidAutoController(controller)
        lifecycle.detachSession()
        lifecycle.removeActivityTask()
        advanceTime(5_000)
        assertEquals(0, shutdownRequests)
    }

    @Test
    fun oldDisconnectDoesNotShutdownReplacementSession() {
        val controller = Any()
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachAndroidAutoController(controller)
        advanceTime(4_000)
        lifecycle.detachSession()
        var replacementShutdownRequests = 0
        lifecycle.attachSession { replacementShutdownRequests++ }
        advanceTime(5_000)
        assertEquals(0, shutdownRequests)
        assertEquals(0, replacementShutdownRequests)
    }

    // Advance the main looper deterministically without waiting on wall-clock time.
    private fun advanceTime(milliseconds: Long) {
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(milliseconds))
    }
}