package com.example.mashin_flutter.media

import org.junit.Assert.assertEquals
import org.junit.Test

class PlaybackSessionLifecycleTest {
    private val lifecycle = PlaybackSessionLifecycle()
    private var shutdownRequests = 0

    init {
        lifecycle.attachSession { shutdownRequests++ }
    }

    // Activity closure and recreation.
    @Test
    fun closingActivityEndsSessionEvenWithAndroidAutoConnected() {
        val activity = Any()
        lifecycle.attachActivity(activity)
        lifecycle.attachAndroidAutoController(Any())
        lifecycle.detachActivity(activity, false)
        assertEquals(1, shutdownRequests)
    }

    @Test
    fun removingActivityTaskEndsSession() {
        lifecycle.attachActivity(Any())
        lifecycle.removeActivityTask()
        assertEquals(1, shutdownRequests)
    }

    @Test
    fun recreationDoesNotEndSessionWhenAndroidAutoDisconnects() {
        val activity = Any()
        val controller = Any()
        lifecycle.attachActivity(activity)
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachActivity(activity, true)
        lifecycle.detachAndroidAutoController(controller)
        assertEquals(0, shutdownRequests)
        val replacement = Any()
        lifecycle.attachActivity(replacement)
        lifecycle.detachActivity(replacement, false)
        assertEquals(1, shutdownRequests)
    }

    // Android Auto disconnect and remaining UI ownership.
    @Test
    fun androidAutoDisconnectWithoutActivityEndsSession() {
        val controller = Any()
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachAndroidAutoController(controller)
        assertEquals(1, shutdownRequests)
    }

    @Test
    fun androidAutoDisconnectKeepsSessionForMinimizedActivity() {
        lifecycle.attachActivity(Any())
        val controller = Any()
        lifecycle.attachAndroidAutoController(controller)
        lifecycle.detachAndroidAutoController(controller)
        assertEquals(0, shutdownRequests)
    }

    @Test
    fun androidAutoDisconnectWaitsForLastController() {
        val first = Any()
        val second = Any()
        lifecycle.attachAndroidAutoController(first)
        lifecycle.attachAndroidAutoController(second)
        lifecycle.detachAndroidAutoController(first)
        assertEquals(0, shutdownRequests)
        lifecycle.detachAndroidAutoController(second)
        assertEquals(1, shutdownRequests)
    }

    @Test
    fun unrelatedControllerDisconnectDoesNotEndSession() {
        lifecycle.detachAndroidAutoController(Any())
        assertEquals(0, shutdownRequests)
    }

    @Test
    fun detachedSessionDoesNotReceiveShutdownRequests() {
        lifecycle.detachSession()
        lifecycle.removeActivityTask()
        assertEquals(0, shutdownRequests)
    }
}