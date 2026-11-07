package com.joie96.mashin.media.artwork

import android.net.Uri
import android.os.AsyncTask
import android.os.ParcelFileDescriptor
import java.util.Base64
import java.util.concurrent.CompletableFuture
import java.util.concurrent.Executor
import java.util.concurrent.TimeUnit
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
    // Phase E supplies each request through a pipe without creating artwork files.
    @Test
    fun inlineArtworkReadsDoNotCreatePersistentFiles() {
        val provider = Robolectric.buildContentProvider(MediaArtworkContentProvider::class.java)
            .create().get()
        val directory = RuntimeEnvironment.getApplication().cacheDir
        val before = directory.walkTopDown().map { it.relativeTo(directory).path }.toSet()
        val svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"/>"
        val bytes = svg.encodeToByteArray()
        val sources = listOf(
            "data:image/svg+xml;base64,${Base64.getEncoder().encodeToString(bytes)}",
            "data:image/svg+xml,${Uri.encode(svg)}",
        )
        for (source in sources) {
            val uri = Uri.parse(MediaArtworkContentProvider.buildUri(source))
            val previousExecutor = AsyncTask.THREAD_POOL_EXECUTOR
            ReflectionHelpers.setStaticField(AsyncTask::class.java, "THREAD_POOL_EXECUTOR", Executor { it.run() })
            val descriptor = try {
                checkNotNull(provider.openFile(uri, "r"))
            } finally {
                ReflectionHelpers.setStaticField(AsyncTask::class.java, "THREAD_POOL_EXECUTOR", previousExecutor)
            }
            val received = CompletableFuture.supplyAsync {
                ParcelFileDescriptor.AutoCloseInputStream(descriptor).use { input ->
                    // Robolectric models both pipe ends with one temporary-file cursor.
                    input.channel.position(0)
                    input.readBytes()
                }
            }.get(5, TimeUnit.SECONDS)
            assertArrayEquals(bytes, received)
        }
        assertEquals(before, directory.walkTopDown().map { it.relativeTo(directory).path }.toSet())
        provider.shutdown()
    }

    @Test
    fun missingSourceAndWriteRequestsDoNotOpenArtwork() {
        val provider = Robolectric.buildContentProvider(MediaArtworkContentProvider::class.java)
            .create().get()
        assertNull(provider.openFile(Uri.parse("content://com.joie96.mashin.mediaart/art"), "r"))
        val uri = Uri.parse(MediaArtworkContentProvider.buildUri("data:image/png;base64,AQID"))
        assertNull(provider.openFile(uri, "w"))
        provider.shutdown()
    }
}