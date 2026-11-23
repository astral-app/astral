package app.astral.android

import java.io.File

object ForcePhoenix {
    private const val PREFERENCES = "shared_prefs/discord_fat_client.xml"
    private const val LAUNCH_CLIENT = "launch_client"
    private const val UNHEALTHY = "unhealthy_phoenix_launches"
    private val ENTRY = Regex("""\s*<(string|int|boolean|long) name="([^"]+)"[^>]*?(?:/>|>.*?</\1>)""")

    fun apply(dataDirectory: String) {
        val file = File(dataDirectory, PREFERENCES)
        val existing = if (file.isFile) file.readText() else ""
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
}
