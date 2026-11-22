package app.astral.android

import android.content.pm.ApplicationInfo
import android.os.Build
import java.io.File
import java.io.FileOutputStream
import java.util.zip.ZipFile

object NativeLibrary {
    private const val NAME = "libastral.so"

    fun load(module: ApplicationInfo, app: ApplicationInfo) {
        System.load(path(module, app))
    }

    private fun path(module: ApplicationInfo, app: ApplicationInfo): String {
        val installed = module.nativeLibraryDir?.let { File(it, NAME) }
        if (installed?.isFile == true) return installed.absolutePath
        val apk = File(module.sourceDir)
        val target = File(app.dataDir, "code_cache/astral/${apk.lastModified()}-${apk.length()}/$NAME")
        if (target.isFile) return target.absolutePath
        target.parentFile?.parentFile?.deleteRecursively()
        target.parentFile?.mkdirs()
        extract(apk, target)
        return target.absolutePath
    }

    private fun extract(apk: File, target: File) {
        ZipFile(apk).use { zip ->
            val entry = Build.SUPPORTED_ABIS.firstNotNullOfOrNull { abi -> zip.getEntry("lib/$abi/$NAME") }
                ?: error("astral's APK has no $NAME for ${Build.SUPPORTED_ABIS.joinToString()}")
            val staging = File(target.parentFile, "$NAME.partial")
            zip.getInputStream(entry).use { input ->
                FileOutputStream(staging).use { output ->
                    input.copyTo(output)
                    output.fd.sync()
                }
            }
            check(entry.size < 0 || staging.length() == entry.size) { "Extracted $NAME is ${staging.length()} bytes, expected ${entry.size}" }
            check(staging.renameTo(target)) { "Failed to move the extracted $NAME into place" }
        }
    }
}
