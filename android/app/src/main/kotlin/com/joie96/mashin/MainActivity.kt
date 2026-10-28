package com.joie96.mashin

import android.os.Bundle
import androidx.media3.common.util.UnstableApi
import com.joie96.mashin.core.RustCoreBridge
import com.joie96.mashin.flutter.DeviceInfoChannel
import com.joie96.mashin.flutter.FlutterPlaybackChannel
import com.joie96.mashin.media.PlaybackSessionLifecycle
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine

@UnstableApi
class MainActivity : FlutterActivity() {
    private var playbackChannel: FlutterPlaybackChannel? = null
    private var deviceInfoChannel: DeviceInfoChannel? = null

    // Activity existence owns playback even while the UI is minimized.
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        PlaybackSessionLifecycle.shared.attachActivity(this)
    }

    override fun onDestroy() {
        super.onDestroy()
        PlaybackSessionLifecycle.shared.detachActivity(this, isChangingConfigurations)
    }

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