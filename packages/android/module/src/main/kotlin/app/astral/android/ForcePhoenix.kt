package app.astral.android

import android.os.Handler
import android.os.Looper
import java.io.File

object ForcePhoenix {
    private const val PREFERENCES = "shared_prefs/discord_fat_client.xml"
    private const val MEMORY = "files/astral/engine-memory"
    private const val LAUNCH_CLIENT = "launch_client"
    private const val UNHEALTHY = "unhealthy_phoenix_launches"
    private const val NOTICE_DELAY_MS = 6000L
    private val ENTRY = Regex("""\s*<(string|int|boolean|long) name="([^"]+)"[^>]*?(?:/>|>.*?</\1>)""")
    private val CLIENT = Regex("""<string name="$LAUNCH_CLIENT">([^<]*)</string>""")

    fun apply(dataDirectory: String) {
        val file = File(dataDirectory, PREFERENCES)
        val existing = if (file.isFile) file.readText() else ""
        val previous = CLIENT.find(existing)?.groupValues?.get(1).orEmpty()
        val memoryFile = File(dataDirectory, MEMORY)
        val remembered = runCatching { memoryFile.readText().trim().split(' ') }.getOrDefault(emptyList())
        val decision = Native.engineDecide(previous, remembered.getOrNull(0) == "1", remembered.getOrNull(1)?.toIntOrNull() ?: 0)
            ?: return
        memoryFile.parentFile?.mkdirs()
        memoryFile.writeText("${decision[1]} ${decision[2]}")
        if (decision[3] == 1) explainRefusal()
        if (decision[0] != 1) {
            Log.warning("Discord keeps leaving Phoenix; astral stopped forcing it")
            return
        }
        val kept = ENTRY.findAll(existing)
            .filter { it.groupValues[2] != LAUNCH_CLIENT && it.groupValues[2] != UNHEALTHY }
            .joinToString("") { "\n    " + it.value.trim() }
        val document = "<?xml version='1.0' encoding='utf-8' standalone='yes' ?>\n<map>\n" +
            "    <string name=\"$LAUNCH_CLIENT\">phoenix</string>$kept\n</map>\n"
        if (document == existing) return
        file.parentFile?.mkdirs()
        val staging = File(file.parentFile, "${file.name}.astral")
        staging.writeText(document)
        check(staging.renameTo(file)) { "Failed to replace ${file.name}" }
        Log.info("Kept Phoenix as Discord's client")
    }

    private fun explainRefusal() {
        val title = Native.engineRefusedTitle()
        val message = Native.engineRefusedMessage()
        Handler(Looper.getMainLooper()).postDelayed({ PhoenixBridge.showAlert(title, message) }, NOTICE_DELAY_MS)
    }
}
