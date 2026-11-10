package com.joie96.mashin.media.artwork

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.util.Log
import com.joie96.mashin.core.RustCoreBridge
import org.json.JSONObject
import java.util.Base64

open class MediaArtworkContentProvider : ContentProvider() {
    companion object {
        private const val AUTHORITY = "com.joie96.mashin.mediaart"
        private const val REQUEST_PARAMETER = "request"
        private const val TAG = "MediaArtworkProvider"

        fun reviseUri(uri: Uri?, proxyId: String?, revision: Long): Uri? {
            if (uri == null || uri.authority != AUTHORITY) return null
            val request = uri.getQueryParameter(REQUEST_PARAMETER)?.let { runCatching { JSONObject(it) }.getOrNull() } ?: return null
            if (proxyId != null && request.optJSONObject("image")?.optString("proxy_id") != proxyId) return null
            return uri.buildUpon().clearQuery()
                .appendQueryParameter(REQUEST_PARAMETER, request.toString())
                .appendQueryParameter("revision", revision.toString()).build()
        }

        fun retainRevision(previous: Uri?, next: Uri?): Uri? =
            if (previous != null && next != null && previous.authority == AUTHORITY && next.authority == AUTHORITY &&
                previous.getQueryParameter(REQUEST_PARAMETER) == next.getQueryParameter(REQUEST_PARAMETER)) previous else next

        // Album precedence and original metadata positions are shared by all Android consumers.
        fun buildUri(item: JSONObject): String? {
            val mediaItem = item.optJSONObject("media_item")
            val album = mediaItem?.optJSONObject("album") ?: item.optJSONObject("album")
            for (owner in listOfNotNull(album, item, mediaItem)) {
                val ownerUri = owner.optString("uri").takeIf { it.isNotBlank() && it != "null" }
                owner.optJSONObject("image")?.let { image ->
                    buildImageUri(image, ownerUri, 0)?.let { return it }
                }
                val images = owner.optJSONObject("metadata")?.optJSONArray("images") ?: continue
                val ordered = (0 until images.length()).mapNotNull { position ->
                    images.optJSONObject(position)?.let { IndexedValue(position, it) }
                }.sortedBy { if (it.value.optString("type", "thumb") == "thumb") 0 else 1 }
                for ((position, image) in ordered) {
                    buildImageUri(image, ownerUri, position)?.let { return it }
                }
            }
            return null
        }

        // Proxy paths need not expose server URLs in externally readable content descriptors.
        private fun buildImageUri(image: JSONObject, ownerUri: String?, position: Int): String? {
            val proxyId = image.optString("proxy_id").takeUnless { it == "null" }.orEmpty()
            val path = image.optString("path").takeUnless { it == "null" }.orEmpty()
            if (proxyId.isNotEmpty() && !proxyId.matches(Regex("[a-fA-F0-9]{64}"))) return null
            if (proxyId.isEmpty() && image.optJSONObject("cached_inline") == null &&
                !path.startsWith("data:") && !path.trimStart().startsWith('<')) return null
            val isInline = path.startsWith("data:") || path.trimStart().startsWith('<')
            val model = JSONObject()
                .put("type", image.optString("type", "thumb"))
                .put("provider", image.optString("provider", "builtin"))
                .put("path", if (proxyId.isNotEmpty() && !isInline) "" else path)
                .put("proxy_id", proxyId.takeIf { it.isNotEmpty() } ?: JSONObject.NULL)
                .put("remotely_accessible", image.optBoolean("remotely_accessible"))
            image.optJSONObject("cached_inline")?.let { model.put("cached_inline", it) }
            val request = JSONObject().put("image", model)
                .put("ownerUri", ownerUri ?: JSONObject.NULL)
                .put("position", position).put("size", 512)
                .put("providerIconRole", JSONObject.NULL)
            return Uri.Builder()
                .scheme("content")
                .authority(AUTHORITY)
                .appendPath("art")
                .appendQueryParameter(REQUEST_PARAMETER, request.toString())
                .build()
                .toString()
        }
    }

    override fun onCreate(): Boolean = true

    override fun getType(uri: Uri): String = "image/*"

    // Media3 receives a one-shot descriptor without retaining artwork files.
    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor? {
        if (mode != "r") return null
        val request = uri.getQueryParameter(REQUEST_PARAMETER)?.let { runCatching { JSONObject(it) }.getOrNull() } ?: return null
        return openPipeHelper(uri, "image/*", null, request) { output, _, _, _, _ ->
            runCatching {
                ParcelFileDescriptor.AutoCloseOutputStream(output).use { stream ->
                    var snapshot = queryImage(JSONObject().put("request", request).put("refresh", false))
                    if (snapshot.isNull("bytes") && snapshot.optBoolean("hasRefresh")) {
                        snapshot = queryImage(JSONObject().put("request", request).put("refresh", true))
                    }
                    if (!snapshot.isNull("bytes")) stream.write(Base64.getDecoder().decode(snapshot.getString("bytes")))
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

    // The JNI boundary owns validation, server pairing, files and optional miss refresh.
    protected open fun queryImage(argument: JSONObject): JSONObject =
        JSONObject(checkNotNull(RustCoreBridge.query("readImage", argument.toString())))

}