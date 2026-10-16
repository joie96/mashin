package com.example.mashin_flutter.media.session

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PlaybackSessionCommandsTest {
    @Test
    fun commandActionsAreUniqueAndUseTheFlutterContract() {
        val actions = PlaybackCommandKind.entries.map { it.action }
        assertEquals(actions.size, actions.distinct().size)
        assertTrue(actions.containsAll(listOf("transportPlay", "transportPause", "transportSeek", "queuePlayMedia", "selectRemotePlayer")))
        assertFalse(actions.any { it in listOf("play", "pause", "seekSeconds", "activate") })
    }

    @Test
    fun everyPlaybackStartIsClassifiedBeforeCoreDispatch() {
        for (kind in listOf(PlaybackCommandKind.TransportPlay, PlaybackCommandKind.TransportNext,
            PlaybackCommandKind.TransportPrevious, PlaybackCommandKind.QueuePlayMedia,
            PlaybackCommandKind.StartRadio, PlaybackCommandKind.PlayQueueIndex)) {
            assertTrue(kind.action, kind.canStartPlayback)
        }
        assertFalse(PlaybackCommandKind.TransportPause.canStartPlayback)
        assertFalse(PlaybackCommandKind.Shutdown.canStartPlayback)
    }
}