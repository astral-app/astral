package app.astral.android

import android.graphics.Color
import android.text.Spannable
import android.text.Spanned
import android.text.style.BackgroundColorSpan
import android.text.style.ForegroundColorSpan
import android.text.style.RelativeSizeSpan
import android.text.style.StrikethroughSpan
import android.text.style.StyleSpan
import android.text.style.TypefaceSpan
import android.text.style.UnderlineSpan
import android.widget.EditText
import org.json.JSONArray
import java.util.Collections
import java.util.WeakHashMap
import kotlin.math.roundToInt

object MarkdownComposer {
    private val applied = Collections.synchronizedMap(WeakHashMap<EditText, List<Any>>())

    fun apply(view: EditText) {
        val text = view.text ?: return
        val content = text as? Spannable ?: return
        applied.remove(view)?.forEach(content::removeSpan)
        val runs = runCatching { JSONArray(Native.composerStyles(text.toString())) }.getOrNull() ?: return
        val created = ArrayList<Any>()
        for (index in 0 until minOf(runs.length(), 128)) {
            val run = runs.optJSONObject(index) ?: continue
            val start = run.optInt("start", 0)
            val length = run.optInt("length", 0)
            val style = run.optString("style", "")
            if (start < 1 || length < 1) continue
            val first = utf16Offset(text.toString(), start - 1) ?: continue
            val end = utf16Offset(text.toString(), start - 1 + length) ?: continue
            if (end <= first || end > content.length) continue
            for (span in spans(style, view.currentTextColor)) {
                content.setSpan(span, first, end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                created.add(span)
            }
        }
        applied[view] = created
    }

    private fun spans(style: String, color: Int): List<Any> = when (style) {
        "bold" -> listOf(StyleSpan(android.graphics.Typeface.BOLD))
        "italic" -> listOf(StyleSpan(android.graphics.Typeface.ITALIC))
        "underline" -> listOf(UnderlineSpan())
        "strike" -> listOf(StrikethroughSpan())
        "code" -> listOf(TypefaceSpan("monospace"), BackgroundColorSpan(Color.argb(51, 128, 128, 128)))
        "spoiler" -> listOf(BackgroundColorSpan(Color.argb(89, 0, 0, 0)))
        "marker" -> listOf(ForegroundColorSpan(withAlpha(color, 0.4f)))
        "heading1" -> listOf(RelativeSizeSpan(1.45f))
        "heading2" -> listOf(RelativeSizeSpan(1.3f))
        "heading3" -> listOf(RelativeSizeSpan(1.15f))
        "subtext" -> listOf(RelativeSizeSpan(0.85f), ForegroundColorSpan(withAlpha(color, 0.65f)))
        "quote" -> listOf(ForegroundColorSpan(withAlpha(color, 0.75f)))
        else -> emptyList()
    }

    private fun utf16Offset(text: String, byteOffset: Int): Int? {
        val bytes = text.toByteArray(Charsets.UTF_8)
        if (byteOffset < 0 || byteOffset > bytes.size) return null
        val prefix = bytes.copyOfRange(0, byteOffset)
        val decoded = String(prefix, Charsets.UTF_8)
        if (!decoded.toByteArray(Charsets.UTF_8).contentEquals(prefix)) return null
        return decoded.length
    }

    private fun withAlpha(color: Int, strength: Float): Int = Color.argb(
        (Color.alpha(color) * strength).roundToInt(),
        Color.red(color),
        Color.green(color),
        Color.blue(color),
    )
}
