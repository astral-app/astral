package app.astral.android

import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule
import java.io.ByteArrayInputStream
import java.lang.reflect.Constructor
import java.lang.reflect.Field
import java.lang.reflect.InvocationTargetException
import java.lang.reflect.Modifier
import java.net.URI
import java.net.URLDecoder
import java.nio.charset.StandardCharsets
import java.util.Collections
import java.util.IdentityHashMap
import java.util.Locale
import java.util.concurrent.atomic.AtomicLong

object PhoenixOkHttp {
    private const val CLIENT = "g90.i0"
    private const val CLIENT_BUILDER = "g90.h0"
    private const val INTERCEPTOR = "g90.c0"
    private const val CHAIN = "m90.e"
    private const val REQUEST = "g90.k0"
    private const val URL = "g90.b0"
    private const val RESPONSE_BUILDER = "g90.p0"
    private const val RESPONSE_BODY = "g90.r0"
    private const val BUFFER = "w90.b"
    private const val SOURCE = "w90.j"
    private const val URL_BUILDER = "com.discord.nativeapp.modules.s1"
    private const val DISCORD_CLIENTS = "com.discord.nativeapp.modules.za"

    private val blockedRequests = AtomicLong()
    private val patchedClients = Collections.newSetFromMap(IdentityHashMap<Any, Boolean>())
    @Volatile private var interceptor: Any? = null

    val blocked: Long
        get() = blockedRequests.get()

    fun install(module: XposedModule, loader: ClassLoader) {
        val builderType = loader.loadClass(CLIENT_BUILDER)
        val clientType = loader.loadClass(CLIENT)
        val interceptorType = loader.loadClass(INTERCEPTOR)
        val proxy = java.lang.reflect.Proxy.newProxyInstance(loader, arrayOf(interceptorType)) { instance, method, arguments ->
            when (method.name) {
                "a" -> intercept(arguments?.getOrNull(0), loader)
                "equals" -> instance === arguments?.getOrNull(0)
                "hashCode" -> System.identityHashCode(instance)
                "toString" -> "astral Phoenix network interceptor"
                else -> null
            }
        }
        interceptor = proxy
        val builderInterceptors = field(builderType, "c")
        val clientInterceptors = field(clientType, "i")
        val constructor = clientType.getDeclaredConstructor(builderType)
        module.hook(constructor)
            .setExceptionMode(ExceptionMode.PROTECTIVE)
            .intercept { chain ->
                runCatching {
                    val builder = chain.getArg(0) ?: return@runCatching
                    val configured = builderInterceptors.get(builder) as? Collection<*> ?: return@runCatching
                    if (configured.contains(proxy)) return@runCatching
                    val replacement = ArrayList<Any?>(configured.size + 1)
                    replacement.addAll(configured)
                    replacement.add(proxy)
                    builderInterceptors.set(builder, replacement)
                }.onFailure { Log.error("Failed to add the Phoenix network interceptor", it) }
                chain.proceed()
            }
        runCatching {
            val discordClients = loader.loadClass(DISCORD_CLIENTS)
            val primary = field(discordClients, "a").get(null) ?: return@runCatching
            patchClient(primary, clientInterceptors, proxy)
        }.onFailure { Log.warning("Phoenix's existing HTTP client could not be updated: " + it.javaClass.simpleName) }
        Log.info("Hooked Phoenix's shaded OkHttp client")
    }

    private fun patchClient(client: Any, interceptors: Field, proxy: Any) {
        synchronized(patchedClients) {
            if (!patchedClients.add(client)) return
        }
        runCatching {
            val configured = interceptors.get(client) as? Collection<*> ?: emptyList<Any?>()
            val replacement = ArrayList<Any?>(configured.size + 1)
            replacement.addAll(configured)
            if (!replacement.contains(proxy)) replacement.add(proxy)
            interceptors.set(client, replacement)
        }.onFailure {
            synchronized(patchedClients) { patchedClients.remove(client) }
            throw it
        }
    }

    private fun intercept(chain: Any?, loader: ClassLoader): Any? {
        if (chain == null) return null
        try {
            val chainType = loader.loadClass(CHAIN)
            val requestType = loader.loadClass(REQUEST)
            val request = field(chainType, "e").get(chain) ?: return null
            val requestUrl = field(requestType, "a").get(request) ?: return proceed(chain, request, requestType)
            val uri = runCatching { URI(requestUrl.toString()) }.getOrNull() ?: return proceed(chain, request, requestType)
            val host = uri.host?.lowercase(Locale.ROOT) ?: return proceed(chain, request, requestType)
            val path = uri.rawPath ?: "/"
            if (Native.isTelemetry(host, path)) {
                val address = requestUrl.toString()
                blockedRequests.incrementAndGet()
                Native.noteBlockedRequest()
                Log.info("Blocked telemetry request " + path)
                return noContent(request, loader)
            }
            val staffTools = runCatching { Native.staffDevToolsEnabled() }.getOrDefault(false)
            val outgoing = if (staffTools && isGateway(host)) removeGatewayCompression(request, requestUrl, loader) else request
            val response = proceed(chain, outgoing, requestType) ?: return null
            val method = field(requestType, "b").get(request) as? String
            if (!staffTools || method != "GET" || !Native.isCurrentUser(host, path)) return response
            return markCurrentUser(response, loader)
        } catch (error: InvocationTargetException) {
            throw error.targetException
        }
    }

    private fun proceed(chain: Any, request: Any, requestType: Class<*>): Any? {
        val chainType = chain.javaClass
        val method = chainType.getMethod("b", requestType)
        return try {
            method.invoke(chain, request)
        } catch (error: InvocationTargetException) {
            throw error.targetException
        }
    }

    private fun isGateway(host: String): Boolean = host.startsWith("gateway") && host.endsWith(".discord.gg")

    private fun removeGatewayCompression(request: Any, requestUrl: Any, loader: ClassLoader): Any {
        val address = requestUrl.toString()
        val uri = runCatching { URI(address) }.getOrNull() ?: return request
        val query = uri.rawQuery ?: return request
        val remaining = query.split('&').filterNot { parameter ->
            val name = parameter.substringBefore('=')
            runCatching { URLDecoder.decode(name, StandardCharsets.UTF_8.name()) }
                .getOrDefault(name)
                .equals("compress", ignoreCase = true)
        }
        if (remaining.size == query.split('&').size) return request
        val withoutFragment = address.substringBefore('#')
        val fragment = address.substringAfter('#', "").takeIf { address.contains('#') }
        val base = withoutFragment.substringBefore('?')
        val changedAddress = buildString {
            append(base)
            if (remaining.isNotEmpty()) append('?').append(remaining.joinToString("&"))
            if (fragment != null) append('#').append(fragment)
        }
        val urlType = loader.loadClass(URL)
        val parser = loader.loadClass(URL_BUILDER).getConstructor(Int::class.javaPrimitiveType).newInstance(3)
        val parse = parser.javaClass.getMethod("i", urlType, String::class.java)
        parse.invoke(parser, requestUrl, changedAddress)
        val changedUrl = parser.javaClass.getMethod("b").invoke(parser) ?: return request
        val requestBuilder = loader.loadClass(REQUEST).getMethod("a").invoke(request) ?: return request
        field(requestBuilder.javaClass, "e").set(requestBuilder, changedUrl)
        return requestBuilder.javaClass.getMethod("c").invoke(requestBuilder) ?: request
    }

    private fun noContent(request: Any, loader: ClassLoader): Any {
        val responseBuilderType = loader.loadClass(RESPONSE_BUILDER)
        val protocolType = loader.loadClass("g90.j0")
        val protocol = protocolType.enumConstants?.firstOrNull { value ->
            value.toString().equals("http/1.1", ignoreCase = true) || (value as? Enum<*>)?.name == "HTTP_1_1"
        } ?: throw NoSuchFieldException("Phoenix HTTP/1.1 protocol")
        val builder = responseBuilderType.getConstructor().newInstance()
        field(responseBuilderType, "a").set(builder, request)
        field(responseBuilderType, "b").set(builder, protocol)
        field(responseBuilderType, "c").setInt(builder, 204)
        field(responseBuilderType, "d").set(builder, "No Content")
        field(responseBuilderType, "g").set(builder, responseBody(loader, null, ""))
        return responseBuilderType.getMethod("a").invoke(builder) ?: throw IllegalStateException("Phoenix returned no response")
    }

    private fun markCurrentUser(response: Any, loader: ClassLoader): Any {
        val body = field(response.javaClass, "y").get(response) ?: return response
        val content = body.javaClass.getMethod("n").invoke(body) as? String ?: return response
        val updated = Native.withStaffUser(content) ?: return response
        val contentType = body.javaClass.getMethod("i").invoke(body)?.toString()
        val builder = response.javaClass.getMethod("i").invoke(response) ?: return response
        val headers = field(builder.javaClass, "f").get(builder)
        val removeHeader = headers.javaClass.getMethod("g", String::class.java)
        removeHeader.invoke(headers, "Content-Encoding")
        removeHeader.invoke(headers, "Content-Length")
        field(builder.javaClass, "g").set(builder, responseBody(loader, contentType, updated))
        return builder.javaClass.getMethod("a").invoke(builder) ?: response
    }

    private fun responseBody(loader: ClassLoader, contentType: String?, content: String): Any {
        val bytes = content.toByteArray(StandardCharsets.UTF_8)
        val bufferType = loader.loadClass(BUFFER)
        val input = ByteArrayInputStream(bytes)
        val source = bufferType.getMethod("m", java.io.InputStream::class.java).invoke(null, input)
            ?: throw IllegalStateException("Phoenix returned no response source")
        val bufferedSourceFactory = bufferType.methods.first { method ->
            Modifier.isStatic(method.modifiers) && method.name == "d" && method.parameterCount == 1 && method.parameterTypes[0].isInstance(source)
        }
        val buffered = bufferedSourceFactory.invoke(null, source) ?: throw IllegalStateException("Phoenix returned no buffered source")
        val bodyType = loader.loadClass(RESPONSE_BODY)
        val sourceType = loader.loadClass(SOURCE)
        val constructor: Constructor<*> = bodyType.declaredConstructors.first { candidate ->
            val parameters = candidate.parameterTypes
            parameters.size == 4 && !parameters[0].isPrimitive && parameters[1] == Long::class.javaPrimitiveType &&
                parameters[2] == sourceType && parameters[3] == Int::class.javaPrimitiveType
        }.apply { isAccessible = true }
        return constructor.newInstance(contentType, bytes.size.toLong(), buffered, 1)
    }

    private fun field(type: Class<*>, name: String): Field {
        var current: Class<*>? = type
        while (current != null) {
            val result = current.declaredFields.firstOrNull { it.name == name }
            if (result != null) return result.apply { isAccessible = true }
            current = current.superclass
        }
        throw NoSuchFieldException(type.name + "." + name)
    }
}
