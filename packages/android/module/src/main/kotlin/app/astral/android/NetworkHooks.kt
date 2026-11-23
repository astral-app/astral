package app.astral.android

import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule
import java.lang.reflect.InvocationTargetException
import java.lang.reflect.Modifier
import java.lang.reflect.Proxy
import java.net.URI
import java.util.Locale
import java.util.concurrent.atomic.AtomicLong

object NetworkHooks {
    private const val ENGINE_BUILDER = "org.chromium.net.CronetEngine\$Builder"
    private const val INTERCEPTOR = "okhttp3.Interceptor"
    private const val CLIENT_BUILDER = "okhttp3.OkHttpClient\$Builder"
    private const val AUTHORIZATION = "authorization"

    private val hookedEngines = mutableSetOf<Class<*>>()
    private val hookedBuilders = mutableSetOf<Class<*>>()
    private val blockedRequests = AtomicLong()
    @Volatile private var interceptor: Any? = null

    val blocked: Long
        get() = blockedRequests.get()

    fun install(module: XposedModule, loader: ClassLoader) {
        runCatching { installCronet(module, loader) }.onFailure {
            Log.warning("Cronet telemetry hooks are unavailable: " + it.javaClass.simpleName)
        }
        runCatching { installOkHttp(module, loader) }.onFailure {
            Log.warning("OkHttp telemetry hooks are unavailable: " + it.javaClass.simpleName)
        }
    }

    private fun installCronet(module: XposedModule, loader: ClassLoader) {
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
                val concrete = !Modifier.isAbstract(method.modifiers)
                if (method.name == "newUrlRequestBuilder" && concrete && method.parameterTypes.firstOrNull() == String::class.java) {
                    hookNewRequest(module, method)
                    Log.info("Hooked " + type.name + ".newUrlRequestBuilder(" + method.parameterTypes.size + ")")
                }
            }
            type = type.superclass
        }
    }

    @Synchronized
    private fun hookBuilder(module: XposedModule, builder: Any) {
        if (!hookedBuilders.add(builder.javaClass)) return
        hookHeaders(module, builder.javaClass)
        Log.info("Hooked " + builder.javaClass.name + ".addHeader")
    }

    private fun hookNewRequest(module: XposedModule, method: java.lang.reflect.Method) {
        module.hook(method)
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                val url = chain.getArg(0) as? String
                val replacement = url?.let(::blockedReplacement)
                val builder = if (replacement == null) {
                    chain.proceed()
                } else {
                    recordBlocked(url)
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
        val host = uri.host?.lowercase(Locale.ROOT) ?: return null
        if (!Native.isTelemetry(host, uri.rawPath ?: "/")) return null
        return Native.sinkUrl().takeIf(String::isNotEmpty)
    }

    private fun recordBlocked(url: String) {
        blockedRequests.incrementAndGet()
        Native.noteBlockedRequest()
        val path = runCatching { URI(url).rawPath }.getOrNull() ?: url
        Log.info("Blocked telemetry request " + path)
    }

    private fun installOkHttp(module: XposedModule, loader: ClassLoader) {
        if (runCatching { loader.loadClass(CLIENT_BUILDER) }.isSuccess) {
            runCatching { installStandardOkHttp(module, loader) }.onFailure {
                Log.warning("Standard OkHttp hooks are unavailable: ${it.javaClass.simpleName}: ${it.message}")
            }
        }
        runCatching { PhoenixOkHttp.install(module, loader) }.onFailure {
            Log.warning("Phoenix OkHttp hooks are unavailable: ${it.javaClass.simpleName}: ${it.message}")
        }
        runCatching { installPhoenixWebSocketHooks(module, loader) }.onFailure {
            Log.warning("Phoenix WebSocket hooks are unavailable: ${it.javaClass.simpleName}: ${it.message}")
        }
    }

    private fun installStandardOkHttp(module: XposedModule, loader: ClassLoader) {
        val builder = loader.loadClass(CLIENT_BUILDER)
        val interceptorType = loader.loadClass(INTERCEPTOR)
        val proxy = Proxy.newProxyInstance(loader, arrayOf(interceptorType)) { instance, method, arguments ->
            when (method.name) {
                "intercept" -> interceptOkHttp(arguments?.getOrNull(0), loader)
                "equals" -> instance === arguments?.getOrNull(0)
                "hashCode" -> System.identityHashCode(instance)
                "toString" -> "astral telemetry interceptor"
                else -> null
            }
        }
        interceptor = proxy
        val addInterceptor = builder.getMethod("addInterceptor", interceptorType)
        val interceptors = builder.getMethod("interceptors")
        module.hook(builder.getDeclaredMethod("build"))
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                runCatching {
                    val configured = interceptors.invoke(chain.thisObject) as? Collection<*>
                    if (configured?.contains(proxy) != true) addInterceptor.invoke(chain.thisObject, proxy)
                }.onFailure { Log.error("Failed to add the OkHttp telemetry interceptor", it) }
                chain.proceed()
            }
        installWebSocketHooks(module, loader)
        Log.info("Hooked OkHttp client construction")
    }

    private fun installPhoenixWebSocketHooks(module: XposedModule, loader: ClassLoader) {
        val listener = loader.loadClass("com.discord.nativeapp.modules.u6")
        val methods = listener.declaredMethods.filter { method ->
            method.name == "m" && method.parameterTypes.lastOrNull() == String::class.java
        }
        for (method in methods) {
            val index = method.parameterTypes.indexOf(String::class.java)
            if (index < 0) continue
            module.hook(method)
                .setExceptionMode(ExceptionMode.PROTECTIVE)
                .intercept { chain ->
                    if (!runCatching { Native.staffDevToolsEnabled() }.getOrDefault(false)) return@intercept chain.proceed()
                    val message = chain.getArg(index) as? String ?: return@intercept chain.proceed()
                    val updated = Native.withStaffReady(message) ?: return@intercept chain.proceed()
                    chain.proceed(chain.args.toTypedArray().apply { this[index] = updated })
                }
        }
        Log.info("Hooked Phoenix WebSocket READY messages")
    }

    private fun interceptOkHttp(chain: Any?, loader: ClassLoader): Any? {
        if (chain == null) return null
        try {
            val request = chain.javaClass.getMethod("request").invoke(chain) ?: return null
            val requestType = loader.loadClass("okhttp3.Request")
            val url = requestType.getMethod("url").invoke(request) ?: return proceed(chain, request)
            val urlType = url.javaClass
            val host = (urlType.getMethod("host").invoke(url) as? String)?.lowercase(Locale.ROOT) ?: return proceed(chain, request)
            val path = urlType.getMethod("encodedPath").invoke(url) as? String ?: "/"
            if (Native.isTelemetry(host, path)) {
                recordBlocked(url.toString())
                return noContent(request, loader)
            }
            val staffTools = runCatching { Native.staffDevToolsEnabled() }.getOrDefault(false)
            val outgoing = if (staffTools && host.startsWith("gateway")) removeGatewayCompression(request, url, loader) else request
            val response = proceed(chain, outgoing) ?: return null
            val requestMethod = requestType.getMethod("method").invoke(request) as? String
            if (!staffTools || requestMethod != "GET" || !Native.isCurrentUser(host, path)) return response
            return markCurrentUser(response, loader)
        } catch (error: InvocationTargetException) {
            throw error.targetException
        }
    }

    private fun proceed(chain: Any, request: Any): Any? {
        val method = chain.javaClass.methods.firstOrNull { candidate ->
            candidate.name == "proceed" && candidate.parameterCount == 1 && candidate.parameterTypes[0].isInstance(request)
        } ?: return null
        return try {
            method.invoke(chain, request)
        } catch (error: InvocationTargetException) {
            throw error.targetException
        }
    }

    private fun removeGatewayCompression(request: Any, url: Any, loader: ClassLoader): Any {
        val urlBuilder = url.javaClass.getMethod("newBuilder").invoke(url)
        urlBuilder.javaClass.getMethod("removeAllQueryParameters", String::class.java).invoke(urlBuilder, "compress")
        val changedUrl = urlBuilder.javaClass.getMethod("build").invoke(urlBuilder)
        val requestType = loader.loadClass("okhttp3.Request")
        val requestBuilder = requestType.getMethod("newBuilder").invoke(request)
        val setUrl = requestBuilder.javaClass.methods.first { method ->
            method.name == "url" && method.parameterCount == 1 && method.parameterTypes[0].isInstance(changedUrl)
        }
        setUrl.invoke(requestBuilder, changedUrl)
        return requestBuilder.javaClass.getMethod("build").invoke(requestBuilder) ?: throw IllegalStateException("OkHttp returned no request")
    }

    private fun noContent(request: Any, loader: ClassLoader): Any {
        val responseBuilder = loader.loadClass("okhttp3.Response\$Builder")
        val requestType = loader.loadClass("okhttp3.Request")
        val protocolType = loader.loadClass("okhttp3.Protocol")
        val protocol = protocolType.getMethod("valueOf", String::class.java).invoke(null, "HTTP_1_1")
        val emptyBody = responseBody(loader, null, "")
        val builder = responseBuilder.getConstructor().newInstance()
        responseBuilder.getMethod("request", requestType).invoke(builder, request)
        responseBuilder.getMethod("protocol", protocolType).invoke(builder, protocol)
        responseBuilder.getMethod("code", Int::class.javaPrimitiveType).invoke(builder, 204)
        responseBuilder.getMethod("message", String::class.java).invoke(builder, "No Content")
        responseBuilder.getMethod("body", loader.loadClass("okhttp3.ResponseBody")).invoke(builder, emptyBody)
        return responseBuilder.getMethod("build").invoke(builder) ?: throw IllegalStateException("OkHttp returned no response")
    }

    private fun markCurrentUser(response: Any, loader: ClassLoader): Any {
        val responseType = loader.loadClass("okhttp3.Response")
        val body = responseType.getMethod("body").invoke(response) ?: return response
        val source = body.javaClass.getMethod("string").invoke(body) as? String ?: return response
        val changed = Native.withStaffUser(source) ?: return response
        val contentType = body.javaClass.getMethod("contentType").invoke(body)
        val replacement = responseBody(loader, contentType, changed)
        val builder = responseType.getMethod("newBuilder").invoke(response)
        builder.javaClass.getMethod("removeHeader", String::class.java).invoke(builder, "Content-Encoding")
        builder.javaClass.getMethod("removeHeader", String::class.java).invoke(builder, "Content-Length")
        builder.javaClass.getMethod("body", loader.loadClass("okhttp3.ResponseBody")).invoke(builder, replacement)
        return builder.javaClass.getMethod("build").invoke(builder) ?: throw IllegalStateException("OkHttp returned no response")
    }

    private fun responseBody(loader: ClassLoader, mediaType: Any?, content: String): Any {
        val mediaTypeType = loader.loadClass("okhttp3.MediaType")
        val bodyType = loader.loadClass("okhttp3.ResponseBody")
        val factory = bodyType.methods.firstOrNull { method ->
            Modifier.isStatic(method.modifiers) && method.name == "create" && method.parameterTypes.contentEquals(
                arrayOf(mediaTypeType, String::class.java),
            )
        } ?: throw NoSuchMethodException("OkHttp ResponseBody.create(MediaType, String)")
        return factory.invoke(null, mediaType, content) ?: throw IllegalStateException("OkHttp returned no response body")
    }

    private fun installWebSocketHooks(module: XposedModule, loader: ClassLoader) {
        val realWebSocket = runCatching { loader.loadClass("okhttp3.internal.ws.RealWebSocket") }.getOrNull()
        val methods = realWebSocket?.declaredMethods.orEmpty().filter { method ->
            method.name == "onReadMessage" && method.parameterTypes.contentEquals(arrayOf(String::class.java))
        }
        val listener = runCatching { loader.loadClass("okhttp3.WebSocketListener") }.getOrNull()
        val listenerMethods = listener?.declaredMethods.orEmpty().filter { method ->
            method.name == "onMessage" && method.parameterTypes.lastOrNull() == String::class.java && !Modifier.isAbstract(method.modifiers)
        }
        for (method in methods + listenerMethods) {
            module.hook(method)
                .setExceptionMode(ExceptionMode.PROTECTIVE)
                .intercept { chain ->
                    if (!runCatching { Native.staffDevToolsEnabled() }.getOrDefault(false)) return@intercept chain.proceed()
                    val index = method.parameterTypes.indexOf(String::class.java)
                    val message = chain.getArg(index) as? String ?: return@intercept chain.proceed()
                    val updated = Native.withStaffReady(message) ?: return@intercept chain.proceed()
                    chain.proceed(chain.args.toTypedArray().apply { this[index] = updated })
                }
        }
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
                    if (name != null && value != null && name.lowercase(Locale.ROOT) == AUTHORIZATION) captureToken(value)
                    chain.proceed()
                }
        }
    }
}
