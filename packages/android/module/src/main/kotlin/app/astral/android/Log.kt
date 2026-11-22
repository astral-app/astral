package app.astral.android

object Log {
    private const val TAG = "astral"

    fun info(message: String) {
        android.util.Log.i(TAG, message)
    }

    fun warning(message: String) {
        android.util.Log.w(TAG, message)
    }

    fun error(message: String, error: Throwable? = null) {
        android.util.Log.e(TAG, message, error)
    }
}
