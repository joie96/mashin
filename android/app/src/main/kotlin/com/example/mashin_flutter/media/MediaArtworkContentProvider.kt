package com.example.mashin_flutter.media

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.util.Log
import java.io.File
import java.net.URL
import java.security.MessageDigest
import java.util.Base64

class MediaArtworkContentProvider : ContentProvider() {
    companion object {
        private const val AUTHORITY = "com.example.mashin_flutter.mediaart"
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

    // Resolve remote artwork into the app cache because Media3 requires a readable local descriptor.
    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor? {
        val source = uri.getQueryParameter(SOURCE_PARAMETER) ?: return null
        val context = context ?: return null
        return runCatching {
            val cacheDirectory = File(context.cacheDir, "media-art").apply { mkdirs() }
            val target = File(cacheDirectory, sha256(source) + extensionFor(source))
            if (!target.exists() || target.length() == 0L) {
                // Populate a temporary file before replacing the cache entry to avoid partial artwork reads.
                val temporary = File(cacheDirectory, "${target.name}.tmp")
                if (source.startsWith("data:", ignoreCase = true)) {
                    temporary.writeBytes(decodeDataUri(source))
                } else {
                    URL(source).openConnection().getInputStream().use { input ->
                        temporary.outputStream().use(input::copyTo)
                    }
                }
                if (target.exists()) target.delete()
                temporary.renameTo(target)
            }
            ParcelFileDescriptor.open(target, ParcelFileDescriptor.MODE_READ_ONLY)
        }.onFailure { error ->
            Log.w(TAG, "Unable to load media artwork: ${error::class.java.simpleName}")
        }.getOrNull()
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

    private fun extensionFor(source: String): String = when {
        source.startsWith("data:image/png", ignoreCase = true) -> ".png"
        source.startsWith("data:image/webp", ignoreCase = true) -> ".webp"
        source.startsWith("data:image/jpeg", ignoreCase = true) -> ".jpg"
        else -> Regex("\\.(jpg|jpeg|png|webp)(?:[?#].*)?$", RegexOption.IGNORE_CASE)
            .find(source)?.groupValues?.get(1)?.let { ".$it" } ?: ".img"
    }

    private fun sha256(value: String): String = MessageDigest.getInstance("SHA-256")
        .digest(value.encodeToByteArray())
        .joinToString("") { byte -> "%02x".format(byte) }

}