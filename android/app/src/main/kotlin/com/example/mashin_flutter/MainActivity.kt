package com.example.mashin_flutter

import android.content.Context
import android.os.Bundle
import android.util.Log
import io.flutter.embedding.android.FlutterActivity

class MainActivity : FlutterActivity() {
	private var nativeBridgeReady: Boolean = false

	companion object {
		private const val TAG = "MainActivity"
	}

	private external fun nativeSetAndroidContext(context: Context)

	override fun onCreate(savedInstanceState: Bundle?) {
		super.onCreate(savedInstanceState)

		nativeBridgeReady = try {
			System.loadLibrary("mashin_playback_engine")
			true
		} catch (error: UnsatisfiedLinkError) {
			Log.e(TAG, "Native bridge library not available", error)
			false
		}

		if (!nativeBridgeReady) {
			return
		}

		try {
			nativeSetAndroidContext(applicationContext)
		} catch (error: Throwable) {
			Log.e(TAG, "nativeSetAndroidContext failed", error)
		}
	}
}
