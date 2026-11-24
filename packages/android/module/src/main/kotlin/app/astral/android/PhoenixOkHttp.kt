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
import java.util.Locale
import java.util.concurrent.atomic.AtomicLong

object PhoenixOkHttp {

    private val blockedRequests = AtomicLong()
    @Volatile private var interceptor: Any? = null

    val blocked: Long
        get() = blockedRequests.get()

    fun install(module: XposedModule, loader: ClassLoader) {
        verifyShape(loader)
        val builderType = loader.loadClass(PhoenixClasses.OKHTTP_CLIENT_BUILDER)
        val clientType = loader.loadClass(PhoenixClasses.OKHTTP_CLIENT)
        val interceptorType = loader.loadClass(PhoenixClasses.OKHTTP_INTERCEPTOR)
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
        Log.info("Hooked Phoenix's shaded OkHttp client")
    }

    private fun intercept(chain: Any?, loader: ClassLoader): Any? {
        if (chain == null) return null
        try {
            val chainType = loader.loadClass(PhoenixClasses.OKHTTP_CHAIN)
            val requestType = loader.loadClass(PhoenixClasses.OKHTTP_REQUEST)
            val urlType = loader.loadClass(PhoenixClasses.OKHTTP_URL)
            val request = typedField(chainType, requestType).get(chain) ?: return null
            val requestUrl = typedField(requestType, urlType).get(request) ?: return proceed(chain, request, requestType)
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
            val method = typedField(requestType, String::class.java).get(request) as? String
            var outgoing = request
            if (staffTools && isGateway(host)) {
                outgoing = runCatching { removeGatewayCompression(outgoing, requestUrl) }
                    .onFailure { Log.error("Failed to remove gateway compression for Staff DevTools", it) }
                    .getOrDefault(outgoing)
            }
            val response = proceed(chain, outgoing, requestType) ?: return null
            if (!staffTools || method != "GET" || !Native.isCurrentUser(host, path)) return response
            return runCatching { markCurrentUser(response, loader) }
                .onFailure { Log.error("Failed to mark the current user as staff", it) }
                .getOrDefault(response)
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

    private fun removeGatewayCompression(request: Any, requestUrl: Any): Any {
        val address = requestUrl.toString()
        val uri = runCatching { URI(address) }.getOrNull() ?: return request
        val query = uri.rawQuery ?: return request
        val remaining = query.split('&').filterNot { parameter -> isCompression(parameter.substringBefore('=')) }
        if (remaining.size == query.split('&').size) return request
        val fields = instanceFields(requestUrl.javaClass)
        val addressField = fields.filter { it.type == String::class.java && it.get(requestUrl) == address }.singleOrNull()
            ?: throw NoSuchFieldException("${requestUrl.javaClass.name} has no single field holding its address")
        val queryField = fields.filter { field ->
            List::class.java.isAssignableFrom(field.type) && (field.get(requestUrl) as? List<*>)?.let(::hasCompression) == true
        }.singleOrNull() ?: throw NoSuchFieldException("${requestUrl.javaClass.name} has no single query list")
        val pairs = (queryField.get(requestUrl) as List<*>).chunked(2).filterNot { pair -> isCompression(pair[0] as? String ?: "") }
        queryField.set(requestUrl, ArrayList(pairs.flatten()))
        addressField.set(requestUrl, changedAddress(address, remaining))
        return request
    }

    private fun hasCompression(values: List<*>): Boolean = values.chunked(2).any { pair -> isCompression(pair[0] as? String ?: "") }

    private fun isCompression(name: String): Boolean =
        runCatching { URLDecoder.decode(name, StandardCharsets.UTF_8.name()) }.getOrDefault(name).equals("compress", ignoreCase = true)

    private fun changedAddress(address: String, remaining: List<String>): String {
        val withoutFragment = address.substringBefore('#')
        val fragment = address.substringAfter('#', "").takeIf { address.contains('#') }
        return buildString {
            append(withoutFragment.substringBefore('?'))
            if (remaining.isNotEmpty()) append('?').append(remaining.joinToString("&"))
            if (fragment != null) append('#').append(fragment)
        }
    }

    private fun instanceFields(type: Class<*>): List<Field> =
        generateSequence(type) { it.superclass }
            .takeWhile { it != Any::class.java }
            .flatMap { it.declaredFields.asSequence() }
            .filter { !Modifier.isStatic(it.modifiers) }
            .onEach { it.isAccessible = true }
            .toList()

    private fun noContent(request: Any, loader: ClassLoader): Any {
        val responseBuilderType = loader.loadClass(PhoenixClasses.OKHTTP_RESPONSE_BUILDER)
        val protocolType = loader.loadClass(PhoenixClasses.OKHTTP_PROTOCOL)
        val protocol = protocolType.enumConstants?.firstOrNull { value ->
            value.toString().equals("http/1.1", ignoreCase = true) || (value as? Enum<*>)?.name == "HTTP_1_1"
        } ?: throw NoSuchFieldException("Phoenix HTTP/1.1 protocol")
        val builder = responseBuilderType.getConstructor().newInstance()
        typedField(responseBuilderType, loader.loadClass(PhoenixClasses.OKHTTP_REQUEST)).set(builder, request)
        typedField(responseBuilderType, protocolType).set(builder, protocol)
        typedField(responseBuilderType, Int::class.javaPrimitiveType!!).setInt(builder, 204)
        typedField(responseBuilderType, String::class.java).set(builder, "No Content")
        typedField(responseBuilderType, responseBodyBase(loader)).set(builder, responseBody(loader, null, ""))
        return responseBuilderType.getMethod("a").invoke(builder) ?: throw IllegalStateException("Phoenix returned no response")
    }

    private fun markCurrentUser(response: Any, loader: ClassLoader): Any {
        val bodyType = responseBodyBase(loader)
        val body = typedField(response.javaClass, bodyType).get(response) ?: return response
        val content = body.javaClass.getMethod("n").invoke(body) as? String ?: return response
        val updated = Native.withStaffUser(content) ?: return response
        val contentType = body.javaClass.getMethod("i").invoke(body)?.toString()
        val builder = response.javaClass.getMethod("i").invoke(response) ?: return response
        val headers = field(builder.javaClass, "f").get(builder)
        val removeHeader = headers.javaClass.getMethod("g", String::class.java)
        removeHeader.invoke(headers, "Content-Encoding")
        removeHeader.invoke(headers, "Content-Length")
        typedField(builder.javaClass, bodyType).set(builder, responseBody(loader, contentType, updated))
        return builder.javaClass.getMethod("a").invoke(builder) ?: response
    }

    private fun responseBody(loader: ClassLoader, contentType: String?, content: String): Any {
        val bytes = content.toByteArray(StandardCharsets.UTF_8)
        val bufferType = loader.loadClass(PhoenixClasses.OKIO_BUFFER)
        val input = ByteArrayInputStream(bytes)
        val source = bufferType.getMethod("m", java.io.InputStream::class.java).invoke(null, input)
            ?: throw IllegalStateException("Phoenix returned no response source")
        val bufferedSourceFactory = bufferType.methods.first { method ->
            Modifier.isStatic(method.modifiers) && method.name == "d" && method.parameterCount == 1 && method.parameterTypes[0].isInstance(source)
        }
        val buffered = bufferedSourceFactory.invoke(null, source) ?: throw IllegalStateException("Phoenix returned no buffered source")
        val bodyType = loader.loadClass(PhoenixClasses.OKHTTP_RESPONSE_BODY)
        val sourceType = loader.loadClass(PhoenixClasses.OKIO_SOURCE)
        val constructor: Constructor<*> = bodyType.declaredConstructors.first { candidate ->
            val parameters = candidate.parameterTypes
            parameters.size == 4 && !parameters[0].isPrimitive && parameters[1] == Long::class.javaPrimitiveType &&
                parameters[2] == sourceType && parameters[3] == Int::class.javaPrimitiveType
        }.apply { isAccessible = true }
        return constructor.newInstance(contentType, bytes.size.toLong(), buffered, 1)
    }

    private fun responseBodyBase(loader: ClassLoader): Class<*> =
        loader.loadClass(PhoenixClasses.OKHTTP_RESPONSE_BODY).superclass ?: throw NoSuchFieldException("Phoenix's response body has no base class")

    private fun verifyShape(loader: ClassLoader) {
        val requestType = loader.loadClass(PhoenixClasses.OKHTTP_REQUEST)
        val chainType = loader.loadClass(PhoenixClasses.OKHTTP_CHAIN)
        val responseBuilderType = loader.loadClass(PhoenixClasses.OKHTTP_RESPONSE_BUILDER)
        typedField(chainType, requestType)
        typedField(requestType, loader.loadClass(PhoenixClasses.OKHTTP_URL))
        typedField(requestType, String::class.java)
        chainType.getMethod("b", requestType)
        typedField(responseBuilderType, requestType)
        typedField(responseBuilderType, loader.loadClass(PhoenixClasses.OKHTTP_PROTOCOL))
        typedField(responseBuilderType, Int::class.javaPrimitiveType!!)
        typedField(responseBuilderType, String::class.java)
        typedField(responseBuilderType, responseBodyBase(loader))
        responseBuilderType.getMethod("a")
        responseBody(loader, null, "")
    }

    private fun typedField(type: Class<*>, fieldType: Class<*>): Field {
        val matches = instanceFields(type).filter { it.type == fieldType }
        val match = matches.singleOrNull()
            ?: throw NoSuchFieldException(
                "${type.name} has ${matches.size} fields of type ${fieldType.name} (${matches.joinToString { it.declaringClass.name + "." + it.name }}); expected one",
            )
        return match.apply { isAccessible = true }
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
