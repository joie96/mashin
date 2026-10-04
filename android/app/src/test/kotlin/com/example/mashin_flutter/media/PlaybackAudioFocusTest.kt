package com.example.mashin_flutter.media

import android.media.AudioManager
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PlaybackAudioFocusTest {
    @Test
    fun deniedFocusCanBeRetriedAndReleased() {
        val context = RuntimeEnvironment.getApplication()
        val manager = context.getSystemService(AudioManager::class.java)
        val focus = PlaybackAudioFocus(context) {}
        shadowOf(manager).setNextFocusRequestResponse(AudioManager.AUDIOFOCUS_REQUEST_FAILED)
        assertFalse(focus.acquire())
        shadowOf(manager).setNextFocusRequestResponse(AudioManager.AUDIOFOCUS_REQUEST_GRANTED)
        assertTrue(focus.acquire())
        focus.release()
        assertNotNull(shadowOf(manager).lastAbandonedAudioFocusRequest)
    }
}