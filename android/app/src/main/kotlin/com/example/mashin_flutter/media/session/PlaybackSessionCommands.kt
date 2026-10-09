package com.example.mashin_flutter.media.session

import android.os.Bundle
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionResult

internal enum class PlaybackCommandKind(val action: String, val canStartPlayback: Boolean = false) {
    Login("login"),
    RestorePersistedSession("restorePersistedSession"),
    SelectDefaultSendspinOutput("selectDefaultSendspinOutput"),
    SelectOfflineLibrary("selectOfflineLibrary"),
    SelectRemotePlayer("selectRemotePlayer"),
    ActivateEngine("activateEngine"),
    DeactivateEngine("deactivateEngine"),
    Shutdown("shutdown"),
    TransportPlay("transportPlay", true),
    TransportPause("transportPause"),
    TransportStop("transportStop"),
    TransportNext("transportNext", true),
    TransportPrevious("transportPrevious", true),
    TransportSeek("transportSeek"),
    TransportRepeatMode("transportRepeatMode"),
    TransportShuffle("transportShuffle"),
    SetAutoplay("setAutoplay"),
    SetVolume("setVolume"),
    SetMuted("setMuted"),
    SetTheme("setTheme"),
    SetLogLevel("setLogLevel"),
    SetListenBrainzCredentials("setListenBrainzCredentials"),
    SetUserDataFavorites("setUserDataFavorites"),
    QueuePlayMedia("queuePlayMedia", true),
    StartSendspinRadio("startSendspinRadio", true),
    InsertQueueItems("insertQueueItems"),
    AppendQueueItems("appendQueueItems"),
    MoveQueueItems("moveQueueItems"),
    DeleteQueueItems("deleteQueueItems"),
    SelectQueueItem("selectQueueItem"),
    PlayQueueIndex("playQueueIndex", true),
    RequestCodec("requestCodec"),
    SetLocalCacheSizeBytes("setLocalCacheSizeBytes"),
}

// Serialized arguments cross the session/JNI boundary; Rust validates their typed contract.
internal data class PlaybackRequest(val kind: PlaybackCommandKind, val arguments: String = "{}")

@UnstableApi
internal object PlaybackSessionCommands {
    const val COMMAND_KEY = "command"
    const val ARGUMENTS_KEY = "arguments"
    const val RESULT_KEY = "result"
    const val ERROR_KEY = "error"
    val execute = SessionCommand("mashin.executePlaybackCommand", Bundle.EMPTY)

    // A completed custom command may still contain a failure; player futures must fail too.
    fun requireSuccess(result: SessionResult) {
        check(result.resultCode == SessionResult.RESULT_SUCCESS) {
            result.extras.getString(ERROR_KEY) ?: "Playback command failed"
        }
    }

    // Session boundary conversion.
    fun encode(request: PlaybackRequest): Bundle = Bundle().apply {
        putString(COMMAND_KEY, request.kind.action)
        putString(ARGUMENTS_KEY, request.arguments)
    }

    fun decode(arguments: Bundle): PlaybackRequest {
        val action = requireNotNull(arguments.getString(COMMAND_KEY)) { "Missing playback command" }
        val kind = PlaybackCommandKind.entries.singleOrNull { it.action == action }
            ?: throw IllegalArgumentException("Unsupported playback command: $action")
        return PlaybackRequest(kind, arguments.getString(ARGUMENTS_KEY) ?: "{}")
    }
}