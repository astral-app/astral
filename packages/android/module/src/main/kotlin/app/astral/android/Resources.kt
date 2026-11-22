package app.astral.android

import android.content.pm.ApplicationInfo
import java.io.File
import java.util.zip.ZipFile

object Resources {
    private const val PREFIX = "assets/astral/"

    fun extract(module: ApplicationInfo, app: ApplicationInfo): File {
        val apk = File(module.sourceDir)
        val root = File(app.dataDir, "files/astral-resources")
        val target = File(root, "${apk.lastModified()}-${apk.length()}")
        if (File(target, ".complete").isFile) return target
        root.deleteRecursively()
        target.mkdirs()
        ZipFile(apk).use { zip ->
            for (entry in zip.entries()) {
                if (entry.isDirectory || !entry.name.startsWith(PREFIX)) continue
                val relative = entry.name.removePrefix(PREFIX)
                if (relative.contains("..")) continue
                val file = File(target, relative)
                file.parentFile?.mkdirs()
                zip.getInputStream(entry).use { input -> file.outputStream().use { output -> input.copyTo(output) } }
            }
        }
        File(target, ".complete").writeText("")
        return target
    }
}
