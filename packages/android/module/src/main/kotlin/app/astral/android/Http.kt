package app.astral.android

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import okhttp3.Call
import okhttp3.Callback
import okhttp3.CookieJar
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import org.json.JSONObject

object Http {
    private const val TIMEOUT_MS = 15_000
    private const val MAXIMUM_RESPONSE_BYTES = 1 shl 20
    private val requests = ConcurrentHashMap<Long, Call>()
    private val client =
        OkHttpClient.Builder()
            .followRedirects(false)
            .followSslRedirects(false)
            .cookieJar(CookieJar.NO_COOKIES)
            .cache(null)
            .build()

    fun get(url: String, token: String): ByteArray {
        var status = 0
        val body =
            try {
                val request = Request.Builder().url(url).apply { if (token.isNotEmpty()) header("Authorization", token) }.get().build()
                client.newBuilder().callTimeout(TIMEOUT_MS.toLong(), TimeUnit.MILLISECONDS).build().newCall(request).execute().use { response ->
                    status = response.code
                    response.body?.byteStream()?.use(::readBounded) ?: ByteArray(0)
                }
            } catch (error: Exception) {
                Log.warning("Failed to fetch Discord data: ${error.javaClass.simpleName}")
                ByteArray(0)
            }
        return ByteBuffer.allocate(Int.SIZE_BYTES + body.size).putInt(status).put(body).array()
    }

    fun request(id: Long, encoded: String) {
        val options = runCatching { JSONObject(encoded) }.getOrElse {
            completeLater(id, 0, false, "", JSONObject(), "", "invalid request options")
            return
        }
        val url = options.optString("url")
        val method = options.optString("method", "GET")
        val timeout = options.optInt("timeout_ms", TIMEOUT_MS).coerceIn(1_000, 30_000)
        val headers = options.optJSONObject("headers") ?: JSONObject()
        val body = options.optString("body", "")
        val requestBody =
            when (method) {
                "POST", "PUT", "PATCH" -> body.toRequestBody("application/octet-stream".toMediaType())
                "DELETE" -> if (options.has("body")) body.toRequestBody("application/octet-stream".toMediaType()) else null
                else -> null
            }
        val requestBuilder =
            runCatching {
                Request.Builder().url(url).method(method, requestBody).apply {
                    headers.keys().forEach { name -> header(name, headers.getString(name)) }
                }
            }.getOrElse {
                completeLater(id, 0, false, "", JSONObject(), "", "invalid request")
                return
            }
        val call = client.newBuilder().callTimeout(timeout.toLong(), TimeUnit.MILLISECONDS).build().newCall(requestBuilder.build())
        requests[id] = call
        call.enqueue(
            object : Callback {
                override fun onFailure(call: Call, e: java.io.IOException) {
                    requests.remove(id, call)
                    if (!call.isCanceled()) complete(id, 0, false, url, JSONObject(), "", "request failed")
                }

                override fun onResponse(call: Call, response: Response) {
                    response.use {
                        requests.remove(id, call)
                        val responseBody = response.body
                        val bytes = responseBody?.byteStream()?.use(::readBounded) ?: ByteArray(0)
                        val tooLarge = bytes.size > MAXIMUM_RESPONSE_BYTES
                        val resultHeaders = JSONObject()
                        response.headers.names().take(64).forEach { name ->
                            resultHeaders.put(name, response.headers.values(name).joinToString(", ").take(8_192))
                        }
                        val text = if (tooLarge) "" else textOf(bytes)
                        complete(
                            id,
                            response.code,
                            response.isSuccessful && !tooLarge,
                            response.request.url.toString(),
                            resultHeaders,
                            text ?: "",
                            if (tooLarge) "response body exceeds 1 MiB" else null,
                            if (tooLarge || text != null) null else android.util.Base64.encodeToString(bytes, android.util.Base64.NO_WRAP),
                        )
                    }
                }
            },
        )
    }

    fun cancel(id: Long) {
        requests.remove(id)?.cancel()
    }

    private fun textOf(bytes: ByteArray): String? =
        runCatching {
            Charsets.UTF_8.newDecoder()
                .onMalformedInput(java.nio.charset.CodingErrorAction.REPORT)
                .onUnmappableCharacter(java.nio.charset.CodingErrorAction.REPORT)
                .decode(java.nio.ByteBuffer.wrap(bytes))
                .toString()
        }.getOrNull()

    private fun readBounded(input: java.io.InputStream): ByteArray {
        val output = ByteArrayOutputStream()
        val buffer = ByteArray(16 * 1024)
        while (output.size() <= MAXIMUM_RESPONSE_BYTES) {
            val count = input.read(buffer, 0, minOf(buffer.size, MAXIMUM_RESPONSE_BYTES + 1 - output.size()))
            if (count < 0) break
            output.write(buffer, 0, count)
        }
        return output.toByteArray()
    }

    private fun complete(
        id: Long,
        status: Int,
        ok: Boolean,
        url: String,
        headers: JSONObject,
        body: String,
        error: String?,
        bodyBase64: String? = null,
    ) {
        Native.completeRequest(id, status, ok, url, headers.toString(), body, bodyBase64, error)
    }

    private fun completeLater(
        id: Long,
        status: Int,
        ok: Boolean,
        url: String,
        headers: JSONObject,
        body: String,
        error: String?,
    ) {
        client.dispatcher.executorService.execute { complete(id, status, ok, url, headers, body, error) }
    }
}
