package app.astral.android

import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule

object PhoenixBridge {
    private const val GUEST_HOST = "com.discord.nativeapp.bridge.GuestHost"

    fun install(module: XposedModule, loader: ClassLoader) {
        val guestHost = loader.loadClass(GUEST_HOST)
        val applyOps = guestHost.getDeclaredMethod("applyOps", ByteArray::class.java, Int::class.javaPrimitiveType)
        module.hook(applyOps)
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                val ops = chain.getArg(0) as ByteArray
                val length = chain.getArg(1) as Int
                val rewritten = Native.rewrite(ops, length)
                if (rewritten == null) chain.proceed() else chain.proceed(arrayOf<Any>(rewritten, rewritten.size))
            }
        Log.info("Hooked GuestHost.applyOps")
    }
}
