package app.astral.android

import java.lang.reflect.InvocationTargetException
import java.lang.reflect.Modifier
import java.nio.charset.StandardCharsets

internal data class OutgoingMessageTransport(
    val requestBodyType: String,
    val bufferType: String,
    val newBuilderMethod: String,
    val buildMethod: String,
    val headerBuilderField: String?,
    val removeHeaderMethod: String,
)

internal object OutgoingMessageBody {
    val standard = OutgoingMessageTransport("okhttp3.RequestBody", "okio.Buffer", "newBuilder", "build", null, "removeHeader")
    val phoenix = OutgoingMessageTransport("g90.o0", "w90.h", "a", "c", "v", "g")

    fun transform(
        request: Any,
        loader: ClassLoader,
        transport: OutgoingMessageTransport,
        host: String,
        path: String,
        method: String,
    ): Any {
        return runCatching {
            val bodyType = loader.loadClass(transport.requestBodyType)
            val body = requestBody(request, bodyType) ?: return request
            val bufferType = loader.loadClass(transport.bufferType)
            val factory = bodyFactory(bodyType) ?: return request
            val mediaType = body.javaClass.methods.firstOrNull { candidate ->
                candidate.parameterCount == 0 && candidate.returnType == factory.parameterTypes[0]
            }?.invoke(body)
            val mediaTypeText = mediaType?.toString()?.lowercase().orEmpty()
            if (!mediaTypeText.contains("application/json")) return request
            if (body.javaClass.methods.any { candidate ->
                    candidate.parameterCount == 0 && candidate.returnType == Boolean::class.javaPrimitiveType &&
                        candidate.invoke(body) == true
                }) return request
            val contentLength = body.javaClass.methods.firstOrNull { candidate ->
                candidate.parameterCount == 0 && candidate.returnType == Long::class.javaPrimitiveType
            }?.invoke(body) as? Long ?: return request
            if (contentLength < 0 || contentLength > (1 shl 20)) return request
            val buffer = bufferType.getDeclaredConstructor().newInstance()
            val writeBody = body.javaClass.methods.firstOrNull { candidate ->
                candidate.parameterCount == 1 && candidate.returnType == Void.TYPE && candidate.parameterTypes[0].isInstance(buffer)
            } ?: return request
            invoke(writeBody, body, buffer)
            val bytes = bufferType.methods.firstOrNull { candidate ->
                candidate.parameterCount == 0 && candidate.returnType == ByteArray::class.java
            }?.invoke(buffer) as? ByteArray ?: return request
            val content = String(bytes, StandardCharsets.UTF_8)
            val changed = Native.transformOutgoingMessageBody(host, path, method, content) ?: return request
            val replacementBody = createBody(factory, mediaType, changed) ?: return request
            val builder = request.javaClass.getMethod(transport.newBuilderMethod).invoke(request) ?: return request
            val builderType = builder.javaClass
            val setMethod = builderType.methods.firstOrNull { candidate ->
                candidate.parameterCount == 2 && candidate.parameterTypes[0] == String::class.java &&
                    candidate.parameterTypes[1].isInstance(replacementBody)
            } ?: return request
            val headerTarget = transport.headerBuilderField?.let { field(builderType, it).get(builder) } ?: builder
            val removeHeader = headerTarget.javaClass.getMethod(transport.removeHeaderMethod, String::class.java)
            setMethod.invoke(builder, method, replacementBody)
            removeHeader.invoke(headerTarget, "Content-Length")
            removeHeader.invoke(headerTarget, "Transfer-Encoding")
            builderType.getMethod(transport.buildMethod).invoke(builder) ?: request
        }.getOrDefault(request)
    }

    private fun requestBody(request: Any, bodyType: Class<*>): Any? {
        var type: Class<*>? = request.javaClass
        while (type != null) {
            for (field in type.declaredFields) {
                if (!bodyType.isAssignableFrom(field.type)) continue
                field.isAccessible = true
                return field.get(request)
            }
            type = type.superclass
        }
        return request.javaClass.methods.firstOrNull { candidate ->
            candidate.parameterCount == 0 && bodyType.isAssignableFrom(candidate.returnType)
        }?.invoke(request)
    }

    private fun field(type: Class<*>, name: String): java.lang.reflect.Field {
        var current: Class<*>? = type
        while (current != null) {
            val match = current.declaredFields.firstOrNull { candidate -> candidate.name == name }
            if (match != null) return match.apply { isAccessible = true }
            current = current.superclass
        }
        throw NoSuchFieldException(type.name + "." + name)
    }

    private fun bodyFactory(bodyType: Class<*>): java.lang.reflect.Method? {
        return bodyType.methods
            .filter { candidate ->
                Modifier.isStatic(candidate.modifiers) && bodyType.isAssignableFrom(candidate.returnType) &&
                    candidate.parameterCount == 2 &&
                    (candidate.parameterTypes[1] == ByteArray::class.java || candidate.parameterTypes[1] == String::class.java) &&
                    candidate.parameterTypes[0] != String::class.java
            }
            .sortedBy { candidate -> if (candidate.parameterTypes[1] == ByteArray::class.java) 0 else 1 }
            .firstOrNull()
    }

    private fun createBody(factory: java.lang.reflect.Method, mediaType: Any?, content: String): Any? {
        val value: Any = if (factory.parameterTypes[1] == ByteArray::class.java) content.toByteArray(StandardCharsets.UTF_8) else content
        return factory.invoke(null, mediaType, value)
    }

    private fun invoke(method: java.lang.reflect.Method, receiver: Any, argument: Any): Any? {
        return try {
            method.invoke(receiver, argument)
        } catch (error: InvocationTargetException) {
            throw error.targetException
        }
    }
}
