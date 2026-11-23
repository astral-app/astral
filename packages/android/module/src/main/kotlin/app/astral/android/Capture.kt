package app.astral.android

import java.io.File

object Capture {
    private const val MAX_BATCHES = 400

    @Volatile private var directory: File? = null
    private var count = 0

    fun configure(dataDirectory: String) {
        val folder = File(dataDirectory, "files/astral/capture-wire")
        directory = folder.takeIf { it.isDirectory }
        if (directory != null) Log.info("Recording ViewWire batches to $folder")
    }

    @Synchronized
    fun record(ops: ByteArray, length: Int) {
        val folder = directory ?: return
        if (count >= MAX_BATCHES) return
        count++
        runCatching { File(folder, "%04d.bin".format(count)).writeBytes(ops.copyOf(length)) }
            .onFailure { Log.warning("Failed to record batch $count: ${it.message}") }
    }
}
