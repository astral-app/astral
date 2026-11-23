package app.astral.android

import android.os.Handler
import android.os.HandlerThread
import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule
import java.lang.reflect.Method

object PhoenixBridge {
    private const val GUEST_HOST = "com.discord.nativeapp.bridge.GuestHost"
    private const val RETRY_INTERVAL_MS = 1000L

    private const val EVENT_FRAME = 1
    private const val NODE_OFFSET = 2
    private const val EVENT_OFFSET = 6
    private const val FUNCTION0 = "kotlin.jvm.functions.Function0"

    @Volatile var pressEvent = -1
    @Volatile private var guestHost: Any? = null
    private var applyOps: Method? = null
    private val delivering = ThreadLocal.withInitial { false }
    private val worker = HandlerThread("astral-wire").apply { start() }
    private val handler = Handler(worker.looper)

    fun install(module: XposedModule, loader: ClassLoader) {
        val type = loader.loadClass(GUEST_HOST)
        val method = type.getDeclaredMethod("applyOps", ByteArray::class.java, Int::class.javaPrimitiveType)
        method.isAccessible = true
        applyOps = method
        module.hook(method)
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                guestHost = chain.thisObject
                if (delivering.get() == true) return@intercept chain.proceed()
                val ops = chain.getArg(0) as ByteArray
                val length = chain.getArg(1) as Int
                Capture.record(ops, length)
                val rewritten = Native.rewrite(ops, length)
                if (rewritten != null) Capture.record(rewritten, rewritten.size)
                if (rewritten == null) chain.proceed() else chain.proceed(arrayOf<Any>(rewritten, rewritten.size))
            }
        hookEvents(module, type, loader)
        Native.attachBridge(PhoenixBridge::class.java)
        handler.postDelayed(::retry, RETRY_INTERVAL_MS)
        Log.info("Hooked GuestHost.applyOps")
    }

    private fun hookEvents(module: XposedModule, type: Class<*>, loader: ClassLoader) {
        val function0 = loader.loadClass(FUNCTION0)
        val enqueue = type.declaredMethods.firstOrNull {
            it.returnType == Void.TYPE && it.parameterTypes.contentEquals(arrayOf(ByteArray::class.java, function0))
        } ?: return Log.warning("GuestHost has no event method; astral's settings rows stay inert")
        module.hook(enqueue)
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                val event = chain.getArg(0) as? ByteArray
                if (event != null && handled(event)) {
                    chain.getArg(1)?.let { done -> handler.post { runCatching { done.javaClass.getMethod("invoke").invoke(done) } } }
                    null
                } else {
                    chain.proceed()
                }
            }
        Log.info("Hooked GuestHost.${enqueue.name} for events")
    }

    private fun handled(event: ByteArray): Boolean {
        if (pressEvent < 0 || event.size < EVENT_OFFSET + 2 || event[0].toInt() != EVENT_FRAME || event[1].toInt() != EVENT_FRAME) return false
        val buffer = java.nio.ByteBuffer.wrap(event).order(java.nio.ByteOrder.LITTLE_ENDIAN)
        if ((buffer.getShort(EVENT_OFFSET).toInt() and 0xFFFF) != pressEvent) return false
        return Native.press(buffer.getInt(NODE_OFFSET))
    }

    @JvmStatic
    fun fetch(url: String, token: String): String = Http.get(url, token)

    @JvmStatic
    fun deliver(ops: ByteArray) {
        handler.post { apply(ops) }
    }

    private fun apply(ops: ByteArray) {
        val host = guestHost ?: return
        val method = applyOps ?: return
        delivering.set(true)
        try {
            method.invoke(host, ops, ops.size)
        } catch (error: Throwable) {
            Log.error("Failed to deliver astral's ViewWire update", error)
        } finally {
            delivering.set(false)
        }
    }

    private fun retry() {
        try {
            if (guestHost != null) Native.pendingUpdates()?.let(::apply)
        } finally {
            handler.postDelayed(::retry, RETRY_INTERVAL_MS)
        }
    }
}
