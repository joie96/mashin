package com.joie96.mashin.media

/** Requests service stops on the main thread; Android bindings retain ownership until onDestroy. */
internal class PlaybackSessionLifecycle {
    companion object {
        val shared = PlaybackSessionLifecycle()
    }

    private val activities = mutableSetOf<Any>()
    private var isActivityRecreating = false
    private var shouldKeepSessionForPlayback = false
    private var isForegroundRequired = false
    private var hasRequestedServiceStop = false
    private var requestServiceStop: (() -> Unit)? = null

    // Session ownership and activity lifecycle.
    fun attachSession(requestServiceStop: () -> Unit) {
        this.requestServiceStop = requestServiceStop
        hasRequestedServiceStop = false
    }

    fun detachSession() {
        requestServiceStop = null
        shouldKeepSessionForPlayback = false
        isForegroundRequired = false
        hasRequestedServiceStop = false
    }

    fun attachActivity(activity: Any) {
        activities.add(activity)
        isActivityRecreating = false
        hasRequestedServiceStop = false
    }

    fun detachActivity(activity: Any, isChangingConfigurations: Boolean) {
        if (!activities.remove(activity)) return
        isActivityRecreating = isChangingConfigurations
        stopServiceIfIdle()
    }

    fun removeActivityTask() {
        activities.clear()
        isActivityRecreating = false
        stopServiceIfIdle()
    }

    // Media3 owns the paused foreground grace; queued starts also prevent service stops.
    fun updatePlaybackState(hasPlaybackIntent: Boolean, isPlaybackStarting: Boolean) {
        val shouldKeepSession = hasPlaybackIntent || isPlaybackStarting
        if (shouldKeepSessionForPlayback == shouldKeepSession) return
        shouldKeepSessionForPlayback = shouldKeepSession
        stopServiceIfIdle()
    }

    fun updateForegroundState(isForegroundRequired: Boolean) {
        this.isForegroundRequired = isForegroundRequired
        stopServiceIfIdle()
    }

    // stopSelf relinquishes started ownership without releasing sessions held by bound browsers.
    private fun stopServiceIfIdle() {
        if (activities.isNotEmpty() || isActivityRecreating || shouldKeepSessionForPlayback || isForegroundRequired) {
            hasRequestedServiceStop = false
            return
        }
        val requestStop = requestServiceStop ?: return
        if (hasRequestedServiceStop) return
        hasRequestedServiceStop = true
        requestStop()
    }
}