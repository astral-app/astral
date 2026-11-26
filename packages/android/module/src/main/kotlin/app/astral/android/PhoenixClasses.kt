package app.astral.android

import android.content.pm.ApplicationInfo
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.lang.reflect.Modifier

object PhoenixClasses {
    private const val CACHE = "files/astral/phoenix-classes.json"
    private const val CLIENT_ANCHOR = "Null interceptor: "
    private const val REQUEST_ANCHOR = "Request{method="
    private const val CHAIN_ANCHOR = "network interceptor "
    private const val RESPONSE_BUILDER_ANCHOR = "code < 0: "
    private const val PROTOCOL_ANCHOR = "h2_prior_knowledge"
    private val ANCHORS = listOf(CLIENT_ANCHOR, REQUEST_ANCHOR, CHAIN_ANCHOR, RESPONSE_BUILDER_ANCHOR, PROTOCOL_ANCHOR)

    @Volatile var OKHTTP_CLIENT = "g90.i0"
    @Volatile var OKHTTP_CLIENT_BUILDER = "g90.h0"
    @Volatile var OKHTTP_INTERCEPTOR = "g90.c0"
    @Volatile var OKHTTP_CHAIN = "m90.e"
    @Volatile var OKHTTP_REQUEST = "g90.k0"
    @Volatile var OKHTTP_URL = "g90.b0"
    @Volatile var OKHTTP_RESPONSE_BUILDER = "g90.p0"
    @Volatile var OKHTTP_RESPONSE_BODY = "g90.r0"
    @Volatile var OKHTTP_PROTOCOL = "g90.j0"
    @Volatile var OKIO = "w90.b"
    @Volatile var OKIO_SOURCE = "w90.j"

    fun resolve(application: ApplicationInfo, loader: ClassLoader) {
        val found = runCatching { anchored(application) }
            .onFailure { Log.warning("Failed to scan Discord's classes: ${it.message}") }
            .getOrNull() ?: return
        fun first(anchor: String, accept: (Class<*>) -> Boolean = { true }): Class<*>? {
            val names = found.optJSONArray(anchor) ?: return null
            return (0 until names.length()).asSequence()
                .mapNotNull { runCatching { loader.loadClass(names.getString(it)) }.getOrNull() }
                .firstOrNull(accept)
        }
        first(CLIENT_ANCHOR)?.let { client ->
            OKHTTP_CLIENT = client.name
            builderOf(client)?.let { builder ->
                OKHTTP_CLIENT_BUILDER = builder.name
                interceptorOf(builder)?.let { OKHTTP_INTERCEPTOR = it.name }
            }
        }
        first(REQUEST_ANCHOR)?.let { request ->
            OKHTTP_REQUEST = request.name
            urlOf(request)?.let { OKHTTP_URL = it.name }
        }
        first(CHAIN_ANCHOR)?.let { OKHTTP_CHAIN = it.name }
        first(RESPONSE_BUILDER_ANCHOR)?.let { OKHTTP_RESPONSE_BUILDER = it.name }
        first(PROTOCOL_ANCHOR) { it.isEnum }?.let { OKHTTP_PROTOCOL = it.name }
        Log.info("Found Phoenix's OkHttp classes: client $OKHTTP_CLIENT, builder $OKHTTP_CLIENT_BUILDER, interceptor $OKHTTP_INTERCEPTOR, request $OKHTTP_REQUEST, url $OKHTTP_URL, chain $OKHTTP_CHAIN, response builder $OKHTTP_RESPONSE_BUILDER, protocol $OKHTTP_PROTOCOL")
    }

    private fun anchored(application: ApplicationInfo): JSONObject {
        val apk = File(application.sourceDir)
        val key = "${apk.length()}:${apk.lastModified()}"
        val cache = File(application.dataDir, CACHE)
        runCatching { JSONObject(cache.readText()) }.getOrNull()
            ?.takeIf { it.optString("key") == key }
            ?.let { return it.getJSONObject("classes") }
        val classes = JSONObject(Native.anchoredClasses(apk.absolutePath, JSONArray(ANCHORS).toString()))
        runCatching { cache.writeText(JSONObject().put("key", key).put("classes", classes).toString()) }
            .onFailure { Log.warning("Failed to cache Discord's classes: ${it.message}") }
        return classes
    }

    private fun samePackage(type: Class<*>, anchor: Class<*>): Boolean = type.`package`?.name == anchor.`package`?.name

    private fun builderOf(client: Class<*>): Class<*>? = client.declaredConstructors
        .mapNotNull { it.parameterTypes.singleOrNull() }
        .firstOrNull { !it.isPrimitive && samePackage(it, client) }

    private fun interceptorOf(builder: Class<*>): Class<*>? = builder.declaredMethods
        .filter { it.returnType == builder && it.parameterTypes.size == 1 }
        .map { it.parameterTypes[0] }
        .firstOrNull { it.isInterface && samePackage(it, builder) }

    private fun urlOf(request: Class<*>): Class<*>? = request.declaredFields
        .filter { !Modifier.isStatic(it.modifiers) }
        .map { it.type }
        .firstOrNull { type ->
            samePackage(type, request) && !type.isInterface && !Modifier.isAbstract(type.modifiers) &&
                !Iterable::class.java.isAssignableFrom(type) && !type.isEnum
        }
}
