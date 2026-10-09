package com.example.mashin_flutter.media

import android.os.Handler
import android.os.Looper

/** Tracks activity and Android Auto ownership on the main thread, including reconnect grace. */
internal class PlaybackSessionLifecycle {
    companion object {
        private const val ANDROID_AUTO_RECONNECT_GRACE_MS = 5_000L
        val shared = PlaybackSessionLifecycle()
    }

    private val mainHandler = Handler(Looper.getMainLooper())
    private val activities = mutableSetOf<Any>()
    private val androidAutoControllers = mutableSetOf<Any>()
    private var isActivityRecreating = false
    private var shutdownSession: (() -> Unit)? = null
    private var pendingShutdown: Runnable? = null
    private var shutdownGeneration = 0L

    // Session ownership and activity lifecycle.
    fun attachSession(shutdownSession: () -> Unit) {
        cancelPendingShutdown()
        this.shutdownSession = shutdownSession
    }

    fun detachSession() {
        cancelPendingShutdown()
        shutdownSession = null
        androidAutoControllers.clear()
    }

    fun attachActivity(activity: Any) {
        cancelPendingShutdown()
        activities.add(activity)
        isActivityRecreating = false
    }

    fun detachActivity(activity: Any, isChangingConfigurations: Boolean) {
        if (!activities.remove(activity)) return
        isActivityRecreating = isChangingConfigurations
        shutdownIfUnowned()
    }

    fun removeActivityTask() {
        activities.clear()
        isActivityRecreating = false
        shutdownIfUnowned()
    }

    // Android Auto connection ownership.
    fun attachAndroidAutoController(controller: Any) {
        cancelPendingShutdown()
        androidAutoControllers.add(controller)
    }

    fun detachAndroidAutoController(controller: Any) {
        if (!androidAutoControllers.remove(controller)) return
        if (androidAutoControllers.isNotEmpty() || shutdownSession == null) return
        cancelPendingShutdown()
        val generation = shutdownGeneration
        val shutdown = Runnable {
            if (generation != shutdownGeneration) return@Runnable
            pendingShutdown = null
            shutdownIfUnowned()
        }
        pendingShutdown = shutdown
        mainHandler.postDelayed(shutdown, ANDROID_AUTO_RECONNECT_GRACE_MS)
    }

    // Pending shutdowns must not outlive their session or a newly attached owner.
    private fun cancelPendingShutdown() {
        shutdownGeneration++
        pendingShutdown?.let(mainHandler::removeCallbacks)
        pendingShutdown = null
    }

    private fun shutdownIfUnowned() {
        if (activities.isEmpty() && androidAutoControllers.isEmpty() &&
            !isActivityRecreating && pendingShutdown == null) {
            shutdownSession?.invoke()
        }
    }
}