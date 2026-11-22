package app.astral.android

import android.content.pm.ApplicationInfo
import dalvik.system.DexFile
import java.io.File
import java.lang.reflect.Field
import java.lang.reflect.Modifier

object PropertyTable {
    private const val CACHE = "files/astral/property-table"
    private const val ANCHOR = "accessibilityActions"
    private val OBFUSCATED = Regex("^[a-z][a-z0-9]{0,2}\\.[a-z][a-z0-9]{0,2}$")

    fun names(app: ApplicationInfo, loader: ClassLoader): Array<String>? {
        val cache = File(app.dataDir, CACHE)
        val apk = File(app.sourceDir)
        val key = "${apk.lastModified()}-${apk.length()}"
        val cached = runCatching { cache.readText().split(' ') }.getOrNull()
        if (cached?.size == 2 && cached[0] == key) table(loader, cached[1])?.let { return it }
        val found = search(apk, loader) ?: return null
        runCatching {
            cache.parentFile?.mkdirs()
            cache.writeText("$key ${found.first}")
        }
        return found.second
    }

    @Suppress("DEPRECATION")
    private fun search(apk: File, loader: ClassLoader): Pair<String, Array<String>>? {
        val dex = DexFile(apk)
        try {
            for (name in dex.entries()) {
                if (!OBFUSCATED.matches(name)) continue
                val names = table(loader, name) ?: continue
                return name to names
            }
        } finally {
            dex.close()
        }
        return null
    }

    private fun table(loader: ClassLoader, name: String): Array<String>? {
        val type = runCatching { Class.forName(name, false, loader) }.getOrNull() ?: return null
        if (!Modifier.isAbstract(type.modifiers)) return null
        val fields = type.declaredFields.filter { Modifier.isStatic(it.modifiers) }
        if (fields.size != 2 || fields.none { it.type == HashMap::class.java }) return null
        val names = fields.firstOrNull { it.type == Array<String>::class.java } ?: return null
        return read(names)
    }

    @Suppress("UNCHECKED_CAST")
    private fun read(field: Field): Array<String>? {
        field.isAccessible = true
        val names = field.get(null) as? Array<String> ?: return null
        return names.takeIf { it.size > 100 && it.getOrNull(1) == ANCHOR }
    }
}
