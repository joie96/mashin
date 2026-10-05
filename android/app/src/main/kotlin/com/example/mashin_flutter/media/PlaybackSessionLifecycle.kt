package com.example.mashin_flutter.media

/** Tracks UI ownership on the main thread; a minimized activity still owns the session. */
internal class PlaybackSessionLifecycle {
    companion object {
        val shared = PlaybackSessionLifecycle()
    }

    private val activities = mutableSetOf<Any>()
    private val androidAutoControllers = mutableSetOf<Any>()
    private var isActivityRecreating = false
    private var shutdownSession: (() -> Unit)? = null

    // Session ownership and activity lifecycle.
    fun attachSession(shutdownSession: () -> Unit) {
        this.shutdownSession = shutdownSession
    }

    fun detachSession() {
        shutdownSession = null
        androidAutoControllers.clear()
    }

    fun attachActivity(activity: Any) {
        activities.add(activity)
        isActivityRecreating = false
    }

    fun detachActivity(activity: Any, isChangingConfigurations: Boolean) {
        if (!activities.remove(activity)) return
        isActivityRecreating = isChangingConfigurations
        if (activities.isEmpty() && !isActivityRecreating) shutdownSession?.invoke()
    }

    fun removeActivityTask() {
        activities.clear()
        isActivityRecreating = false
        shutdownSession?.invoke()
    }

    // Android Auto connection ownership.
    fun attachAndroidAutoController(controller: Any) {
        androidAutoControllers.add(controller)
    }

    fun detachAndroidAutoController(controller: Any) {
        if (!androidAutoControllers.remove(controller)) return
        if (androidAutoControllers.isEmpty() && activities.isEmpty() && !isActivityRecreating) {
            shutdownSession?.invoke()
        }
    }
}