package com.example.mashin_flutter.media.session

import android.os.Bundle
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionResult
import com.example.mashin_flutter.media.library.MediaLibraryBrowser
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture

@UnstableApi
internal class PlaybackSessionCallback(
    private val packageName: String,
    browser: MediaLibraryBrowser,
    private val dispatchCommand: (PlaybackRequest) -> ListenableFuture<SessionResult>,
) : MediaLibraryService.MediaLibrarySession.Callback by browser {
    // Session access: app-only lifecycle commands, standard media access for other controllers.
    override fun onConnectAsync(session: MediaSession, controller: MediaSession.ControllerInfo): ListenableFuture<MediaSession.ConnectionResult> {
        val commands = MediaSession.ConnectionResult.DEFAULT_SESSION_AND_LIBRARY_COMMANDS.buildUpon()
        if (controller.packageName == packageName) commands.add(PlaybackSessionCommands.execute)
        return Futures.immediateFuture(MediaSession.ConnectionResult.AcceptedResultBuilder()
            .setAvailableSessionCommands(commands.build())
            .setAvailablePlayerCommands(MediaSession.ConnectionResult.DEFAULT_PLAYER_COMMANDS)
            .build())
    }

    // Custom commands preserve the service's asynchronous acknowledgement.
    override fun onCustomCommand(
        session: MediaSession,
        controller: MediaSession.ControllerInfo,
        customCommand: SessionCommand,
        args: Bundle,
    ): ListenableFuture<SessionResult> {
        if (controller.packageName != packageName || customCommand.customAction != PlaybackSessionCommands.execute.customAction) {
            return Futures.immediateFuture(SessionResult(SessionResult.RESULT_ERROR_PERMISSION_DENIED))
        }
        return try {
            dispatchCommand(PlaybackSessionCommands.decode(args))
        } catch (error: IllegalArgumentException) {
            Futures.immediateFuture(SessionResult(SessionResult.RESULT_ERROR_BAD_VALUE,
                Bundle().apply { putString(PlaybackSessionCommands.ERROR_KEY, error.message) }))
        }
    }
}