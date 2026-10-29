package com.joie96.mashin.flutter

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodChannel
import java.io.File

/** The activity owns permission requests; only the system installer installs packages. */
internal class AppUpdateChannel(
    private val activity: Activity,
    messenger: BinaryMessenger,
    private val createInstallerUri: (File) -> Uri = {
        FileProvider.getUriForFile(activity, "${activity.packageName}.updates", it)
    },
) {
    private val channel = MethodChannel(messenger, "mashin/updates")
    private var pendingFile: File? = null
    private var pendingResult: MethodChannel.Result? = null

    // Flutter-engine channel lifecycle.
    init {
        channel.setMethodCallHandler { call, result ->
            if (call.method != "installApk") {
                result.notImplemented()
            } else if (pendingResult != null) {
                result.error("update_busy", "Eine Installation ist bereits aktiv.", null)
            } else {
                try {
                    val path = requireNotNull(call.argument<String>("path"))
                    val file = validateApk(path)
                    if (requiresInstallPermission()) {
                        pendingFile = file
                        pendingResult = result
                        activity.startActivityForResult(
                            Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                                Uri.parse("package:${activity.packageName}")),
                            INSTALL_PERMISSION_REQUEST,
                        )
                    } else {
                        launchInstaller(file)
                        result.success(null)
                    }
                } catch (error: Exception) {
                    pendingFile = null
                    pendingResult = null
                    result.error("update_install_failed", error.message, null)
                }
            }
        }
    }

    fun disconnect() {
        pendingResult?.error("update_detached", "Die Installation wurde unterbrochen.", null)
        pendingResult = null
        pendingFile = null
        channel.setMethodCallHandler(null)
    }

    // Resume exactly one pending installation after the system permission screen.
    fun onActivityResult(requestCode: Int): Boolean {
        if (requestCode != INSTALL_PERMISSION_REQUEST) return false
        val result = pendingResult ?: return true
        val file = pendingFile
        pendingResult = null
        pendingFile = null
        try {
            check(!requiresInstallPermission()) {
                "Bitte Mashin die Installation unbekannter Apps erlauben und erneut versuchen."
            }
            launchInstaller(validateApk(requireNotNull(file).path))
            result.success(null)
        } catch (error: Exception) {
            result.error("update_install_failed", error.message, null)
        }
        return true
    }

    // Installer boundary: never share arbitrary cache files or another application's APK.
    private fun validateApk(path: String): File {
        val file = File(path).canonicalFile
        val updates = File(activity.cacheDir, "mashin-updates").canonicalFile
        require(file.path.startsWith(updates.path + File.separator) &&
            file.isFile && file.extension == "apk") { "Ungültige Update-Datei." }
        val archive = activity.packageManager.getPackageArchiveInfo(file.path, 0)
        require(archive?.packageName == activity.packageName) {
            "Die APK ist kein gültiges Mashin-Update."
        }
        return file
    }

    private fun requiresInstallPermission(): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
            !activity.packageManager.canRequestPackageInstalls()

    private fun launchInstaller(file: File) {
        val uri = createInstallerUri(file)
        activity.startActivity(Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            clipData = android.content.ClipData.newRawUri("Mashin update", uri)
        })
    }

    companion object {
        private const val INSTALL_PERMISSION_REQUEST = 8401
    }
}