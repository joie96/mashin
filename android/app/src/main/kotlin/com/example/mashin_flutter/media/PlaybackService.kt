package com.example.mashin_flutter.media

import android.app.NotificationManager
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.util.Log
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.DefaultMediaNotificationProvider
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession
import androidx.media3.session.SessionResult
import com.example.mashin_flutter.core.RustCoreBridge
import com.example.mashin_flutter.media.library.MediaLibraryBrowser
import com.example.mashin_flutter.media.notification.PlaybackNotificationProvider
import com.example.mashin_flutter.media.session.PlaybackCommandKind
import com.example.mashin_flutter.media.session.PlaybackRequest
import com.example.mashin_flutter.media.session.PlaybackSessionCallback
import com.example.mashin_flutter.media.session.PlaybackSessionActions
import com.example.mashin_flutter.media.session.PlaybackSessionCommands
import com.example.mashin_flutter.media.session.PlaybackSnapshot
import com.example.mashin_flutter.media.session.PlaybackSnapshotMapper
import com.example.mashin_flutter.media.session.RustSessionPlayer
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.MoreExecutors
import com.google.common.util.concurrent.SettableFuture
import java.util.concurrent.Executors
import org.json.JSONObject

/** Owns Android playback independently of Flutter. Rust remains the sole queue/state owner. */
@UnstableApi
class PlaybackService : MediaLibraryService() {
    companion object {
        private const val TAG = "MashinPlaybackService"
        // Process-scoped serialization prevents an old service shutdown racing a new restore.
        private val coreExecutor = Executors.newSingleThreadExecutor()
    }

    private val mainHandler = Handler(Looper.getMainLooper())
    private var sessionPlayer: RustSessionPlayer? = null
    private var mediaLibrarySession: MediaLibrarySession? = null
    private var browser: MediaLibraryBrowser? = null
    private lateinit var notificationProvider: PlaybackNotificationProvider
    private lateinit var audioFocus: PlaybackAudioFocus
    private var snapshot = PlaybackSnapshot.empty()
    @Volatile private var isDestroyed = false
    private var hasRestoredSession = false
    private var pendingPlaybackStarts = 0
    private var isSnapshotRefreshQueued = false
    private var isCoreReady = false
    private var isProcessingCommand = false
    private var initializationError: String? = null
    private data class PendingCommand(val request: PlaybackRequest, val completion: SettableFuture<SessionResult>)
    private val pendingCommands = ArrayDeque<PendingCommand>()
    private val wakeLock by lazy {
        getSystemService(PowerManager::class.java).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "mashin:playback")
            .apply { setReferenceCounted(false) }
    }

    // Service lifecycle and native session ownership.
    override fun onCreate() {
        super.onCreate()
        Log.d(TAG, "Creating playback service")
        notificationProvider = PlaybackNotificationProvider(this)
        setMediaNotificationProvider(notificationProvider)
        audioFocus = PlaybackAudioFocus(this) {
            dispatchCoreCommand(PlaybackRequest(PlaybackCommandKind.TransportPause))
        }
        RustCoreBridge.initializeCore(applicationContext)
        val initializedBrowser = MediaLibraryBrowser(applicationContext, dispatchCommand = ::dispatchPlayerCommand)
        browser = initializedBrowser
        val player = RustSessionPlayer(::dispatchPlayerCommand, initializedBrowser::startPlayback, initializedBrowser::queueEntry)
        sessionPlayer = player
        val callback = PlaybackSessionCallback(packageName, initializedBrowser, readSnapshot = { snapshot }, dispatchCommand = ::dispatchCoreCommand)
        val session = MediaLibrarySession.Builder(this, player, callback)
            .setSessionActivity(notificationProvider.sessionActivity)
            .setMediaButtonPreferences(PlaybackSessionActions.buttons(snapshot))
            .build()
        mediaLibrarySession = session
        addSession(session)
        PlaybackSessionLifecycle.shared.attachSession(::shutdownSession)
        setListener(object : androidx.media3.session.MediaSessionService.Listener {
            override fun onForegroundServiceStartNotAllowedException() {
                Log.e(TAG, "Foreground playback was rejected by Android")
                dispatchCoreCommand(PlaybackRequest(PlaybackCommandKind.TransportPause))
            }
        })
        RustCoreBridge.observeEvents(::applyCoreEvent)
        coreExecutor.execute {
            try {
                hasRestoredSession = RustCoreBridge.restorePersistedSession()
                val restoredSnapshot = readSnapshot()
                mainHandler.post {
                    if (!isDestroyed) {
                        applySnapshot(restoredSnapshot)
                        isCoreReady = true
                        executeNextCommand()
                    }
                }
            } catch (error: Exception) {
                Log.e(TAG, "Native session restore failed", error)
                mainHandler.post {
                    if (!isDestroyed) {
                        initializationError = error.message ?: "Native session restore failed"
                        isCoreReady = true
                        executeNextCommand()
                    }
                }
            }
        }
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibrarySession? =
        mediaLibrarySession?.takeUnless { isDestroyed }

    override fun onTaskRemoved(rootIntent: Intent?) {
        PlaybackSessionLifecycle.shared.removeActivityTask()
    }

    override fun onDestroy() {
        shutdownSession()
        super.onDestroy()
    }

    // Release the session immediately: bound controllers can delay service destruction.
    private fun shutdownSession() {
        if (isDestroyed) return
        isDestroyed = true
        PlaybackSessionLifecycle.shared.detachSession()
        Log.d(TAG, "Shutting down playback session")
        pendingCommands.forEach { it.completion.set(failedResult("Playback service disconnected")) }
        pendingCommands.clear()
        RustCoreBridge.stopObservingEvents()
        audioFocus.release()
        if (wakeLock.isHeld) wakeLock.release()
        browser?.close()
        browser = null
        mediaLibrarySession?.let { session ->
            removeSession(session)
            session.release()
        }
        mediaLibrarySession = null
        sessionPlayer?.release()
        sessionPlayer = null
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        getSystemService(NotificationManager::class.java).cancel(DefaultMediaNotificationProvider.DEFAULT_NOTIFICATION_ID)
        coreExecutor.execute {
            try {
                RustCoreBridge.command(PlaybackCommandKind.Shutdown.action, "{}")
            } catch (error: Exception) {
                Log.e(TAG, "Native shutdown failed", error)
            }
        }
        stopSelf()
    }

    // Playback commands are acknowledged only after Rust execution and Media3 synchronization.
    private fun dispatchPlayerCommand(request: PlaybackRequest): ListenableFuture<Void?> =
        Futures.transform(dispatchCoreCommand(request), { result ->
            PlaybackSessionCommands.requireSuccess(checkNotNull(result))
            null
        }, MoreExecutors.directExecutor())

    private fun dispatchCoreCommand(request: PlaybackRequest): ListenableFuture<SessionResult> {
        val completion = SettableFuture.create<SessionResult>()
        if (isDestroyed) {
            completion.set(failedResult("Playback service is shutting down"))
            return completion
        }
        pendingCommands.addLast(PendingCommand(request, completion))
        executeNextCommand()
        return completion
    }

    // Startup decisions observe the preceding command's applied output/state snapshot.
    private fun executeNextCommand() {
        if (isDestroyed || !isCoreReady || isProcessingCommand) return
        val pending = pendingCommands.removeFirstOrNull() ?: return
        val request = pending.request
        val completion = pending.completion
        isProcessingCommand = true
        var requiresPlaybackStartup = false
        try {
            check(initializationError == null || request.kind == PlaybackCommandKind.Login) { initializationError.orEmpty() }
            requiresPlaybackStartup = request.kind.canStartPlayback && snapshot.isLocalPlayback &&
                (request.kind != PlaybackCommandKind.QueuePlayMedia ||
                    JSONObject(request.arguments).getJSONObject("request").optString("option") == "play")
            if (requiresPlaybackStartup) {
                startPlaybackForeground()
                check(audioFocus.acquire()) { "Audio focus is required to start playback" }
                pendingPlaybackStarts++
            }
        } catch (error: Exception) {
            applySnapshot(snapshot)
            triggerNotificationUpdate()
            completion.set(failedResult(error.message ?: "Playback startup failed"))
            isProcessingCommand = false
            mainHandler.post(::executeNextCommand)
            return
        }
        coreExecutor.execute {
            var resultValue: String? = null
            var commandError: Exception? = null
            var nextSnapshot: PlaybackSnapshot? = null
            try {
                if (request.kind == PlaybackCommandKind.RestorePersistedSession) {
                    resultValue = hasRestoredSession.toString()
                } else {
                    check(RustCoreBridge.command(request.kind.action, request.arguments)) { "Core command failed" }
                    if (request.kind == PlaybackCommandKind.Login) hasRestoredSession = true
                    if (request.kind == PlaybackCommandKind.Shutdown) hasRestoredSession = false
                }
            } catch (error: Exception) {
                commandError = error
            }
            try {
                nextSnapshot = readSnapshot()
            } catch (error: Exception) {
                if (commandError == null) commandError = error
            }
            val result = if (commandError == null) SessionResult(SessionResult.RESULT_SUCCESS,
                Bundle().apply { resultValue?.let { putString(PlaybackSessionCommands.RESULT_KEY, it) } })
            else failedResult(commandError.message ?: "Playback command failed")
            mainHandler.post {
                if (requiresPlaybackStartup) pendingPlaybackStarts--
                if (isDestroyed) {
                    completion.set(failedResult("Playback service disconnected"))
                } else {
                    if (request.kind == PlaybackCommandKind.Login && result.resultCode == SessionResult.RESULT_SUCCESS) initializationError = null
                    nextSnapshot?.let(::applySnapshot)
                    triggerNotificationUpdate()
                    completion.set(result)
                }
                isProcessingCommand = false
                executeNextCommand()
            }
        }
    }

    // Media3 takes over notification management after the pending startup completes.
    override fun onUpdateNotificationAsync(session: MediaSession, startInForegroundRequired: Boolean): ListenableFuture<Void?> =
        if (isDestroyed || pendingPlaybackStarts > 0) Futures.immediateFuture(null)
        else super.onUpdateNotificationAsync(session, startInForegroundRequired)

    private fun startPlaybackForeground() {
        val session = checkNotNull(mediaLibrarySession)
        ContextCompat.startForegroundService(this, Intent(this, PlaybackService::class.java))
        ServiceCompat.startForeground(this, DefaultMediaNotificationProvider.DEFAULT_NOTIFICATION_ID,
            notificationProvider.createStartupNotification(session), ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
        if (!wakeLock.isHeld) wakeLock.acquire()
    }

    // Snapshots are read off the main thread and projected only on the player looper.
    private fun readSnapshot(): PlaybackSnapshot {
        val state = JSONObject(checkNotNull(RustCoreBridge.query("mediaSessionState", null)))
        val settings = JSONObject(checkNotNull(RustCoreBridge.query("sessionSettings", null)))
        val userData = if (state.getJSONObject("queue").optJSONArray("items")?.length()?.let { it > 0 } == true) {
            try {
                RustCoreBridge.query("userData", null)?.let(::JSONObject)
            } catch (error: Exception) {
                Log.w(TAG, "Favorite state is unavailable", error)
                null
            }
        } else null
        return PlaybackSnapshotMapper.fromPlayerState(state.getJSONObject("playerState"), state.getJSONObject("queue"),
            settings.optString("musicAssistantUrl"), userData)
    }

    private fun applySnapshot(nextSnapshot: PlaybackSnapshot) {
        snapshot = nextSnapshot
        sessionPlayer?.updateState(nextSnapshot)
        mediaLibrarySession?.setMediaButtonPreferences(PlaybackSessionActions.buttons(nextSnapshot))
        if (nextSnapshot.hasPlaybackIntent && nextSnapshot.isLocalPlayback) {
            if (!wakeLock.isHeld) wakeLock.acquire()
        } else if (pendingPlaybackStarts == 0) {
            audioFocus.release()
            if (wakeLock.isHeld) wakeLock.release()
        }
    }

    // Core events only schedule reads; a long Rust operation never blocks Android's main thread.
    private fun applyCoreEvent(payload: String) {
        val eventType = JSONObject(payload).optString("type")
        if (isDestroyed || isSnapshotRefreshQueued || eventType !in listOf("player_state_changed", "queue_changed", "user_data_changed")) return
        isSnapshotRefreshQueued = true
        coreExecutor.execute {
            try {
                val nextSnapshot = readSnapshot()
                mainHandler.post {
                    isSnapshotRefreshQueued = false
                    if (!isDestroyed) applySnapshot(nextSnapshot)
                }
            } catch (error: Exception) {
                Log.e(TAG, "Snapshot refresh failed", error)
                mainHandler.post { isSnapshotRefreshQueued = false }
            }
        }
    }

    // Session-bound error mapping.
    private fun failedResult(message: String): SessionResult = SessionResult(SessionResult.RESULT_ERROR_UNKNOWN,
        Bundle().apply { putString(PlaybackSessionCommands.ERROR_KEY, message) })
}