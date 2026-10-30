package com.joie96.mashin.flutter

import android.os.Bundle
import android.os.Looper
import androidx.media3.session.SessionResult
import com.joie96.mashin.media.session.PlaybackCommandKind
import com.joie96.mashin.media.session.PlaybackRequest
import com.joie96.mashin.media.session.PlaybackSessionCommands
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.FlutterException
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.StandardMethodCodec
import java.nio.ByteBuffer
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class FlutterPlaybackChannelTest {
    private class RecordingMessenger : BinaryMessenger {
        var handler: BinaryMessenger.BinaryMessageHandler? = null
        override fun send(channel: String, message: ByteBuffer?) = Unit
        override fun send(channel: String, message: ByteBuffer?, callback: BinaryMessenger.BinaryReply?) = Unit
        override fun setMessageHandler(channel: String, handler: BinaryMessenger.BinaryMessageHandler?) {
            this.handler = handler
        }
    }

    private class FakeClient : PlaybackCommandClient {
        val completion = SettableFuture.create<SessionResult>()
        var request: PlaybackRequest? = null
        var hasDisconnected = false
        override fun disconnect() { hasDisconnected = true }
        override fun execute(request: PlaybackRequest): ListenableFuture<SessionResult> {
            this.request = request
            return completion
        }
    }

    @Test
    fun commandWaitsForSessionAcknowledgementAndPreservesItsResult() {
        val messenger = RecordingMessenger()
        val client = FakeClient()
        FlutterPlaybackChannel(RuntimeEnvironment.getApplication(), messenger, client)
        var response: ByteBuffer? = null
        val message = StandardMethodCodec.INSTANCE.encodeMethodCall(MethodCall("execute",
            mapOf("command" to "restorePersistedSession", "arguments" to emptyMap<String, Any>())))
        message.flip()
        messenger.handler!!.onMessage(message) { response = it }
        assertEquals(PlaybackCommandKind.RestorePersistedSession, client.request!!.kind)
        assertNull(response)
        client.completion.set(SessionResult(SessionResult.RESULT_SUCCESS,
            Bundle().apply { putString(PlaybackSessionCommands.RESULT_KEY, "true") }))
        shadowOf(Looper.getMainLooper()).idle()
        val envelope = checkNotNull(response)
        envelope.flip()
        assertEquals(true, StandardMethodCodec.INSTANCE.decodeEnvelope(envelope))
    }

    @Test
    fun logoutWaitsForAcknowledgementAndDoesNotStartPlayback() {
        val messenger = RecordingMessenger()
        val client = FakeClient()
        FlutterPlaybackChannel(RuntimeEnvironment.getApplication(), messenger, client)
        var response: ByteBuffer? = null
        val message = StandardMethodCodec.INSTANCE.encodeMethodCall(MethodCall("execute",
            mapOf("command" to "logout", "arguments" to emptyMap<String, Any>())))
        message.flip()
        messenger.handler!!.onMessage(message) { response = it }
        assertEquals(PlaybackCommandKind.Logout, client.request!!.kind)
        assertFalse(client.request!!.canStartPlayback)
        assertNull(response)
        client.completion.set(SessionResult(SessionResult.RESULT_SUCCESS))
        shadowOf(Looper.getMainLooper()).idle()
        val envelope = checkNotNull(response)
        envelope.flip()
        assertNull(StandardMethodCodec.INSTANCE.decodeEnvelope(envelope))
    }

    @Test
    fun sessionErrorIsReturnedWithoutExecutingAnotherCommand() {
        val messenger = RecordingMessenger()
        val client = FakeClient()
        FlutterPlaybackChannel(RuntimeEnvironment.getApplication(), messenger, client)
        var response: ByteBuffer? = null
        val message = StandardMethodCodec.INSTANCE.encodeMethodCall(MethodCall("execute",
            mapOf("command" to "transportPlay", "arguments" to emptyMap<String, Any>())))
        message.flip()
        messenger.handler!!.onMessage(message) { response = it }
        client.completion.set(SessionResult(SessionResult.RESULT_ERROR_UNKNOWN,
            Bundle().apply { putString(PlaybackSessionCommands.ERROR_KEY, "focus denied") }))
        shadowOf(Looper.getMainLooper()).idle()
        val envelope = checkNotNull(response)
        envelope.flip()
        val error = assertThrows(FlutterException::class.java) { StandardMethodCodec.INSTANCE.decodeEnvelope(envelope) }
        assertEquals("playback_command_failed", error.code)
        assertEquals("focus denied", error.message)
    }

    @Test
    fun engineDetachDisconnectsWithoutSendingShutdown() {
        val messenger = RecordingMessenger()
        val client = FakeClient()
        val channel = FlutterPlaybackChannel(RuntimeEnvironment.getApplication(), messenger, client)
        channel.disconnect()
        assertTrue(client.hasDisconnected)
        assertNull(client.request)
        assertNull(messenger.handler)
    }
}