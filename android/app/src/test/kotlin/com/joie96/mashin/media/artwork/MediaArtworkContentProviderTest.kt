package com.joie96.mashin.media.artwork

import android.net.Uri
import android.content.pm.ProviderInfo
import android.os.AsyncTask
import android.os.ParcelFileDescriptor
import java.util.Base64
import java.util.concurrent.CompletableFuture
import java.util.concurrent.Executor
import java.util.concurrent.TimeUnit
import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.LooperMode
import org.robolectric.util.ReflectionHelpers

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
@LooperMode(LooperMode.Mode.LEGACY)
class MediaArtworkContentProviderTest {
    @Test
    fun presentationRevisionsTargetOnlyMatchingProxyAndPreserveNativeRequest() {
        val proxy = "a".repeat(64)
        val original = Uri.parse(MediaArtworkContentProvider.buildUri(JSONObject().put("image",JSONObject().put("proxy_id",proxy))))
        assertNull(MediaArtworkContentProvider.reviseUri(original,"b".repeat(64),1))
        val revised = MediaArtworkContentProvider.reviseUri(original,proxy,2)!!
        assertEquals("2",revised.getQueryParameter("revision"))
        assertEquals(original.getQueryParameter("request"),revised.getQueryParameter("request"))
        assertEquals(revised,MediaArtworkContentProvider.retainRevision(revised,original))
        val cleared = MediaArtworkContentProvider.reviseUri(revised,null,3)!!
        assertEquals("3",cleared.getQueryParameter("revision"))
        assertNull(MediaArtworkContentProvider.reviseUri(Uri.parse("https://example.invalid/art"),null,4))
    }

    // Only the native query supplies bytes; the pipe owns no persistent image files.
    @Test
    fun inlineArtworkReadsDoNotCreatePersistentFiles() {
        val directory = RuntimeEnvironment.getApplication().cacheDir
        val before = directory.walkTopDown().map { it.relativeTo(directory).path }.toSet()
        val svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"/>"
        val bytes = svg.encodeToByteArray()
        val calls = mutableListOf<JSONObject>()
        val provider = object : MediaArtworkContentProvider() {
            override fun queryImage(argument: JSONObject): JSONObject {
                calls.add(argument)
                assertEquals("library://playlist/42", argument.getJSONObject("request").getJSONObject("image").getJSONObject("cached_inline").getString("owner_uri"))
                return if (!argument.getBoolean("refresh")) JSONObject().put("bytes", JSONObject.NULL).put("hasRefresh", true)
                    else JSONObject().put("bytes", Base64.getEncoder().encodeToString(bytes)).put("hasRefresh", false)
            }
        }
        provider.attachInfo(RuntimeEnvironment.getApplication(), ProviderInfo().apply { authority = "com.joie96.mashin.mediaart" })
        val image = JSONObject("""{"type":"thumb","provider":"builtin","cached_inline":{"owner_uri":"library://playlist/42","position":1}}""")
        val uri = Uri.parse(MediaArtworkContentProvider.buildUri(JSONObject().put("image", image)))
        assertArrayEquals(bytes, readPipe(provider, uri))
        assertEquals(listOf(false, true), calls.map { it.getBoolean("refresh") })
        assertEquals(before, directory.walkTopDown().map { it.relativeTo(directory).path }.toSet())
        provider.shutdown()
    }

    @Test
    fun localNativeBytesDoNotWaitForAnOptionalRefresh() {
        var calls = 0
        val provider = object : MediaArtworkContentProvider() {
            override fun queryImage(argument: JSONObject): JSONObject {
                calls++
                assertEquals(false, argument.getBoolean("refresh"))
                return JSONObject().put("bytes", "AQID").put("hasRefresh", true).put("cacheError", "synthetic storage failure")
            }
        }
        provider.attachInfo(RuntimeEnvironment.getApplication(), ProviderInfo().apply { authority = "com.joie96.mashin.mediaart" })
        val item = JSONObject("""{"image":{"cached_inline":{"owner_uri":"library://playlist/42","position":0}}}""")
        assertArrayEquals(byteArrayOf(1, 2, 3), readPipe(provider, Uri.parse(MediaArtworkContentProvider.buildUri(item))))
        assertEquals(1, calls)
        provider.shutdown()
    }

    @Test
    fun missingSourceAndWriteRequestsDoNotOpenArtwork() {
        val provider = Robolectric.buildContentProvider(MediaArtworkContentProvider::class.java)
            .create().get()
        assertNull(provider.openFile(Uri.parse("content://com.joie96.mashin.mediaart/art"), "r"))
        val uri = Uri.parse(MediaArtworkContentProvider.buildUri(JSONObject("""{"image":{"path":"data:image/png;base64,AQID"}}""")))
        assertNull(provider.openFile(uri, "w"))
        provider.shutdown()
    }

    // Robolectric shares one cursor across pipe ends; the writer must finish before resetting it.
    private fun readPipe(provider: MediaArtworkContentProvider, uri: Uri): ByteArray {
        val previousExecutor = AsyncTask.THREAD_POOL_EXECUTOR
        ReflectionHelpers.setStaticField(AsyncTask::class.java, "THREAD_POOL_EXECUTOR", Executor { it.run() })
        val descriptor = try {
            checkNotNull(provider.openFile(uri, "r"))
        } finally {
            ReflectionHelpers.setStaticField(AsyncTask::class.java, "THREAD_POOL_EXECUTOR", previousExecutor)
        }
        return CompletableFuture.supplyAsync {
            ParcelFileDescriptor.AutoCloseInputStream(descriptor).use { input ->
                input.channel.position(0)
                input.readBytes()
            }
        }.get(5, TimeUnit.SECONDS)
    }
}