package com.joie96.mashin.media.artwork

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.util.Log
import java.net.URL
import java.util.Base64

class MediaArtworkContentProvider : ContentProvider() {
    companion object {
        private const val AUTHORITY = "com.joie96.mashin.mediaart"
        private const val SOURCE_PARAMETER = "src"
        private const val TAG = "MediaArtworkProvider"

        fun buildUri(source: String): String = Uri.Builder()
            .scheme("content")
            .authority(AUTHORITY)
            .appendPath("art")
            .appendQueryParameter(SOURCE_PARAMETER, source)
            .build()
            .toString()
    }

    override fun onCreate(): Boolean = true

    override fun getType(uri: Uri): String = "image/*"

    // Media3 receives a one-shot descriptor without retaining artwork files.
    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor? {
        if (mode != "r") return null
        val source = uri.getQueryParameter(SOURCE_PARAMETER) ?: return null
        return openPipeHelper(uri, "image/*", null, source) { output, _, _, _, _ ->
            runCatching {
                ParcelFileDescriptor.AutoCloseOutputStream(output).use { stream ->
                    if (source.startsWith("data:", ignoreCase = true)) {
                        stream.write(decodeDataUri(source))
                    } else {
                        URL(source).openConnection().apply { useCaches = false }.getInputStream().use { input ->
                            input.copyTo(stream)
                        }
                    }
                }
            }.onFailure { error ->
                Log.w(TAG, "Unable to load media artwork: ${error::class.java.simpleName}")
            }
        }
    }

    override fun query(uri: Uri, projection: Array<out String>?, selection: String?, selectionArgs: Array<out String>?, sortOrder: String?): Cursor? = null

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = 0

    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?): Int = 0

    private fun decodeDataUri(source: String): ByteArray {
        val separator = source.indexOf(',')
        require(separator > 0) { "Invalid data URI" }
        val header = source.substring(0, separator)
        val payload = source.substring(separator + 1)
        return if (header.contains(";base64", ignoreCase = true)) {
            Base64.getDecoder().decode(payload)
        } else {
            Uri.decode(payload).encodeToByteArray()
        }
    }

}