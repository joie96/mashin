package com.example.mashin_flutter

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import android.os.Build
import android.util.Log
import android.content.Intent
import com.example.mashin_flutter.media.MashinMediaLibraryService
import com.example.mashin_flutter.media.NativeCoreBridge
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
	private var nativeBridgeReady: Boolean = false

	companion object {
		private const val TAG = "MainActivity"
		private const val NOTIFICATION_PERMISSION_REQUEST_CODE = 1
	}

		// Activity lifecycle and media-service startup.
	override fun onCreate(savedInstanceState: Bundle?) {
		super.onCreate(savedInstanceState)
		nativeBridgeReady = try {
			System.loadLibrary("mashin_engine")
			true
		} catch (error: UnsatisfiedLinkError) {
			Log.e(TAG, "Native bridge library not available", error)
			false
		}

		if (!nativeBridgeReady) {
			return
		}

		try {
			NativeCoreBridge.initializeCore(applicationContext)
		} catch (error: Throwable) {
			Log.e(TAG, "initialize native core failed", error)
		}
		if (hasNotificationPermission()) {
			Log.d(TAG, "Notification permission granted; starting media service")
			startMediaLibraryService()
		} else {
			Log.w(TAG, "Notification permission missing; requesting it before starting media service")
			requestNotificationPermission()
		}
	}

	// Notification permission handling.
	private fun requestNotificationPermission() {
		if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
			requestPermissions(
				arrayOf(Manifest.permission.POST_NOTIFICATIONS),
				NOTIFICATION_PERMISSION_REQUEST_CODE,
			)
		}
	}

	private fun hasNotificationPermission(): Boolean =
		Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
			checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

	override fun onRequestPermissionsResult(
		requestCode: Int,
		permissions: Array<out String>,
		grantResults: IntArray,
	) {
		super.onRequestPermissionsResult(requestCode, permissions, grantResults)
		if (requestCode == NOTIFICATION_PERMISSION_REQUEST_CODE && hasNotificationPermission()) {
			Log.d(TAG, "Notification permission granted; starting media service")
			startMediaLibraryService()
		} else if (requestCode == NOTIFICATION_PERMISSION_REQUEST_CODE) {
			Log.w(TAG, "Notification permission denied; media service will not start")
		}
	}

	// A deliberate app close ends playback, whereas minimizing keeps the service alive.
	override fun onDestroy() {
		if (isFinishing) {
			Log.d(TAG, "Activity is finishing; stopping media service")
			stopService(Intent(applicationContext, MashinMediaLibraryService::class.java))
		}
		super.onDestroy()
	}

	// Flutter platform integration.
	override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
		super.configureFlutterEngine(flutterEngine)
		MethodChannel(
			flutterEngine.dartExecutor.binaryMessenger,
			"mashin_flutter/device_info",
		).setMethodCallHandler { call, result ->
			when (call.method) {
				"getDeviceName" -> {
					val manufacturer = Build.MANUFACTURER?.trim().orEmpty()
					val model = Build.MODEL?.trim().orEmpty()
					val deviceName = when {
						manufacturer.isEmpty() -> model
						model.isEmpty() -> manufacturer
						model.startsWith(manufacturer, ignoreCase = true) -> model
						else -> "$manufacturer $model"
					}
					result.success(deviceName.ifEmpty { null })
				}
				else -> result.notImplemented()
			}
		}

	}

	// The visible activity may start the service; Media3 promotes it for active playback.
	private fun startMediaLibraryService() {
		Log.d(TAG, "Starting media library service")
		startService(Intent(applicationContext, MashinMediaLibraryService::class.java))
	}
}
