package app.astral.android

import android.content.pm.ApplicationInfo
import android.content.res.Resources
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.drawable.BitmapDrawable
import android.graphics.drawable.Drawable
import android.graphics.drawable.ColorDrawable
import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger
import java.util.zip.ZipFile

object ResourceImages {
    private data class Image(val name: String, val original: Int)

    private val nextID = AtomicInteger(0x7e000000)
    private val identifiers = ConcurrentHashMap<String, Int>()
    private val images = ConcurrentHashMap<Int, Image>()
    private var aliases: Map<String, String> = emptyMap()
    private lateinit var assets: File

    fun install(module: XposedModule, app: ApplicationInfo, resources: File) {
        assets = File(resources, "assets")
        aliases = runCatching {
            ZipFile(app.sourceDir).use { zip ->
                val entry = zip.getEntry("assets/phoenix_fat_client_aliases.txt") ?: return@use emptyMap()
                zip.getInputStream(entry).bufferedReader().useLines { lines ->
                    lines.map { it.split(' ') }.filter { it.size == 3 && it[0] == "drawable" }
                        .associate { it[1] to it[2] }
                }
            }
        }.getOrDefault(emptyMap())
        val lookup = Resources::class.java.getDeclaredMethod("getIdentifier", String::class.java, String::class.java, String::class.java)
        module.hook(lookup).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
            val raw = chain.getArg(0) as? String
            val kind = chain.getArg(1) as? String
            val name = raw?.substringBefore('#') ?: ""
            val pluginIcon = if (name.startsWith("astral_plugin_")) Native.iconPath(name) else ""
            if (kind != "drawable" || chain.getArg(2) != app.packageName ||
                !(name.startsWith("ic_") || ownAsset(name) != null || pluginIcon.isNotEmpty())) return@intercept chain.proceed()
            val resource = chain.thisObject as Resources
            val arguments = chain.args.toTypedArray().apply { this[0] = name }
            var original = chain.proceed(arguments) as Int
            if (original == 0) aliases[name]?.let { original = resource.getIdentifier(it, kind, chain.getArg(2) as? String) }
            if (original == 0 && ownAsset(name) == null && pluginIcon.isEmpty() && Native.iconPath(name).isEmpty()) return@intercept 0
            val fallback = original
            identifiers.computeIfAbsent(name) {
                val identifier = nextID.incrementAndGet()
                images[identifier] = Image(name, fallback)
                identifier
            }
        }
        for (method in Resources::class.java.declaredMethods) {
            if (method.name != "getDrawable" || method.parameterTypes.firstOrNull() != Int::class.javaPrimitiveType) continue
            module.hook(method).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
                val image = images[chain.getArg(0) as? Int] ?: return@intercept chain.proceed()
                val resource = chain.thisObject as Resources
                drawable(resource, image.name) ?: if (image.original == 0) ColorDrawable(android.graphics.Color.TRANSPARENT)
                else chain.proceed(chain.args.toTypedArray().apply { this[0] = image.original })
            }
        }
    }

    private fun ownAsset(name: String): File? {
        if (!name.startsWith("astral_") || name.any { !(it.isLetterOrDigit() || it == '_') }) return null
        val base = name.removePrefix("astral_")
        return listOf("svg", "png").map { File(assets, "$base.$it") }.firstOrNull { it.isFile }
    }

    private fun drawable(resources: Resources, name: String): Drawable? = runCatching {
        val path = Native.iconPath(name)
        if (path.isEmpty()) {
            val file = ownAsset(name) ?: return@runCatching null
            return@runCatching if (file.extension == "png") pngDrawable(resources, file) else svgDrawable(resources, file)
        }
        if (path.endsWith(".svg")) return@runCatching svgDrawable(resources, File(path))
        val bitmap = BitmapFactory.decodeFile(path) ?: return@runCatching null
        val scale = if (path.endsWith("@3x.png")) 3 else if (path.endsWith("@2x.png")) 2 else 1
        bitmap.density = scale * 160
        BitmapDrawable(resources, bitmap)
    }.getOrNull()

    private fun pngDrawable(resources: Resources, file: File): Drawable? {
        val size = (24 * resources.displayMetrics.density).toInt().coerceAtLeast(1)
        val bitmap = BitmapFactory.decodeFile(file.path) ?: return null
        val scaled = Bitmap.createScaledBitmap(bitmap, size, size, true)
        scaled.density = resources.displayMetrics.densityDpi
        return BitmapDrawable(resources, scaled)
    }

    private fun svgDrawable(resources: Resources, file: File): Drawable? {
        val size = (24 * resources.displayMetrics.density).toInt().coerceAtLeast(1)
        val png = Native.renderSvg(file.readBytes(), size, size) ?: return null
        val bitmap = BitmapFactory.decodeByteArray(png, 0, png.size) ?: return null
        bitmap.density = resources.displayMetrics.densityDpi
        return BitmapDrawable(resources, bitmap)
    }
}
