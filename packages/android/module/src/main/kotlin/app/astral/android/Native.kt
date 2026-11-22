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
}
