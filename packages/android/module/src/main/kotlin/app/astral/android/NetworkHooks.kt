package app.astral.android

import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule
import java.lang.reflect.Method
import java.net.URI

object NetworkHooks {
    private const val ENGINE_BUILDER = "org.chromium.net.CronetEngine\$Builder"
    private const val URL_REQUEST_CALLBACK = "org.chromium.net.UrlRequest\$Callback"
    private const val AUTHORIZATION = "authorization"

    private val hookedEngines = mutableSetOf<Class<*>>()
    private val hookedBuilders = mutableSetOf<Class<*>>()
    @Volatile var blocked = 0
        private set

    fun install(module: XposedModule, loader: ClassLoader) {
        val builder = loader.loadClass(ENGINE_BUILDER)
        val build = builder.getDeclaredMethod("buildExperimental")
        module.hook(build)
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                val engine = chain.proceed()
                if (engine != null) runCatching { hookEngine(module, engine) }.onFailure { Log.error("Failed to hook Cronet", it) }
                engine
            }
        Log.info("Hooked CronetEngine.Builder.buildExperimental")
    }

    @Synchronized
    private fun hookEngine(module: XposedModule, engine: Any) {
        if (!hookedEngines.add(engine.javaClass)) return
        var type: Class<*>? = engine.javaClass
        while (type != null && type != Any::class.java) {
            for (method in type.declaredMethods) {
                val concrete = !java.lang.reflect.Modifier.isAbstract(method.modifiers)
                if (method.name == "newUrlRequestBuilder" && concrete && method.parameterTypes.firstOrNull() == String::class.java) {
                    hookNewRequest(module, method)
                    Log.info("Hooked ${type.name}.newUrlRequestBuilder(${method.parameterTypes.size})")
                }
            }
            type = type.superclass
        }
    }

    @Synchronized
    private fun hookBuilder(module: XposedModule, builder: Any) {
        if (!hookedBuilders.add(builder.javaClass)) return
        hookHeaders(module, builder.javaClass)
        Log.info("Hooked ${builder.javaClass.name}.addHeader")
    }

    private fun hookNewRequest(module: XposedModule, method: Method) {
        module.hook(method)
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                val url = chain.getArg(0) as? String
                val replacement = url?.let(::blockedReplacement)
                val builder = if (replacement == null) {
                    chain.proceed()
                } else {
                    blocked++
                    val args = chain.args.toTypedArray()
                    args[0] = replacement
                    chain.proceed(args)
                }
                if (builder != null) runCatching { hookBuilder(module, builder) }
                builder
            }
    }

    private fun blockedReplacement(url: String): String? {
        val uri = runCatching { URI(url) }.getOrNull() ?: return null
        val host = uri.host ?: return null
        if (!Native.isTelemetry(host, uri.rawPath ?: "/")) return null
        return Native.sinkUrl().takeIf(String::isNotEmpty)
    }

    @Volatile private var tokenSeen = false

    private fun captureToken(value: String) {
        Native.setSessionToken(value)
        if (!tokenSeen) {
            tokenSeen = true
            Log.info("Captured Discord's session from its API requests")
        }
    }

    private fun hookHeaders(module: XposedModule, type: Class<*>) {
        for (method in type.declaredMethods) {
            if (method.name != "addHeader" || method.parameterTypes.size != 2 || method.isBridge) continue
            module.hook(method)
                .setExceptionMode(ExceptionMode.PROTECTIVE)
                .intercept { chain ->
                    val name = chain.getArg(0) as? String
                    val value = chain.getArg(1) as? String
                    if (name != null && value != null && name.lowercase() == AUTHORIZATION) captureToken(value)
                    chain.proceed()
                }
        }
    }
}
