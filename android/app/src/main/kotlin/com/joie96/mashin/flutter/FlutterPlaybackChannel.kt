package com.joie96.mashin.flutter

import android.content.Context
import android.os.Handler
import android.os.Looper
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.SessionResult
import com.joie96.mashin.media.session.PlaybackCommandKind
import com.joie96.mashin.media.session.PlaybackRequest
import com.joie96.mashin.media.session.PlaybackSessionCommands
import com.google.common.util.concurrent.Futures
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.json.JSONObject
import org.json.JSONTokener

@UnstableApi
internal class FlutterPlaybackChannel(
    context: Context,
    messenger: BinaryMessenger,
    private val connection: PlaybackCommandClient = PlaybackControllerConnection(context.applicationContext),
) {
    private val channel = MethodChannel(messenger, "mashin/playback")
    private val mainHandler = Handler(Looper.getMainLooper())
    private val mainExecutor = java.util.concurrent.Executor { mainHandler.post(it) }

    // Flutter-engine connection lifecycle.
    init {
        channel.setMethodCallHandler(::handleMethodCall)
    }

    fun disconnect() {
        channel.setMethodCallHandler(null)
        connection.disconnect()
    }

    // Method-channel boundary validation and result mapping.
    private fun handleMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "disconnect" -> {
                connection.disconnect()
                result.success(null)
            }
            "execute" -> {
                val request = try {
                    decodeRequest(call)
                } catch (error: IllegalArgumentException) {
                    result.error("invalid_playback_command", error.message, null)
                    return
                }
                val completion = try {
                    connection.execute(request)
                } catch (error: Exception) {
                    result.error("service_unavailable", error.message, null)
                    return
                }
                completion.addListener({
                    try {
                        val response = Futures.getDone(completion)
                        if (response.resultCode != SessionResult.RESULT_SUCCESS) {
                            result.error("playback_command_failed", response.extras.getString(PlaybackSessionCommands.ERROR_KEY), response.resultCode)
                        } else {
                            val value = response.extras.getString(PlaybackSessionCommands.RESULT_KEY)
                                ?.let { JSONTokener(it).nextValue() }
                            result.success(value.takeUnless { it == JSONObject.NULL })
                        }
                    } catch (error: Exception) {
                        result.error("service_unavailable", error.cause?.message ?: error.message, null)
                    }
                }, mainExecutor)
            }
            else -> result.notImplemented()
        }
    }

    private fun decodeRequest(call: MethodCall): PlaybackRequest {
        val action = requireNotNull(call.argument<String>("command")) { "Missing playback command" }
        val kind = PlaybackCommandKind.entries.singleOrNull { it.action == action }
            ?: throw IllegalArgumentException("Unsupported playback command: $action")
        val arguments = requireNotNull(call.argument<Map<String, Any?>>("arguments")) { "Missing command arguments" }
        return PlaybackRequest(kind, JSONObject(arguments).toString())
    }
}