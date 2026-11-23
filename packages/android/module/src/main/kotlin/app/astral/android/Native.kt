package app.astral.android

object Native {
    @JvmStatic external fun initialize(
        propertyNames: Array<String>?,
        dataDirectory: String,
        resourceDirectory: String,
        languages: Array<String>,
    ): Int

    @JvmStatic external fun rewrite(ops: ByteArray, length: Int): ByteArray?

    @JvmStatic external fun plugins(): String

    @JvmStatic external fun setEnabled(identifier: String, enabled: Boolean)

    @JvmStatic external fun eval(identifier: String, code: String): String

    @JvmStatic external fun pendingUpdates(): ByteArray?

    @JvmStatic external fun attachBridge(bridge: Class<*>)

    @JvmStatic external fun setSessionToken(token: String)

    @JvmStatic external fun isTelemetry(host: String, path: String): Boolean

    @JvmStatic external fun sinkUrl(): String
}
