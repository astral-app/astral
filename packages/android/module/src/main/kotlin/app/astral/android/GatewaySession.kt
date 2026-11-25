package app.astral.android

import java.lang.reflect.Method

object GatewaySession {
    private const val INVALID_SESSION = "{\"op\":9,\"d\":false}"
    private const val GOING_AWAY = 1001

    private val lock = Any()
    private var socket: Any? = null
    private var listener: Any? = null
    private var onMessage: Method? = null
    private var uncompressed = false
    private var invalidateAfterHello = false

    fun opened(socket: Any, listener: Any, uncompressed: Boolean) = synchronized(lock) {
        this.socket = socket
        this.listener = listener
        this.onMessage = null
        this.uncompressed = uncompressed
    }

    fun afterMessage(from: Any?, method: Method) {
        if (from == null) return
        val target = synchronized(lock) {
            if (from !== socket) return
            onMessage = method
            if (!invalidateAfterHello || !uncompressed) return
            invalidateAfterHello = false
            listener
        } ?: return
        deliverInvalidSession(target, from, method)
    }

    fun dispatchLocal(message: String): Boolean {
        val (current, target, method, plain) = synchronized(lock) { Quad(socket, listener, onMessage, uncompressed) }
        if (current == null || target == null || method == null || !plain) return false
        return runCatching { method.invoke(target, current, message) }
            .onFailure { Log.warning("Failed to deliver a local gateway event: ${it.javaClass.simpleName}: ${it.message}") }
            .isSuccess
    }

    fun restart() {
        val staffTools = runCatching { Native.staffDevToolsEnabled() }.getOrDefault(false)
        val (current, target, method, plain) = synchronized(lock) { Quad(socket, listener, onMessage, uncompressed) }
        if (current == null) return
        if (plain && !staffTools && target != null && method != null) {
            deliverInvalidSession(target, current, method)
            return
        }
        synchronized(lock) { invalidateAfterHello = true }
        runCatching {
            current.javaClass.methods
                .single { it.parameterTypes.contentEquals(arrayOf(Int::class.javaPrimitiveType, String::class.java)) && it.returnType == Boolean::class.javaPrimitiveType }
                .invoke(current, GOING_AWAY, null)
            Log.info("Closed the gateway so it reconnects with the new settings")
        }.onFailure { Log.warning("Failed to close the gateway: ${it.javaClass.simpleName}: ${it.message}") }
    }

    private fun deliverInvalidSession(target: Any, from: Any, method: Method) {
        runCatching {
            method.invoke(target, from, INVALID_SESSION)
            Log.info("Asked Discord for a new gateway session")
        }.onFailure { Log.warning("Failed to ask Discord for a new gateway session: ${it.javaClass.simpleName}: ${it.message}") }
    }

    private data class Quad(val socket: Any?, val listener: Any?, val method: Method?, val uncompressed: Boolean)
}
