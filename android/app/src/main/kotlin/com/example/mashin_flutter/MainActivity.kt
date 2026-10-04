package com.example.mashin_flutter

import androidx.media3.common.util.UnstableApi
import com.example.mashin_flutter.core.RustCoreBridge
import com.example.mashin_flutter.flutter.DeviceInfoChannel
import com.example.mashin_flutter.flutter.FlutterPlaybackChannel
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine

@UnstableApi
class MainActivity : FlutterActivity() {
    private var playbackChannel: FlutterPlaybackChannel? = null
    private var deviceInfoChannel: DeviceInfoChannel? = null

    // Activity hosts UI channels; the playback service owns the native session.
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        playbackChannel = FlutterPlaybackChannel(applicationContext, flutterEngine.dartExecutor.binaryMessenger)
        deviceInfoChannel = DeviceInfoChannel(flutterEngine.dartExecutor.binaryMessenger)
    }

    // Revoke the FFI listener before engine destruction or service disconnect can emit native events.
    override fun cleanUpFlutterEngine(flutterEngine: FlutterEngine) {
        RustCoreBridge.unregisterFlutterEventCallback()
        playbackChannel?.disconnect()
        playbackChannel = null
        deviceInfoChannel?.disconnect()
        deviceInfoChannel = null
        super.cleanUpFlutterEngine(flutterEngine)
    }
}