package com.joie96.mashin.media.artwork

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.net.Uri
import androidx.core.content.ContextCompat
import androidx.core.graphics.ColorUtils
import com.joie96.mashin.R

/** Projects Flutter's mood symbols and dark cover palette for external media browsers. */
internal object MoodPlaylistArtwork {
    private data class Style(val iconResource: Int, val hue: Float)
    private val styles = mapOf(
        "relax" to Style(R.drawable.self_improvement_fill1, 155f),
        "focus" to Style(R.drawable.mindfulness_fill1, 200f),
        "energy" to Style(R.drawable.power_fill1, 35f),
        "melancholy" to Style(R.drawable.sentiment_sad_fill1, 225f),
        "love" to Style(R.drawable.favorite_fill1, 345f),
        "party" to Style(R.drawable.celebration_fill1, 285f),
        "happy" to Style(R.drawable.sentiment_excited_fill1, 50f),
        "nostalgic" to Style(R.drawable.local_cafe_fill1, 25f),
        "angry" to Style(R.drawable.sentiment_extremely_dissatisfied_fill1, 0f),
        "epic" to Style(R.drawable.mode_heat_fill1, 12f),
    )

    // Cover identities are independent of translated playlist names and server artwork.
    fun buildUri(itemId: String): Uri? {
        val prefix = "global_mood_playlist_"
        if (!itemId.startsWith(prefix)) return null
        val key = itemId.removePrefix(prefix)
        if (key !in styles) return null
        return Uri.Builder().scheme("content").authority(MediaArtworkContentProvider.AUTHORITY)
            .appendPath("mood").appendPath(key).build()
    }

    fun keyFromUri(uri: Uri): String? = uri.pathSegments.takeIf {
        uri.scheme == "content" && uri.authority == MediaArtworkContentProvider.AUTHORITY &&
            it.size == 2 && it[0] == "mood" && it[1] in styles
    }?.get(1)

    // Rendering keeps the same 8-bit HSL round trips and 44-percent symbol size as Flutter.
    fun render(context: Context, key: String): Bitmap {
        val style = styles.getValue(key)
        val accent = accentColor(key, style.hue)
        val foreground = ColorUtils.HSLToColor(floatArrayOf(accent[0], accent[1], 0.68f))
        val background = ColorUtils.HSLToColor(floatArrayOf(accent[0], accent[1] * 0.55f, 0.15f))
        val bitmap = Bitmap.createBitmap(512, 512, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        canvas.drawColor(background)
        val icon = checkNotNull(ContextCompat.getDrawable(context, style.iconResource)).mutate()
        icon.setTint(foreground)
        val size = (bitmap.width * 0.44f).toInt()
        val inset = (bitmap.width - size) / 2
        icon.setBounds(inset, inset, inset + size, inset + size)
        icon.draw(canvas)
        return bitmap
    }

    // Palette mapping mirrors accentColorFromText and MoodPlaylistCover at the Android boundary.
    private fun accentColor(key: String, hue: Float): FloatArray {
        var hash = 2166136261L
        for (unit in "121212:$key") hash = ((hash xor unit.code.toLong()) * 16777619L) and 0xFFFFFFFFL
        val color = ColorUtils.HSLToColor(floatArrayOf(
            ((hash and 0xFFFF) % 360).toFloat(),
            0.56f + ((hash shr 16) and 0xFF).toFloat() / 255f * 0.16f,
            0.50f + ((hash shr 24) and 0xFF).toFloat() / 255f * 0.12f,
        ))
        val accent = FloatArray(3)
        ColorUtils.colorToHSL(color, accent)
        accent[0] = hue
        ColorUtils.colorToHSL(ColorUtils.HSLToColor(accent), accent)
        return accent
    }
}