package app.astral.android

import android.content.res.AssetManager
import android.graphics.fonts.Font
import android.graphics.fonts.FontStyle
import android.os.SystemClock
import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule
import org.json.JSONObject
import java.io.File
import java.util.Collections
import java.util.WeakHashMap

object FontPacks {
    private const val RELOAD_INTERVAL_MS = 1000L
    private const val GG_SANS = "fonts/ggsans-"

    private data class Faces(val pack: String, val ready: Boolean, val files: Map<String, String>)

    @Volatile private var faces = Faces("", false, emptyMap())
    @Volatile private var checkedAt = 0L
    private val assetPaths = Collections.synchronizedMap(WeakHashMap<Font.Builder, String>())

    fun install(module: XposedModule) {
        val fromAsset = Font.Builder::class.java.getDeclaredConstructor(AssetManager::class.java, String::class.java)
        module.hook(fromAsset).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
            val result = chain.proceed()
            val builder = chain.thisObject as? Font.Builder
            val path = chain.getArg(1) as? String
            if (builder != null && path != null && path.startsWith(GG_SANS)) assetPaths[builder] = path
            result
        }
        val build = Font.Builder::class.java.getDeclaredMethod("build")
        module.hook(build).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
            replacement(assetPaths[chain.thisObject as? Font.Builder]) ?: chain.proceed()
        }
    }

    private fun replacement(path: String?): Font? {
        if (path == null) return null
        val weight = weightOf(path)
        val current = reload()
        val file = current.files[weight] ?: current.files["normal"] ?: return null
        val slant = if (path.contains("Italic")) FontStyle.FONT_SLANT_ITALIC else FontStyle.FONT_SLANT_UPRIGHT
        return runCatching { Font.Builder(File(file)).setWeight(weightValue(weight)).setSlant(slant).build() }
            .onFailure { Log.warning("Failed to load font pack face $weight from ${current.pack}: ${it.javaClass.simpleName}") }
            .getOrNull()
    }

    private fun reload(): Faces {
        val now = SystemClock.elapsedRealtime()
        if (faces.ready && now - checkedAt < RELOAD_INTERVAL_MS) return faces
        checkedAt = now
        val spec = runCatching { JSONObject(Native.fontFaces()) }.getOrNull() ?: return faces
        val files = mutableMapOf<String, String>()
        val listed = spec.optJSONObject("faces")
        listed?.keys()?.forEach { weight ->
            listed.optJSONObject(weight)?.optString("file")?.takeIf { it.isNotEmpty() && File(it).isFile }?.let { files[weight] = it }
        }
        faces = Faces(spec.optString("id"), spec.optBoolean("ready"), files)
        return faces
    }

    private fun weightOf(path: String): String =
        when (path.removePrefix(GG_SANS).substringBefore('.').removeSuffix("Italic")) {
            "ExtraBold" -> "extrabold"
            "Bold" -> "bold"
            "Semibold" -> "semibold"
            "Medium" -> "medium"
            else -> "normal"
        }

    private fun weightValue(weight: String): Int =
        when (weight) {
            "extrabold" -> FontStyle.FONT_WEIGHT_EXTRA_BOLD
            "bold" -> FontStyle.FONT_WEIGHT_BOLD
            "semibold" -> FontStyle.FONT_WEIGHT_SEMI_BOLD
            "medium" -> FontStyle.FONT_WEIGHT_MEDIUM
            else -> FontStyle.FONT_WEIGHT_NORMAL
        }
}
