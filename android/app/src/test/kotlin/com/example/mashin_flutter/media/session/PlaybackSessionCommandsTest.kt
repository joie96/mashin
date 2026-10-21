package com.example.mashin_flutter.media.session

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PlaybackSessionCommandsTest {
    @Test
    fun commandActionsAreUniqueAndUseTheFlutterContract() {
        val actions = PlaybackCommandKind.entries.map { it.action }
        assertEquals(actions.size, actions.distinct().size)
        assertTrue(actions.containsAll(listOf("transportPlay", "transportPause", "transportSeek", "queuePlayMedia", "selectRemotePlayer",
            "moveQueueItem", "moveQueueItemToEnd", "deleteQueueItems")))
        assertFalse(actions.any { it in listOf("play", "pause", "seekSeconds", "activate",
            "insertQueueItems", "appendQueueItems", "moveQueueItems", "deleteQueueItem") })
    }

    @Test
    fun everyPlaybackStartIsClassifiedBeforeCoreDispatch() {
        for (kind in listOf(PlaybackCommandKind.TransportPlay, PlaybackCommandKind.TransportNext,
            PlaybackCommandKind.TransportPrevious, PlaybackCommandKind.QueuePlayMedia,
            PlaybackCommandKind.StartRadio, PlaybackCommandKind.SelectQueueItem, PlaybackCommandKind.PlayQueueIndex)) {
            assertTrue(kind.action, kind.canStartPlayback)
        }
        assertFalse(PlaybackCommandKind.TransportPause.canStartPlayback)
        assertFalse(PlaybackCommandKind.Shutdown.canStartPlayback)
    }

    @Test
    fun mediaPlayStartupFollowsOptionAndContent() {
        for (option in listOf("play", "replace", "next", "replace_next", "add")) {
            val request = PlaybackRequest(PlaybackCommandKind.QueuePlayMedia,
                """{"request":{"mediaItems":[{}],"option":"$option"}}""")
            assertEquals(option, option in listOf("play", "replace"), request.canStartPlayback)
        }
        assertFalse(PlaybackRequest(PlaybackCommandKind.QueuePlayMedia,
            """{"request":{"mediaItems":[],"option":"replace"}}""").canStartPlayback)
    }

    @Test
    fun preferredProviderSettingsRoundTripWithoutStartingPlayback() {
        for (arguments in listOf(
            """{"preferredProvider":{"providerInstance":"tidal-instance","providerDomain":"tidal"}}""",
            """{"preferredProvider":null}""",
        )) {
            val request = PlaybackRequest(PlaybackCommandKind.SetPreferredProvider, arguments)
            assertEquals("setPreferredProvider", request.kind.action)
            assertFalse(request.canStartPlayback)
            assertEquals(request, PlaybackSessionCommands.decode(PlaybackSessionCommands.encode(request)))
        }
    }
}