package com.joie96.mashin.media.artwork

import android.graphics.Color
import android.net.Uri
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class MoodPlaylistArtworkTest {
    @Test
    fun allAppMoodsRenderColoredCoversWithCenteredMaterialSymbols() {
        val context = RuntimeEnvironment.getApplication()
        for (key in listOf("relax", "focus", "energy", "melancholy", "love", "party", "happy", "nostalgic", "angry", "epic")) {
            val uri = checkNotNull(MoodPlaylistArtwork.buildUri("global_mood_playlist_$key"))
            assertEquals(key, MoodPlaylistArtwork.keyFromUri(uri))
            val cover = MoodPlaylistArtwork.render(context, key)
            try {
                assertEquals(512, cover.width)
                assertEquals(512, cover.height)
                val background = cover.getPixel(0, 0)
                assertEquals(255, Color.alpha(background))
                assertEquals(background, cover.getPixel(511, 511))
                var symbolPixels = 0
                for (vertical in 0 until 512) for (horizontal in 0 until 512) {
                    if (cover.getPixel(horizontal, vertical) != background) {
                        assertTrue(horizontal in 143..367 && vertical in 143..367)
                        symbolPixels++
                    }
                }
                assertTrue("$key must contain its Material symbol", symbolPixels > 100)
            } finally { cover.recycle() }
        }
    }

    @Test
    fun unrelatedPlaylistsAndInvalidContentPathsHaveNoMoodCover() {
        assertNull(MoodPlaylistArtwork.buildUri("relax"))
        assertNull(MoodPlaylistArtwork.buildUri("global_mood_playlist_unknown"))
        assertNull(MoodPlaylistArtwork.keyFromUri(Uri.parse("content://other/mood/relax")))
        assertNull(MoodPlaylistArtwork.keyFromUri(Uri.parse("content://com.joie96.mashin.mediaart/mood/relax/extra")))
    }
}