package app.astral.android

object Native {
    @JvmStatic external fun initialize(propertyNames: Array<String>?): Int

    @JvmStatic external fun rewrite(ops: ByteArray, length: Int): ByteArray?
}
