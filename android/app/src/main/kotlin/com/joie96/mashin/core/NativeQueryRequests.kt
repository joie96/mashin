package com.joie96.mashin.core

import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.MoreExecutors
import com.google.common.util.concurrent.SettableFuture
import java.util.concurrent.atomic.AtomicLong
import org.json.JSONObject

/** Registers before native start; completion, cancellation and close each release ownership once. */
internal class NativeQueryRequests(
    private val startQuery: (Long, String, String?) -> Boolean = { id, kind, argument -> RustCoreBridge.startQuery(id, kind, argument) },
    private val cancelQuery: (Long) -> Boolean = { id -> RustCoreBridge.cancelQuery(id) },
) {
    private val pending = mutableMapOf<Long, SettableFuture<String>>()
    private var isClosed = false

    // IDs remain distinct across browser instances and from Flutter requests.
    companion object {
        private val sequence = AtomicLong(-1)
    }

    // Lifecycle.
    fun close() {
        synchronized(this) { isClosed = true }
        invalidate()
    }

    fun invalidate() {
        val requests = synchronized(this) { pending.values.toList() }
        requests.forEach { it.cancel(false) }
    }

    // Request dispatch and native cancellation.
    fun execute(kind: String, argument: String?): ListenableFuture<String> {
        val requestId = sequence.getAndDecrement()
        val result = SettableFuture.create<String>()
        var failure: Exception? = null
        synchronized(this) {
            check(!isClosed) { "Native query requests are closed" }
            pending[requestId] = result
            try {
                check(startQuery(requestId, kind, argument)) { "Native query was rejected" }
            } catch (error: Exception) {
                pending.remove(requestId)
                failure = error
            }
        }
        result.addListener({
            val shouldCancel = synchronized(this) { result.isCancelled && pending.remove(requestId) != null }
            if (shouldCancel) try { cancelQuery(requestId) } catch (_: Exception) { }
        }, MoreExecutors.directExecutor())
        failure?.let(result::setException)
        return result
    }

    // JNI completion events are never routed through Flutter's operation map.
    fun onCoreEvent(event: JSONObject) {
        val type = event.optString("type")
        if (type != "android_query_completed" && type != "android_query_failed") return
        val result = synchronized(this) { pending.remove(event.getLong("request_id")) } ?: return
        if (type == "android_query_completed") {
            result.set(event.get("result").toString())
        } else {
            result.setException(IllegalStateException(event.getJSONObject("error").getString("message")))
        }
    }
}