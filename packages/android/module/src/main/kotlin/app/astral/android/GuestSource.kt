package app.astral.android

import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule

object GuestSource {
    private const val RUNTIME = "com.discord.nativeapp.bridge.WasmRuntime"
    private const val LINKED = "nativeCreateLinked"
    private val creators = setOf(LINKED, "nativeCreate", "nativeCreateMapped", "nativeCreatePrecompiled")

    fun install(module: XposedModule, loader: ClassLoader) {
        val runtime = loader.loadClass(RUNTIME)
        val hooked = runtime.declaredMethods.filter { it.name in creators }
        check(hooked.isNotEmpty()) { "no guest creators on $RUNTIME" }
        for (method in hooked) {
            module.hook(method).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
                val source = if (method.name == LINKED) "bundled" else "${method.name}:${chain.args.drop(1).firstOrNull() as? String ?: "bytes"}"
                Log.info("Phoenix guest source: $source")
                Native.noteGuestSource(method.name == LINKED, source)
                chain.proceed()
            }
        }
    }
}
