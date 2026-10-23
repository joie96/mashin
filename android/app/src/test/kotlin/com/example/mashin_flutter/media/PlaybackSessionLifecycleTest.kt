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
    private var serviceStopRequests = 0

    init {
        lifecycle.attachSession { serviceStopRequests++ }
    }

    // Activity closure relinquishes started ownership, never bound session ownership.
    @Test
    fun closingLastIdleActivityRequestsServiceStop() {
        val first = Any()
        val second = Any()
        lifecycle.attachActivity(first)
        lifecycle.attachActivity(second)
        lifecycle.detachActivity(first, false)
        assertEquals(0, serviceStopRequests)
        lifecycle.detachActivity(second, false)
        assertEquals(1, serviceStopRequests)
    }

    @Test
    fun removingIdleActivityTaskRequestsServiceStop() {
        lifecycle.attachActivity(Any())
        lifecycle.removeActivityTask()
        assertEquals(1, serviceStopRequests)
    }

    @Test
    fun activityRecreationKeepsServiceUntilReplacementCloses() {
        val activity = Any()
        lifecycle.attachActivity(activity)
        lifecycle.detachActivity(activity, true)
        lifecycle.updateForegroundState(isForegroundRequired = false)
        advanceTime(600_000)
        assertEquals(0, serviceStopRequests)
        val replacement = Any()
        lifecycle.attachActivity(replacement)
        lifecycle.detachActivity(replacement, false)
        assertEquals(1, serviceStopRequests)
    }

    @Test
    fun foregroundDemotionKeepsServiceForMinimizedActivity() {
        lifecycle.attachActivity(Any())
        lifecycle.updateForegroundState(isForegroundRequired = true)
        lifecycle.updateForegroundState(isForegroundRequired = false)
        assertEquals(0, serviceStopRequests)
    }

    @Test
    fun unrelatedActivityDetachDoesNotRequestServiceStop() {
        lifecycle.detachActivity(Any(), false)
        assertEquals(0, serviceStopRequests)
    }

    // Pauses follow Media3 foreground decisions rather than legacy controller inactivity.
    @Test
    fun pauseAfterLegacyInactivityKeepsSessionUntilMedia3DemotesService() {
        lifecycle.updatePlaybackState(hasPlaybackIntent = true, isPlaybackStarting = false)
        lifecycle.updateForegroundState(isForegroundRequired = true)
        advanceTime(305_000)
        lifecycle.updatePlaybackState(hasPlaybackIntent = false, isPlaybackStarting = false)
        advanceTime(599_999)
        assertEquals(0, serviceStopRequests)
        lifecycle.updateForegroundState(isForegroundRequired = false)
        assertEquals(1, serviceStopRequests)
    }

    @Test
    fun resumingPausedPlaybackPreventsStopAfterForegroundDemotion() {
        lifecycle.updateForegroundState(isForegroundRequired = true)
        advanceTime(599_999)
        lifecycle.updatePlaybackState(hasPlaybackIntent = true, isPlaybackStarting = false)
        lifecycle.updateForegroundState(isForegroundRequired = false)
        assertEquals(0, serviceStopRequests)
    }

    @Test
    fun closingActivityDuringPausedForegroundGraceKeepsService() {
        val activity = Any()
        lifecycle.attachActivity(activity)
        lifecycle.updateForegroundState(isForegroundRequired = true)
        lifecycle.detachActivity(activity, false)
        advanceTime(600_000)
        assertEquals(0, serviceStopRequests)
        lifecycle.updateForegroundState(isForegroundRequired = false)
        assertEquals(1, serviceStopRequests)
    }

    @Test
    fun removingActivityTaskDuringPausedForegroundGraceKeepsService() {
        lifecycle.attachActivity(Any())
        lifecycle.updateForegroundState(isForegroundRequired = true)
        lifecycle.removeActivityTask()
        assertEquals(0, serviceStopRequests)
    }

    @Test
    fun repeatedIdleUpdatesRequestServiceStopOnlyOnce() {
        lifecycle.updateForegroundState(isForegroundRequired = false)
        lifecycle.updateForegroundState(isForegroundRequired = false)
        lifecycle.removeActivityTask()
        assertEquals(1, serviceStopRequests)
    }

    @Test
    fun newPlaybackAllowsAnotherStopRequestAfterDemotion() {
        lifecycle.updateForegroundState(isForegroundRequired = false)
        lifecycle.updateForegroundState(isForegroundRequired = true)
        lifecycle.updateForegroundState(isForegroundRequired = false)
        assertEquals(2, serviceStopRequests)
    }

    // Queued starts remain protected before the native snapshot and notification are ready.
    @Test
    fun pendingStartKeepsServiceAfterForegroundDemotion() {
        lifecycle.updatePlaybackState(hasPlaybackIntent = false, isPlaybackStarting = true)
        lifecycle.updateForegroundState(isForegroundRequired = false)
        assertEquals(0, serviceStopRequests)
        lifecycle.updatePlaybackState(hasPlaybackIntent = true, isPlaybackStarting = false)
        assertEquals(0, serviceStopRequests)
    }

    @Test
    fun failedStartRequestsServiceStopWhenMedia3NoLongerNeedsForeground() {
        lifecycle.updatePlaybackState(hasPlaybackIntent = false, isPlaybackStarting = true)
        lifecycle.updateForegroundState(isForegroundRequired = true)
        lifecycle.updatePlaybackState(hasPlaybackIntent = false, isPlaybackStarting = false)
        assertEquals(0, serviceStopRequests)
        lifecycle.updateForegroundState(isForegroundRequired = false)
        assertEquals(1, serviceStopRequests)
    }

    @Test
    fun closingActivityDuringPlaybackKeepsService() {
        val activity = Any()
        lifecycle.attachActivity(activity)
        lifecycle.updatePlaybackState(hasPlaybackIntent = true, isPlaybackStarting = false)
        lifecycle.detachActivity(activity, false)
        assertEquals(0, serviceStopRequests)
    }

    @Test
    fun removingActivityTaskDuringPendingStartKeepsService() {
        lifecycle.attachActivity(Any())
        lifecycle.updatePlaybackState(hasPlaybackIntent = false, isPlaybackStarting = true)
        lifecycle.removeActivityTask()
        assertEquals(0, serviceStopRequests)
    }

    // Session replacement must not inherit foreground or playback ownership.
    @Test
    fun initialIdleSnapshotDoesNotEndNewSession() {
        lifecycle.updatePlaybackState(hasPlaybackIntent = false, isPlaybackStarting = false)
        assertEquals(0, serviceStopRequests)
    }

    @Test
    fun detachedSessionDoesNotReceiveServiceStopRequests() {
        lifecycle.detachSession()
        lifecycle.updateForegroundState(isForegroundRequired = false)
        lifecycle.removeActivityTask()
        assertEquals(0, serviceStopRequests)
    }

    @Test
    fun replacementSessionDoesNotRetainOldOwnership() {
        lifecycle.updateForegroundState(isForegroundRequired = true)
        lifecycle.updatePlaybackState(hasPlaybackIntent = true, isPlaybackStarting = false)
        lifecycle.detachSession()
        var replacementStopRequests = 0
        lifecycle.attachSession { replacementStopRequests++ }
        lifecycle.updateForegroundState(isForegroundRequired = false)
        assertEquals(0, serviceStopRequests)
        assertEquals(1, replacementStopRequests)
    }

    // No application timer should end a session while Media3 still requires foreground.
    private fun advanceTime(milliseconds: Long) {
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(milliseconds))
    }
}