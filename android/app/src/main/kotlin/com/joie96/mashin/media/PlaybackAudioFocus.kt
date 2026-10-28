package com.joie96.mashin.media

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Handler
import android.os.Looper

/** Service-owned focus; UI and remote playback never own local audio focus. */
internal class PlaybackAudioFocus(context: Context, private val onFocusLost: () -> Unit) {
    private val manager = context.getSystemService(AudioManager::class.java)
    private var hasAudioFocus = false
    private val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
        .setAudioAttributes(AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_MEDIA)
            .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
            .build())
        .setOnAudioFocusChangeListener({ change ->
            if (change == AudioManager.AUDIOFOCUS_LOSS || change == AudioManager.AUDIOFOCUS_LOSS_TRANSIENT) {
                hasAudioFocus = false
                onFocusLost()
            }
        }, Handler(Looper.getMainLooper()))
        .build()

    // Local playback focus lifecycle, confined to the main thread.
    fun acquire(): Boolean {
        if (!hasAudioFocus) {
            hasAudioFocus = manager.requestAudioFocus(request) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
        }
        return hasAudioFocus
    }

    fun release() {
        manager.abandonAudioFocusRequest(request)
        hasAudioFocus = false
    }
}