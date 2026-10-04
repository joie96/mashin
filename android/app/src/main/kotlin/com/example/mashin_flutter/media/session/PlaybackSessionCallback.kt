package com.example.mashin_flutter.media.session

import android.os.Bundle
import androidx.media3.common.MediaItem
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionResult
import com.example.mashin_flutter.media.library.MediaLibraryBrowser
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture

@UnstableApi
internal class PlaybackSessionCallback(
    private val packageName: String,
    private val browser: MediaLibraryBrowser,
    private val dispatchCommand: (PlaybackRequest) -> ListenableFuture<SessionResult>,
) : MediaLibraryService.MediaLibrarySession.Callback {
    // Session access: app-only lifecycle commands, standard media access for other controllers.
    override fun onConnect(session: MediaSession, controller: MediaSession.ControllerInfo): MediaSession.ConnectionResult {
        val commands = MediaSession.ConnectionResult.DEFAULT_SESSION_AND_LIBRARY_COMMANDS.buildUpon()
        if (controller.packageName == packageName) commands.add(PlaybackSessionCommands.execute)
        return MediaSession.ConnectionResult.AcceptedResultBuilder()
            .setAvailableSessionCommands(commands.build())
            .setAvailablePlayerCommands(MediaSession.ConnectionResult.DEFAULT_PLAYER_COMMANDS)
            .build()
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

    // Explicit forwarding is required for Java default callbacks used by legacy browsers.
    override fun onSetMediaItems(
        session: MediaSession,
        controller: MediaSession.ControllerInfo,
        mediaItems: List<MediaItem>,
        startIndex: Int,
        startPositionMs: Long,
    ): ListenableFuture<MediaSession.MediaItemsWithStartPosition> =
        browser.onSetMediaItems(session, controller, mediaItems, startIndex, startPositionMs)

    override fun onGetLibraryRoot(
        session: MediaLibraryService.MediaLibrarySession,
        controller: MediaSession.ControllerInfo,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<MediaItem>> = browser.onGetLibraryRoot(session, controller, params)

    override fun onGetChildren(
        session: MediaLibraryService.MediaLibrarySession,
        controller: MediaSession.ControllerInfo,
        parentId: String,
        page: Int,
        pageSize: Int,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> =
        browser.onGetChildren(session, controller, parentId, page, pageSize, params)

    override fun onGetItem(
        session: MediaLibraryService.MediaLibrarySession,
        controller: MediaSession.ControllerInfo,
        mediaId: String,
    ): ListenableFuture<LibraryResult<MediaItem>> = browser.onGetItem(session, controller, mediaId)

    override fun onSearch(
        session: MediaLibraryService.MediaLibrarySession,
        controller: MediaSession.ControllerInfo,
        query: String,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<Void>> = browser.onSearch(session, controller, query, params)

    override fun onGetSearchResult(
        session: MediaLibraryService.MediaLibrarySession,
        controller: MediaSession.ControllerInfo,
        query: String,
        page: Int,
        pageSize: Int,
        params: MediaLibraryService.LibraryParams?,
    ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> =
        browser.onGetSearchResult(session, controller, query, page, pageSize, params)
}