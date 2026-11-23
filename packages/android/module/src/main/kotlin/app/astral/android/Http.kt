package app.astral.android

import java.net.HttpURLConnection
import java.net.URL

object Http {
    private const val TIMEOUT_MS = 15_000

    fun get(url: String, token: String): String {
        val connection = URL(url).openConnection() as HttpURLConnection
        return try {
            connection.connectTimeout = TIMEOUT_MS
            connection.readTimeout = TIMEOUT_MS
            connection.useCaches = false
            connection.setRequestProperty("Authorization", token)
            val status = connection.responseCode
            val stream = if (status in 200..299) connection.inputStream else connection.errorStream
            val body = stream?.use { it.readBytes().decodeToString() }.orEmpty()
            "$status\n$body"
        } catch (error: Exception) {
            Log.warning("Failed to fetch $url: ${error.message}")
            "0\n"
        } finally {
            connection.disconnect()
        }
    }
}
