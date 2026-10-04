package com.example.mashin_flutter.flutter

import android.content.ComponentName
import android.content.Context
import android.os.Handler
import android.os.Looper
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaController
import androidx.media3.session.SessionResult
import androidx.media3.session.SessionToken
import com.example.mashin_flutter.media.PlaybackService
import com.example.mashin_flutter.media.session.PlaybackRequest
import com.example.mashin_flutter.media.session.PlaybackSessionCommands
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import java.util.concurrent.TimeoutException

internal interface PlaybackCommandClient {
    fun disconnect()
    fun execute(request: PlaybackRequest): ListenableFuture<SessionResult>
}

/** UI-scoped binding; releasing it never terminates foreground playback. */
@UnstableApi
internal class PlaybackControllerConnection(
    private val context: Context,
    private val createController: ((MediaController.Listener) -> ListenableFuture<MediaController>)? = null,
    private val releaseController: (ListenableFuture<MediaController>) -> Unit = MediaController::releaseFuture,
) : PlaybackCommandClient {
    private val mainHandler = Handler(Looper.getMainLooper())
    private var controllerFuture: ListenableFuture<MediaController>? = null
    private var connectionFuture: SettableFuture<MediaController>? = null
    private var connectionTimeout: Runnable? = null
    private val mainExecutor = java.util.concurrent.Executor { mainHandler.post(it) }

    // Connection lifecycle.
    fun connect(): ListenableFuture<MediaController> {
        connectionFuture?.let { return it }
        val connected = SettableFuture.create<MediaController>()
        connectionFuture = connected
        val listener = object : MediaController.Listener {
                override fun onDisconnected(controller: MediaController) {
                    val current = connectionFuture
                    if (current != null && current.isDone && runCatching { Futures.getDone(current) }.getOrNull() === controller) {
                        disconnect()
                    }
                }
            }
        val pending = createController?.invoke(listener) ?: MediaController.Builder(context,
            SessionToken(context, ComponentName(context, PlaybackService::class.java)))
            .setListener(listener).buildAsync()
        controllerFuture = pending
        val timeout = Runnable {
            if (connected.setException(TimeoutException("Playback service connection timed out"))) {
                disconnect()
            }
        }
        connectionTimeout = timeout
        mainHandler.postDelayed(timeout, 10_000L)
        pending.addListener({
            mainHandler.removeCallbacks(timeout)
            if (connectionFuture !== connected) return@addListener
            try {
                connected.set(Futures.getDone(pending))
            } catch (error: Exception) {
                connected.setException(error)
                disconnect()
            }
        }, mainExecutor)
        return connected
    }

    override fun disconnect() {
        connectionTimeout?.let(mainHandler::removeCallbacks)
        connectionTimeout = null
        val pending = controllerFuture
        controllerFuture = null
        connectionFuture?.setException(IllegalStateException("Playback service disconnected"))
        connectionFuture = null
        if (pending != null) releaseController(pending)
    }

    // Acknowledged session command transport; uncertain commands are never replayed.
    override fun execute(request: PlaybackRequest): ListenableFuture<SessionResult> = Futures.transformAsync(
        connect(),
        { controller ->
            checkNotNull(controller).sendCustomCommand(
                PlaybackSessionCommands.execute,
                PlaybackSessionCommands.encode(request),
            )
        },
        mainExecutor,
    )
}