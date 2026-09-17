package com.example.mashin_flutter.media

import android.content.Context
import android.os.Handler
import android.os.Looper
import com.example.mashin_flutter.SecureSessionStore

object NativeCoreBridge {
    private val mainHandler = Handler(Looper.getMainLooper())
    private var eventListener: ((String) -> Unit)? = null

    init {
        System.loadLibrary("mashin_playback_engine")
    }

    external fun initialize(context: Context, secureSessionStore: Any)
    external fun restorePersistedSession(): Boolean
    external fun query(kind: String, argument: String?): String?
    external fun command(kind: String, argument: String?): Boolean

    fun initializeCore(context: Context) {
        SecureSessionStore.initialize(context)
        initialize(context.applicationContext, SecureSessionStore)
    }

    fun observeEvents(listener: (String) -> Unit) {
        eventListener = listener
    }

    fun stopObservingEvents() {
        eventListener = null
    }

    @JvmStatic
    fun deliverCoreEvent(payload: String) {
        mainHandler.post { eventListener?.invoke(payload) }
    }
}