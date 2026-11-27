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
    @Volatile private var classLoader: ClassLoader? = null

    val blocked: Long
        get() = blockedRequests.get()

    fun install(module: XposedModule, loader: ClassLoader) {
        verifyShape(loader)
        classLoader = loader
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
            rememberDiscordTemplate(chain, request, requestType, host, path, loader)
            val method = typedField(requestType, String::class.java).get(request) as? String ?: "GET"
            val flags = NetworkExchange.plan(method, requestUrl.toString())
            if (flags and NetworkExchange.BLOCK != 0) {
                blockedRequests.incrementAndGet()
                Native.noteBlockedRequest()
                Log.info("Blocked telemetry request " + path)
                return noContent(request, loader)
            }
            var outgoing = request
            if (flags and NetworkExchange.PLAIN_GATEWAY != 0) {
                outgoing = runCatching { removeGatewayCompression(outgoing, requestUrl) }
                    .onFailure { Log.error("Failed to remove gateway compression", it) }
                    .getOrDefault(outgoing)
            }
            if (!NetworkExchange.needsExchange(flags)) return proceed(chain, outgoing, requestType)
            return exchange(chain, outgoing, requestType, method, loader)
        } catch (error: InvocationTargetException) {
            throw error.targetException
        }
    }

    private fun exchange(chain: Any, request: Any, requestType: Class<*>, method: String, loader: ClassLoader): Any? {
        val urlType = loader.loadClass(PhoenixClasses.OKHTTP_URL)
        val decision = runCatching {
            val url = typedField(requestType, urlType).get(request)?.toString() ?: throw NoSuchFieldException("Phoenix request has no URL")
            val headers = NetworkExchange.pairs(headerArray(requestHeaders(request, requestType)))
            val body = requestBody(request, requestType)?.let { bodyBytes(it, loader) } ?: ByteArray(0)
            NetworkExchange.begin(method, url, headers, body)
        }.onFailure { Log.error("Failed to read a Discord request for plugins", it) }
            .getOrNull() ?: return proceed(chain, request, requestType)
        decision.respond?.let { plain ->
            return runCatching { localResponse(request, loader, plain) }
                .onFailure { Log.error("Failed to answer a Discord request for a plugin", it) }
                .getOrElse { proceed(chain, request, requestType) }
        }
        val outgoing = decision.request?.let { changed ->
            runCatching { rebuiltRequest(request, requestType, urlType, changed, loader) }
                .onFailure { Log.error("Failed to rewrite a Discord request for a plugin", it) }
                .getOrDefault(request)
        } ?: request
        val response = try {
            proceed(chain, outgoing, requestType)
        } catch (error: Throwable) {
            NetworkExchange.abandon(decision.exchange)
            throw error
        }
        if (response == null) {
            NetworkExchange.abandon(decision.exchange)
            return null
        }
        if (decision.exchange == 0L) return response
        return runCatching { finishedResponse(response, decision.exchange, loader) }
            .onFailure {
                NetworkExchange.abandon(decision.exchange)
                Log.error("Failed to change a Discord response for a plugin", it)
            }
            .getOrDefault(response)
    }

    private fun rebuiltRequest(request: Any, requestType: Class<*>, urlType: Class<*>, changed: NetworkExchange.Plain, loader: ClassLoader): Any {
        val template = typedField(requestType, urlType).get(request) ?: throw NoSuchFieldException("Phoenix request has no URL")
        val body = String(changed.body, StandardCharsets.UTF_8).takeIf(String::isNotEmpty)
        return newRequest(template, requestType, urlType, changed.url, changed.method, changed.headerArray(), body, loader)
    }

    private fun finishedResponse(response: Any, exchange: Long, loader: ClassLoader): Any {
        val bodyType = responseBodyBase(loader)
        val body = typedField(response.javaClass, bodyType).get(response) ?: return response.also { NetworkExchange.abandon(exchange) }
        val content = (body.javaClass.getMethod("n").invoke(body) as? String ?: "").toByteArray(StandardCharsets.UTF_8)
        val contentType = body.javaClass.getMethod("i").invoke(body)?.toString()
        val status = typedField(response.javaClass, Int::class.javaPrimitiveType!!).getInt(response)
        val headers = instanceFields(response.javaClass).firstOrNull { field -> field.type.declaredConstructors.any { it.parameterTypes.contentEquals(arrayOf(Array<String>::class.java)) } }
            ?.get(response)?.let(::headerArray) ?: emptyArray()
        val changed = NetworkExchange.finish(exchange, status, NetworkExchange.pairs(headers), content)
            ?: return rebuiltResponse(response, loader, status, contentType, content)
        return rebuiltResponse(response, loader, changed.status, changed.header("content-type") ?: contentType, changed.body)
    }

    private fun requestHeadersField(requestType: Class<*>): Field =
        instanceFields(requestType).first { field -> field.type.declaredConstructors.any { it.parameterTypes.contentEquals(arrayOf(Array<String>::class.java)) } }

    private fun requestHeaders(request: Any, requestType: Class<*>): Any =
        requestHeadersField(requestType).get(request) ?: throw NoSuchFieldException("Phoenix's request has no headers")

    @Volatile private var discordTemplate: Any? = null
    @Volatile private var discordClient: Any? = null
    @Volatile private var discordCallType: Class<*>? = null
    private val discordExecutor = java.util.concurrent.Executors.newCachedThreadPool { task -> Thread(task, "astral-discord-request").apply { isDaemon = true } }
    private val droppedTemplateHeaders = setOf(
        "content-length", "content-type", "content-encoding", "x-context-properties", "x-audit-log-reason",
        "if-none-match", "if-modified-since", "range", "idempotency-key",
    )

    private fun rememberDiscordTemplate(chain: Any, request: Any, requestType: Class<*>, host: String, path: String, loader: ClassLoader) {
        if (host != "discord.com" || !path.startsWith("/api/v")) return
        runCatching {
            if (headerValue(headerArray(requestHeaders(request, requestType)), "authorization").isNullOrEmpty()) return
            val clientType = loader.loadClass(PhoenixClasses.OKHTTP_CLIENT)
            val call = instanceFields(chain.javaClass).asSequence().mapNotNull { it.get(chain) }.firstOrNull { value ->
                instanceFields(value.javaClass).any { it.type == clientType }
            } ?: return
            discordTemplate = request
            discordClient = instanceFields(call.javaClass).first { it.type == clientType }.get(call)
            discordCallType = call.javaClass
        }.onFailure { Log.error("Failed to remember Discord's API request shape", it) }
    }

    fun sendDiscordRequest(id: Long, method: String, path: String, body: String) {
        discordExecutor.execute {
            val loader = classLoader ?: return@execute Native.completeRequest(id, 0, false, "", "{}", "", null, "Discord has not called its API yet")
            val result = runCatching { executeDiscordRequest(method, path, body, loader) }
                .onFailure { Log.error("Failed to send Discord request $id", it) }
                .getOrNull()
            if (result == null) {
                Native.completeRequest(id, 0, false, "", "{}", "", null, "Discord has not called its API yet")
            } else {
                val (status, url, headers, text) = result
                Native.completeRequest(id, status, status in 200..299, url, headers, text, null, null)
            }
        }
    }

    private data class DiscordResult(val status: Int, val url: String, val headers: String, val body: String)

    private fun executeDiscordRequest(method: String, path: String, body: String, loader: ClassLoader): DiscordResult? {
        val template = discordTemplate ?: return null
        val client = discordClient ?: return null
        val requestType = loader.loadClass(PhoenixClasses.OKHTTP_REQUEST)
        val urlType = loader.loadClass(PhoenixClasses.OKHTTP_URL)
        val templateUrl = typedField(requestType, urlType).get(template)?.toString() ?: return null
        val base = templateUrl.substringBefore('?').let { address ->
            val start = address.indexOf("/api/v")
            if (start < 0) return null
            val end = address.indexOf('/', start + "/api/v".length).takeIf { it >= 0 } ?: address.length
            address.substring(0, end)
        }
        val address = base + path
        val headers = headerArray(requestHeaders(template, requestType)).toList().chunked(2)
            .filter { it.size == 2 && it[0].lowercase(Locale.ROOT) !in droppedTemplateHeaders }
            .flatten()
            .let { if (body.isNotEmpty()) it + listOf("Content-Type", "application/json") else it }
        val request = newRequest(typedField(requestType, urlType).get(template) ?: return null, requestType, urlType, address, method, headers.toTypedArray(), body.takeIf(String::isNotEmpty), loader)
        val clientType = loader.loadClass(PhoenixClasses.OKHTTP_CLIENT)
        val responseType = loader.loadClass(PhoenixClasses.OKHTTP_RESPONSE_BUILDER).getMethod("a").returnType
        val callType = discordCallType ?: return null
        val call = callType.declaredConstructors.first { candidate ->
            candidate.parameterTypes.contentEquals(arrayOf(clientType, requestType, java.lang.Boolean.TYPE))
        }.apply { isAccessible = true }.newInstance(client, request, false)
        val execute = callType.declaredMethods.filter { candidate ->
            !Modifier.isStatic(candidate.modifiers) && candidate.parameterCount == 0 && candidate.returnType == responseType
        }.minBy { it.name }.apply { isAccessible = true }
        val response = try {
            execute.invoke(call)
        } catch (error: InvocationTargetException) {
            throw error.targetException
        } ?: return null
        try {
            val status = typedField(responseType, Int::class.javaPrimitiveType!!).getInt(response)
            val responseBody = typedField(responseType, responseBodyBase(loader)).get(response)
            val text = responseBody?.javaClass?.getMethod("n")?.invoke(responseBody) as? String ?: ""
            val fields = org.json.JSONObject()
            instanceFields(responseType).firstOrNull { field -> field.type.declaredConstructors.any { it.parameterTypes.contentEquals(arrayOf(Array<String>::class.java)) } }
                ?.get(response)?.let(::headerArray)?.toList()?.chunked(2)?.filter { it.size == 2 }?.forEach { fields.put(it[0], it[1]) }
            return DiscordResult(status, address, fields.toString(), text)
        } finally {
            (response as? java.io.Closeable)?.close()
        }
    }

    private fun staticFactory(type: Class<*>, parameters: (Array<Class<*>>) -> Boolean, returns: (Class<*>) -> Boolean): Pair<Any?, java.lang.reflect.Method>? {
        type.declaredMethods.firstOrNull { Modifier.isStatic(it.modifiers) && parameters(it.parameterTypes) && returns(it.returnType) }
            ?.let { return null to it.apply { isAccessible = true } }
        val companion = type.declaredFields.firstOrNull { Modifier.isStatic(it.modifiers) && it.type.name.startsWith(type.name) && it.type != type }
            ?.apply { isAccessible = true }?.get(null) ?: return null
        return companion.javaClass.declaredMethods.firstOrNull { !Modifier.isStatic(it.modifiers) && parameters(it.parameterTypes) && returns(it.returnType) }
            ?.let { companion to it.apply { isAccessible = true } }
    }

    private fun newRequest(template: Any, requestType: Class<*>, urlType: Class<*>, address: String, method: String, headers: Array<String>, body: String?, loader: ClassLoader): Any {
        val url = urlLike(template, urlType, address)
        val headersType = requestHeadersField(requestType).type
        val headerObject = headersType.declaredConstructors.first { it.parameterTypes.contentEquals(arrayOf(Array<String>::class.java)) }
            .apply { isAccessible = true }.newInstance(headers as Any)
        val bodyType = bodyField(requestType).type
        val requestBody = body?.let { requestBody(bodyType, it) }
        val constructor = requestType.declaredConstructors.first { candidate ->
            val parameters = candidate.parameterTypes.toList()
            urlType in parameters && String::class.java in parameters && headersType in parameters && bodyType in parameters
        }.apply { isAccessible = true }
        val arguments = constructor.parameterTypes.map { parameter ->
            when {
                parameter == urlType -> url
                parameter == String::class.java -> method
                parameter == headersType -> headerObject
                parameter == bodyType -> requestBody
                Map::class.java.isAssignableFrom(parameter) -> java.util.LinkedHashMap<Any, Any>()
                else -> null
            }
        }
        return constructor.newInstance(*arguments.toTypedArray())
    }

    private fun decoded(part: String): String = URLDecoder.decode(part.replace("+", "%2B"), StandardCharsets.UTF_8.name())

    private fun urlLike(template: Any, urlType: Class<*>, address: String): Any {
        val uri = URI(address)
        val segments = ArrayList((uri.rawPath ?: "/").removePrefix("/").split('/').map(::decoded))
        val query = uri.rawQuery?.split('&')?.flatMap { pair ->
            listOf(decoded(pair.substringBefore('=')), pair.substringAfter('=', "").takeIf { pair.contains('=') }?.let(::decoded))
        }?.let { ArrayList(it) }
        val identity = instanceFields(urlType).filter { it.type == String::class.java }.take(4).map { it.get(template) }
        val port = typedField(urlType, Int::class.javaPrimitiveType!!).getInt(template)
        val constructor = urlType.declaredConstructors.first { candidate ->
            val parameters = candidate.parameterTypes
            parameters.count { it == Int::class.javaPrimitiveType } == 1 && parameters.count { it == java.util.ArrayList::class.java } == 2 &&
                parameters.count { it == String::class.java } == 6
        }.apply { isAccessible = true }
        var strings = 0
        var lists = 0
        val arguments = constructor.parameterTypes.map { parameter ->
            when (parameter) {
                Int::class.javaPrimitiveType -> port
                java.util.ArrayList::class.java -> if (lists++ == 0) segments else query
                else -> when (strings++) {
                    in 0..3 -> identity.getOrNull(strings - 1)
                    4 -> null
                    else -> address
                }
            }
        }
        return constructor.newInstance(*arguments.toTypedArray())
    }

    private fun requestBody(bodyType: Class<*>, content: String): Any {
        val (owner, factory) = staticFactory(bodyType, { parameters ->
            parameters.size == 2 && parameters.count { it == String::class.java } == 1 && parameters.none { it.isPrimitive }
        }, { bodyType.isAssignableFrom(it) }) ?: throw NoSuchMethodException("Phoenix's request body factory")
        val arguments = factory.parameterTypes.map { if (it == String::class.java) content else null }
        return factory.invoke(owner, *arguments.toTypedArray()) ?: throw IllegalStateException("Phoenix returned no request body")
    }

    private fun headerArray(headers: Any): Array<String> {
        val values = instanceFields(headers.javaClass).first { it.type == Array<String>::class.java }.get(headers) as Array<*>
        return values.map { it?.toString() ?: "" }.toTypedArray()
    }

    private fun bodyField(requestType: Class<*>): Field =
        instanceFields(requestType).first { field -> Modifier.isAbstract(field.type.modifiers) && !field.type.isInterface }

    private fun requestBody(request: Any, requestType: Class<*>): Any? = bodyField(requestType).get(request)

    private fun heldBytes(body: Any, depth: Int = 0): ByteArray? {
        val fields = instanceFields(body.javaClass)
        val bytes = fields.firstOrNull { it.type == ByteArray::class.java }?.get(body) as? ByteArray
            ?: return fields.firstOrNull { Modifier.isAbstract(it.type.modifiers) && !it.type.isInterface && it.type.isInstance(body) }
                ?.get(body)
                ?.takeIf { depth < MAX_BODY_WRAPPERS }
                ?.let { inner -> heldBytes(inner, depth + 1) }
        val numbers = fields.filter { it.type == Int::class.javaPrimitiveType }.map { it.getInt(body) }
        val (offset, count) = when (numbers.size) {
            2 -> numbers.sorted().let { (low, high) -> if (low + high <= bytes.size && low == 0) low to high else 0 to bytes.size }
            else -> 0 to bytes.size
        }
        return bytes.copyOfRange(offset, (offset + count).coerceAtMost(bytes.size))
    }

    private const val MAX_BODY_WRAPPERS = 4
    @Volatile private var bufferClass: Class<*>? = null

    private fun obfuscatedNames(): Sequence<String> {
        val letters = ('a'..'z').map(Char::toString)
        return letters.asSequence() + letters.asSequence().flatMap { first -> letters.asSequence().map { first + it } }
    }

    private fun okioBuffer(loader: ClassLoader): Class<*> {
        bufferClass?.let { return it }
        val packageName = PhoenixClasses.OKIO.substringBeforeLast('.')
        val found = obfuscatedNames()
            .mapNotNull { name -> runCatching { loader.loadClass("$packageName.$name") }.getOrNull() }
            .firstOrNull { type ->
                !type.isInterface && java.nio.channels.ByteChannel::class.java.isAssignableFrom(type) &&
                    type.declaredConstructors.any { it.parameterCount == 0 }
            } ?: throw ClassNotFoundException("Phoenix's okio Buffer in $packageName")
        bufferClass = found
        return found
    }

    private fun bodyBytes(body: Any, loader: ClassLoader): ByteArray {
        heldBytes(body)?.let { return it }
        val bufferType = runCatching { okioBuffer(loader) }.getOrElse {
            throw IllegalStateException(
                "unreadable ${body.javaClass.name} body (${instanceFields(body.javaClass).joinToString { it.type.name }})",
                it,
            )
        }
        val buffer = bufferType.getDeclaredConstructor().apply { isAccessible = true }.newInstance()
        val bodyType = bodyField(loader.loadClass(PhoenixClasses.OKHTTP_REQUEST)).type
        bodyType.declaredMethods.first { method ->
            Modifier.isAbstract(method.modifiers) && method.parameterCount == 1 && method.returnType == Void.TYPE && method.parameterTypes[0].isAssignableFrom(bufferType)
        }.invoke(body, buffer)
        val stream = buffer.javaClass.methods.first { it.parameterCount == 0 && it.returnType == java.io.InputStream::class.java }
            .invoke(buffer) as java.io.InputStream
        return stream.readBytes()
    }

    private fun headerValue(headers: Array<String>, name: String): String? =
        headers.toList().chunked(2).firstOrNull { it.size == 2 && it[0].equals(name, ignoreCase = true) }?.get(1)

    private fun rebuiltResponse(response: Any, loader: ClassLoader, status: Int, contentType: String?, body: ByteArray): Any {
        val bodyType = responseBodyBase(loader)
        val builder = response.javaClass.getMethod("i").invoke(response) ?: return response
        val headers = field(builder.javaClass, "f").get(builder)
        val removeHeader = headers.javaClass.getMethod("g", String::class.java)
        removeHeader.invoke(headers, "Content-Encoding")
        removeHeader.invoke(headers, "Content-Length")
        typedField(builder.javaClass, Int::class.javaPrimitiveType!!).setInt(builder, status)
        typedField(builder.javaClass, bodyType).set(builder, responseBody(loader, contentType, body))
        return builder.javaClass.getMethod("a").invoke(builder) ?: response
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

    private fun noContent(request: Any, loader: ClassLoader): Any = response(request, loader, 204, "No Content", null, ByteArray(0))

    private fun localResponse(request: Any, loader: ClassLoader, plain: NetworkExchange.Plain): Any =
        response(request, loader, plain.status, "OK", plain.header("content-type"), plain.body)

    private fun response(request: Any, loader: ClassLoader, status: Int, message: String, contentType: String?, body: ByteArray): Any {
        val responseBuilderType = loader.loadClass(PhoenixClasses.OKHTTP_RESPONSE_BUILDER)
        val protocolType = loader.loadClass(PhoenixClasses.OKHTTP_PROTOCOL)
        val protocol = protocolType.enumConstants?.firstOrNull { value ->
            value.toString().equals("http/1.1", ignoreCase = true) || (value as? Enum<*>)?.name == "HTTP_1_1"
        } ?: throw NoSuchFieldException("Phoenix HTTP/1.1 protocol")
        val builder = responseBuilderType.getConstructor().newInstance()
        typedField(responseBuilderType, loader.loadClass(PhoenixClasses.OKHTTP_REQUEST)).set(builder, request)
        typedField(responseBuilderType, protocolType).set(builder, protocol)
        typedField(responseBuilderType, Int::class.javaPrimitiveType!!).setInt(builder, status)
        typedField(responseBuilderType, String::class.java).set(builder, message)
        typedField(responseBuilderType, responseBodyBase(loader)).set(builder, responseBody(loader, contentType, body))
        return responseBuilderType.getMethod("a").invoke(builder) ?: throw IllegalStateException("Phoenix returned no response")
    }

    private fun responseBody(loader: ClassLoader, contentType: String?, content: String): Any =
        responseBody(loader, contentType, content.toByteArray(StandardCharsets.UTF_8))

    private fun responseBody(loader: ClassLoader, contentType: String?, bytes: ByteArray): Any {
        val bufferType = loader.loadClass(PhoenixClasses.OKIO)
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
