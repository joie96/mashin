package com.example.mashin_flutter.core

import android.content.Context
import android.os.Handler
import android.os.Looper
import com.example.mashin_flutter.persistence.SecureSessionStore

object RustCoreBridge {
    private val mainHandler = Handler(Looper.getMainLooper())

    // The Android media service is the sole event consumer and receives callbacks on the main thread.
    private var eventListener: ((String) -> Unit)? = null

    init {
        System.loadLibrary("mashin_engine")
    }

    // Native session and Flutter-listener lifecycle.
    external fun initialize(context: Context, secureSessionStore: Any)

    // Secure storage must be initialized before the Rust core restores its persisted session.
    fun initializeCore(context: Context) {
        SecureSessionStore.initialize(context)
        initialize(context.applicationContext, SecureSessionStore)
    }

    external fun restorePersistedSession(): Boolean

    // Waits for native listener invocations before the Flutter engine can be destroyed.
    external fun unregisterFlutterEventCallback()

    // Event subscription lifecycle.
    fun observeEvents(listener: (String) -> Unit) {
        eventListener = listener
    }

    fun stopObservingEvents() {
        eventListener = null
    }

    // Native command boundary.
    external fun command(kind: String, argument: String?): Boolean

    // Native snapshot boundary.
    external fun query(kind: String, argument: String?): String?

    // JNI callbacks may arrive off the UI thread; media-session state is confined to the main thread.
    @JvmStatic
    fun deliverCoreEvent(payload: String) {
        mainHandler.post { eventListener?.invoke(payload) }
    }
}