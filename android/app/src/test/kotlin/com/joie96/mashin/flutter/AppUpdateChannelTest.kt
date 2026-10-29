package com.joie96.mashin.flutter

import android.app.Activity
import android.content.Intent
import android.content.pm.PackageInfo
import android.net.Uri
import android.provider.Settings
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.FlutterException
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.StandardMethodCodec
import java.io.File
import java.nio.ByteBuffer
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class AppUpdateChannelTest {
    private class RecordingMessenger : BinaryMessenger {
        var handler: BinaryMessenger.BinaryMessageHandler? = null
        override fun send(channel: String, message: ByteBuffer?) = Unit
        override fun send(channel: String, message: ByteBuffer?, callback: BinaryMessenger.BinaryReply?) = Unit
        override fun setMessageHandler(channel: String, handler: BinaryMessenger.BinaryMessageHandler?) {
            this.handler = handler
        }
    }

    private fun call(messenger: RecordingMessenger, path: String): FlutterException {
        var response: ByteBuffer? = null
        val message = StandardMethodCodec.INSTANCE.encodeMethodCall(
            MethodCall("installApk", mapOf("path" to path)),
        )
        message.flip()
        messenger.handler!!.onMessage(message) { response = it }
        val envelope = checkNotNull(response)
        envelope.flip()
        return assertThrows(FlutterException::class.java) {
            StandardMethodCodec.INSTANCE.decodeEnvelope(envelope)
        }
    }

    @Test
    fun rejectsFilesOutsideUpdateCacheAndInvalidPackages() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val activity = controller.get()
        val messenger = RecordingMessenger()
        val channel = AppUpdateChannel(activity, messenger)
        val outside = File(activity.cacheDir, "other.apk").apply { writeText("invalid") }
        val inside = File(activity.cacheDir, "mashin-updates/installer-test/update.apk")
            .apply { parentFile!!.mkdirs(); writeText("invalid") }
        try {
            assertEquals("update_install_failed", call(messenger, outside.path).code)
            assertEquals("Ungültige Update-Datei.", call(messenger, outside.path).message)
            assertEquals("Die APK ist kein gültiges Mashin-Update.", call(messenger, inside.path).message)
            assertFalse(channel.onActivityResult(123))
        } finally {
            channel.disconnect()
            outside.delete()
            inside.parentFile!!.deleteRecursively()
            controller.pause().stop().destroy()
        }
    }

    @Test
    fun disconnectRemovesChannelHandler() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val messenger = RecordingMessenger()
        val channel = AppUpdateChannel(controller.get(), messenger)
        channel.disconnect()
        assertNull(messenger.handler)
        controller.pause().stop().destroy()
    }

    @Test
    fun grantedPermissionOpensApkWithReadPermission() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val activity = controller.get()
        val messenger = RecordingMessenger()
        val channel = installerChannel(activity, messenger)
        val file = prepareApk(activity)
        shadowOf(activity.packageManager).setCanRequestPackageInstalls(true)
        var response: ByteBuffer? = null
        try {
            sendInstall(messenger, file) { response = it }
            val envelope = checkNotNull(response)
            envelope.flip()
            assertNull(StandardMethodCodec.INSTANCE.decodeEnvelope(envelope))
            val intent = shadowOf(activity).nextStartedActivity
            assertEquals(Intent.ACTION_VIEW, intent.action)
            assertEquals("application/vnd.android.package-archive", intent.type)
            assertEquals("content", intent.data!!.scheme)
            assertEquals("${activity.packageName}.updates", intent.data!!.authority)
            assertTrue(intent.flags and Intent.FLAG_GRANT_READ_URI_PERMISSION != 0)
        } finally {
            channel.disconnect()
            file.parentFile!!.deleteRecursively()
            controller.pause().stop().destroy()
        }
    }

    @Test
    fun permissionReturnResumesPendingInstallerExactlyOnce() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val activity = controller.get()
        val messenger = RecordingMessenger()
        val channel = installerChannel(activity, messenger)
        val file = prepareApk(activity)
        shadowOf(activity.packageManager).setCanRequestPackageInstalls(false)
        var responseCount = 0
        var response: ByteBuffer? = null
        try {
            sendInstall(messenger, file) { responseCount++; response = it }
            assertNull(response)
            val permission = shadowOf(activity).nextStartedActivityForResult
            assertEquals(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, permission.intent.action)
            assertEquals(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, shadowOf(activity).nextStartedActivity.action)
            shadowOf(activity.packageManager).setCanRequestPackageInstalls(true)
            assertTrue(channel.onActivityResult(permission.requestCode))
            val envelope = checkNotNull(response)
            envelope.flip()
            assertNull(StandardMethodCodec.INSTANCE.decodeEnvelope(envelope))
            assertEquals(Intent.ACTION_VIEW, shadowOf(activity).nextStartedActivity.action)
            assertTrue(channel.onActivityResult(permission.requestCode))
            assertEquals(1, responseCount)
        } finally {
            channel.disconnect()
            file.parentFile!!.deleteRecursively()
            controller.pause().stop().destroy()
        }
    }

    @Test
    fun deniedPermissionReturnsActionableErrorWithoutOpeningInstaller() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val activity = controller.get()
        val messenger = RecordingMessenger()
        val channel = AppUpdateChannel(activity, messenger)
        val file = prepareApk(activity)
        shadowOf(activity.packageManager).setCanRequestPackageInstalls(false)
        var response: ByteBuffer? = null
        try {
            sendInstall(messenger, file) { response = it }
            val permission = shadowOf(activity).nextStartedActivityForResult
            assertEquals(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, shadowOf(activity).nextStartedActivity.action)
            channel.onActivityResult(permission.requestCode)
            val envelope = checkNotNull(response)
            envelope.flip()
            val error = assertThrows(FlutterException::class.java) {
                StandardMethodCodec.INSTANCE.decodeEnvelope(envelope)
            }
            assertEquals("update_install_failed", error.code)
            assertTrue(error.message!!.contains("Installation unbekannter Apps"))
            assertNull(shadowOf(activity).nextStartedActivity)
        } finally {
            channel.disconnect()
            file.parentFile!!.deleteRecursively()
            controller.pause().stop().destroy()
        }
    }

    // Archive metadata and Flutter wire envelopes stay at the test boundary.
    private fun installerChannel(activity: Activity, messenger: RecordingMessenger): AppUpdateChannel =
        AppUpdateChannel(activity, messenger) { file ->
            Uri.parse("content://${activity.packageName}.updates/updates/${file.name}")
        }

    private fun prepareApk(activity: Activity): File {
        val file = File(activity.cacheDir, "mashin-updates/installer-test/update.apk")
            .apply { parentFile!!.mkdirs(); writeText("test archive") }
        shadowOf(activity.packageManager).setPackageArchiveInfo(file.canonicalPath,
            PackageInfo().apply { packageName = activity.packageName })
        return file
    }

    private fun sendInstall(messenger: RecordingMessenger, file: File,
        reply: BinaryMessenger.BinaryReply) {
        val message = StandardMethodCodec.INSTANCE.encodeMethodCall(
            MethodCall("installApk", mapOf("path" to file.path)),
        )
        message.flip()
        messenger.handler!!.onMessage(message, reply)
    }
}