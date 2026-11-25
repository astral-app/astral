package app.astral.android

class HttpExchange(
    @JvmField var method: String,
    @JvmField var url: String,
    @JvmField var headers: Array<String>,
    @JvmField var body: ByteArray,
    @JvmField var status: Int = 0,
) {
    @JvmField var respond: Boolean = false
}
