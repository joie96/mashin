package com.joie96.mashin.flutter

import android.os.Build
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodChannel

internal class DeviceInfoChannel(messenger: BinaryMessenger) {
    private val channel = MethodChannel(messenger, "mashin/device_info")

    // Flutter-engine channel lifecycle.
    init {
        channel.setMethodCallHandler { call, result ->
            if (call.method == "getDeviceName") result.success(deviceName())
            else result.notImplemented()
        }
    }

    fun disconnect() {
        channel.setMethodCallHandler(null)
    }

    // Device identity projection.
    private fun deviceName(): String? {
        val manufacturer = Build.MANUFACTURER?.trim().orEmpty()
        val model = Build.MODEL?.trim().orEmpty()
        return when {
            manufacturer.isEmpty() -> model
            model.isEmpty() -> manufacturer
            model.startsWith(manufacturer, ignoreCase = true) -> model
            else -> "$manufacturer $model"
        }.ifEmpty { null }
    }
}