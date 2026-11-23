package app.astral.android

import android.os.Handler
import android.os.HandlerThread
import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule
import java.lang.reflect.Method

object PhoenixBridge {
    private const val GUEST_HOST = "com.discord.nativeapp.bridge.GuestHost"
    private const val RETRY_INTERVAL_MS = 1000L

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
                if (rewritten == null) chain.proceed() else chain.proceed(arrayOf<Any>(rewritten, rewritten.size))
            }
        Native.attachBridge(PhoenixBridge::class.java)
        handler.postDelayed(::retry, RETRY_INTERVAL_MS)
        Log.info("Hooked GuestHost.applyOps")
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
