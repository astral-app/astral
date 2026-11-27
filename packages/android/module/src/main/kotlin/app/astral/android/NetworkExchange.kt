package app.astral.android

import java.util.Base64
import org.json.JSONArray
import org.json.JSONObject

object NetworkExchange {
    const val BLOCK = 1
    const val MARK_STAFF = 1 shl 1
    const val INTERCEPT = 1 shl 2
    const val PLAIN_GATEWAY = 1 shl 5
    const val OKHTTP_STAGES = BLOCK or MARK_STAFF or INTERCEPT
    private const val NO_EXCHANGE = 0L
    private const val OK = 200

    class Plain(val method: String, val url: String, val status: Int, val headers: List<Pair<String, String>>, val body: ByteArray) {
        fun header(name: String): String? = headers.firstOrNull { it.first.equals(name, ignoreCase = true) }?.second
        fun headerArray(): Array<String> = headers.flatMap { listOf(it.first, it.second) }.toTypedArray()
    }

    class Decision(val block: Boolean, val respond: Plain?, val request: Plain?, val exchange: Long)

    fun plan(method: String, url: String): Int = runCatching { Native.networkPlan(method, url) }.getOrDefault(0)

    fun needsExchange(flags: Int): Boolean = flags and (MARK_STAFF or INTERCEPT) != 0

    fun pairs(headers: Array<String>): List<Pair<String, String>> =
        headers.toList().chunked(2).filter { it.size == 2 }.map { it[0] to it[1] }

    fun begin(method: String, url: String, headers: List<Pair<String, String>>, body: ByteArray, stages: Int = OKHTTP_STAGES): Decision {
        val request = JSONObject()
            .put("method", method)
            .put("url", url)
            .put("headers", headersJson(headers))
            .put("body_base64", Base64.getEncoder().encodeToString(body))
        val decision = JSONObject(Native.networkBegin(request.toString(), stages))
        return Decision(
            block = decision.optBoolean("block"),
            respond = decision.optJSONObject("respond")?.let(::plain),
            request = decision.optJSONObject("request")?.let(::plain),
            exchange = if (decision.isNull("exchange")) NO_EXCHANGE else decision.optLong("exchange", NO_EXCHANGE),
        )
    }

    fun finish(exchange: Long, status: Int, headers: List<Pair<String, String>>, body: ByteArray): Plain? {
        if (exchange == NO_EXCHANGE) return null
        val response = JSONObject()
            .put("status", status)
            .put("headers", headersJson(headers))
            .put("body_base64", Base64.getEncoder().encodeToString(body))
        return Native.networkFinish(exchange, response.toString())?.let { plain(JSONObject(it)) }
    }

    fun abandon(exchange: Long) {
        if (exchange != NO_EXCHANGE) runCatching { Native.networkAbandon(exchange) }
    }

    private fun headersJson(headers: List<Pair<String, String>>): JSONArray =
        JSONArray(headers.map { JSONArray().put(it.first).put(it.second) })

    private fun plain(value: JSONObject): Plain {
        val headers = value.optJSONArray("headers") ?: JSONArray()
        val pairs = (0 until headers.length()).mapNotNull { index ->
            headers.optJSONArray(index)?.takeIf { it.length() == 2 }?.let { it.optString(0) to it.optString(1) }
        }
        val body = value.optString("body_base64").takeIf(String::isNotEmpty)?.let { Base64.getDecoder().decode(it) } ?: ByteArray(0)
        return Plain(value.optString("method", "GET"), value.optString("url"), value.optInt("status", OK), pairs, body)
    }
}
