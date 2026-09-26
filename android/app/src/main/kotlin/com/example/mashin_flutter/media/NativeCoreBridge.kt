package com.example.mashin_flutter.media

import android.content.Context
import android.os.Handler
import android.os.Looper
import com.example.mashin_flutter.SecureSessionStore

object NativeCoreBridge {
    private val mainHandler = Handler(Looper.getMainLooper())

    // The Android media service is the sole event consumer and receives callbacks on the main thread.
    private var eventListener: ((String) -> Unit)? = null

    init {
        System.loadLibrary("mashin_engine")
    }

    external fun initialize(context: Context, secureSessionStore: Any)
    external fun restorePersistedSession(): Boolean
    external fun query(kind: String, argument: String?): String?
    external fun command(kind: String, argument: String?): Boolean

    // Secure storage must be initialized before the Rust core restores its persisted session.
    fun initializeCore(context: Context) {
        SecureSessionStore.initialize(context)
        initialize(context.applicationContext, SecureSessionStore)
    }

    // Event subscription lifecycle.
    fun observeEvents(listener: (String) -> Unit) {
        eventListener = listener
    }

    fun stopObservingEvents() {
        eventListener = null
    }

    // JNI callbacks may arrive off the UI thread; media-session state is confined to the main thread.
    @JvmStatic
    fun deliverCoreEvent(payload: String) {
        mainHandler.post { eventListener?.invoke(payload) }
    }
}