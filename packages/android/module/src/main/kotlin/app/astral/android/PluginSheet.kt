package app.astral.android

import android.app.Activity
import android.app.Dialog
import android.graphics.BitmapFactory
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.GradientDrawable
import android.text.InputFilter
import android.util.LruCache
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.widget.Button
import android.widget.EditText
import android.widget.HorizontalScrollView
import android.widget.ImageButton
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import org.json.JSONArray
import org.json.JSONObject
import java.net.URL
import java.util.concurrent.ConcurrentHashMap

object PluginSheet {
    private const val INSET_DP = 16
    private const val CORNER_DP = 8
    private const val FILL_MINIMUM_DP = 140
    private const val MAXIMUM_INPUT = 4000
    private val sheets = ConcurrentHashMap<Long, Rendered>()
    private val images = LruCache<String, android.graphics.Bitmap>(64)

    private class Palette(dark: Boolean) {
        val background = if (dark) 0xFF232428.toInt() else 0xFFF2F3F5.toInt()
        val surface = if (dark) 0xFF2B2D31.toInt() else 0xFFFFFFFF.toInt()
        val code = if (dark) 0xFF1E1F22.toInt() else 0xFFE3E5E8.toInt()
        val primary = if (dark) 0xFFF2F3F5.toInt() else 0xFF060607.toInt()
        val muted = if (dark) 0xFFB5BAC1.toInt() else 0xFF4E5058.toInt()
        val divider = if (dark) 0xFF3F4147.toInt() else 0xFFD4D7DC.toInt()
        val secondaryButton = if (dark) 0xFF4E5058.toInt() else 0xFF6D6F78.toInt()
    }

    private class Rendered(
        val activity: Activity,
        val dialog: Dialog,
        val title: TextView,
        val subtitle: TextView,
        val content: LinearLayout,
        val event: (String, String, Boolean) -> Unit,
    ) {
        var finished = false
    }

    fun open(activity: Activity, sheet: Long, spec: JSONObject, event: (String, String, Boolean) -> Unit) {
        val palette = palette(activity)
        val dialog = Dialog(activity)
        val root = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            background = rounded(palette.background, dp(activity, 16).toFloat(), topOnly = true)
        }
        val grabber = View(activity).apply {
            background = rounded(palette.divider, dp(activity, 3).toFloat())
            layoutParams = LinearLayout.LayoutParams(dp(activity, 36), dp(activity, 5)).apply {
                gravity = Gravity.CENTER_HORIZONTAL
                topMargin = dp(activity, 8)
            }
        }
        val title = TextView(activity).apply {
            setTextColor(palette.primary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 18f)
            typeface = Typeface.DEFAULT_BOLD
            gravity = Gravity.CENTER
            setPadding(dp(activity, INSET_DP), dp(activity, 14), dp(activity, INSET_DP), dp(activity, 12))
        }
        val subtitle = TextView(activity).apply {
            setTextColor(palette.muted)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
            gravity = Gravity.CENTER
            setPadding(dp(activity, INSET_DP), 0, dp(activity, INSET_DP), dp(activity, 10))
        }
        val content = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(activity, INSET_DP), 0, dp(activity, INSET_DP), dp(activity, INSET_DP))
        }
        val scroll = ScrollView(activity).apply {
            isFillViewport = true
            addView(content)
        }
        root.addView(grabber)
        root.addView(title)
        root.addView(subtitle)
        root.addView(scroll, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        dialog.setContentView(root)
        dialog.window?.apply {
            setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
            setGravity(Gravity.BOTTOM)
            val height = activity.resources.displayMetrics.heightPixels
            val fraction = if (spec.optBoolean("expanded")) 0.9 else 0.6
            setLayout(ViewGroup.LayoutParams.MATCH_PARENT, (height * fraction).toInt())
            setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)
        }
        val rendered = Rendered(activity, dialog, title, subtitle, content, event)
        dialog.setOnDismissListener { finish(sheet, rendered) }
        sheets[sheet] = rendered
        render(rendered, spec)
        dialog.show()
    }

    fun update(sheet: Long, spec: JSONObject) {
        val rendered = sheets[sheet] ?: return
        render(rendered, spec)
    }

    fun close(sheet: Long) {
        val rendered = sheets[sheet] ?: return
        rendered.dialog.dismiss()
    }

    private fun finish(sheet: Long, rendered: Rendered) {
        if (rendered.finished) return
        rendered.finished = true
        sheets.remove(sheet)
        rendered.event("", "", true)
    }

    private fun render(rendered: Rendered, spec: JSONObject) {
        val activity = rendered.activity
        rendered.title.text = spec.optString("title")
        rendered.subtitle.text = spec.optString("subtitle")
        rendered.subtitle.visibility = if (spec.optString("subtitle").isEmpty()) View.GONE else View.VISIBLE
        rendered.content.removeAllViews()
        items(spec.optJSONArray("items")).forEach { item ->
            val view = viewFor(rendered, item) ?: return@forEach
            val fills = item.optBoolean("fill")
            val height = if (fills) 0 else ViewGroup.LayoutParams.WRAP_CONTENT
            rendered.content.addView(view, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, height, if (fills) 1f else 0f).apply {
                topMargin = dp(activity, 12)
            })
        }
    }

    private fun items(array: JSONArray?): List<JSONObject> =
        (0 until (array?.length() ?: 0)).mapNotNull { array?.optJSONObject(it) }

    private fun viewFor(rendered: Rendered, item: JSONObject): View? {
        val activity = rendered.activity
        val palette = palette(activity)
        val view: View = when (item.optString("type")) {
            "heading" -> label(activity, item, 13f, true, palette.muted)
            "text" -> label(activity, item, 15f, item.optBoolean("bold"), palette.primary)
            "code" -> code(activity, item, palette)
            "image" -> image(activity, item)
            "icon" -> icon(activity, item, palette)
            "row", "column" -> group(rendered, item, item.optString("type") == "row")
            "buttons" -> buttons(rendered, item, palette)
            "input" -> input(rendered, item, palette)
            "divider" -> View(activity).apply {
                setBackgroundColor(palette.divider)
                layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 1)
            }
            "spacer" -> View(activity).apply {
                layoutParams = LinearLayout.LayoutParams(1, dp(activity, item.optInt("size", 8)))
            }
            else -> return null
        }
        val wrapped = if (item.has("background")) card(activity, view, item, palette) else view
        val action = item.optString("action")
        if (action.isNotEmpty() && item.optString("type") != "buttons" && item.optString("type") != "input") {
            wrapped.isClickable = true
            wrapped.setOnClickListener { rendered.event(action, "", false) }
        }
        val longAction = item.optString("long_action")
        if (longAction.isNotEmpty()) {
            wrapped.isLongClickable = true
            wrapped.setOnLongClickListener {
                rendered.event(longAction, "", false)
                true
            }
        }
        if (item.has("opacity")) wrapped.alpha = item.optDouble("opacity", 1.0).toFloat()
        return wrapped
    }

    private fun label(activity: Activity, item: JSONObject, size: Float, bold: Boolean, fallback: Int) = TextView(activity).apply {
        text = item.optString("text")
        setTextSize(TypedValue.COMPLEX_UNIT_SP, item.optDouble("size", size.toDouble()).toFloat())
        setTextColor(color(item.opt("color"), fallback, activity))
        typeface = when {
            item.optBoolean("mono") -> Typeface.MONOSPACE
            bold -> Typeface.DEFAULT_BOLD
            else -> Typeface.DEFAULT
        }
        val lines = item.optInt("lines", 0)
        if (lines > 0) maxLines = lines
        gravity = when (item.optString("align")) {
            "center" -> Gravity.CENTER_HORIZONTAL
            "end" -> Gravity.END
            else -> Gravity.START
        }
    }

    private fun code(activity: Activity, item: JSONObject, palette: Palette): View {
        val text = TextView(activity).apply {
            text = item.optString("text")
            typeface = Typeface.MONOSPACE
            setTextSize(TypedValue.COMPLEX_UNIT_SP, item.optDouble("size", 12.0).toFloat())
            setTextColor(palette.primary)
            setTextIsSelectable(true)
            setPadding(dp(activity, 10), dp(activity, 10), dp(activity, 10), dp(activity, 10))
        }
        val height = item.optInt("height", 0)
        val fills = item.optBoolean("fill")
        val container = if (fills) ScrollView(activity).apply {
            addView(HorizontalScrollView(activity).apply { addView(text) })
            minimumHeight = dp(activity, if (height > 0) height else FILL_MINIMUM_DP)
        } else if (height > 0) ScrollView(activity).apply {
            addView(HorizontalScrollView(activity).apply { addView(text) })
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(activity, height))
        } else text
        container.background = rounded(palette.code, dp(activity, CORNER_DP).toFloat())
        return container
    }

    private fun image(activity: Activity, item: JSONObject): View {
        val size = item.optInt("size", 40)
        val width = dp(activity, item.optInt("width", size))
        val height = dp(activity, item.optInt("height", size))
        val view = ImageView(activity).apply {
            scaleType = if (item.optString("fit") == "cover") ImageView.ScaleType.CENTER_CROP else ImageView.ScaleType.FIT_CENTER
            layoutParams = LinearLayout.LayoutParams(width, height)
            clipToOutline = true
            val radius = if (item.optBoolean("round")) minOf(width, height) / 2f else dp(activity, item.optInt("radius", 0)).toFloat()
            background = rounded(Color.TRANSPARENT, radius)
        }
        val address = item.optString("url")
        images.get(address)?.let {
            view.setImageBitmap(it)
            return view
        }
        if (!address.startsWith("https://")) return view
        Thread {
            val bitmap = runCatching { URL(address).openStream().use { BitmapFactory.decodeStream(it) } }.getOrNull() ?: return@Thread
            images.put(address, bitmap)
            view.post { view.setImageBitmap(bitmap) }
        }.start()
        return view
    }

    private val iconGlyphs = mapOf(
        "thumbs_up" to "👍", "thumbs_down" to "👎", "star" to "★", "chevron_right" to "›",
        "flag" to "⚑", "deny" to "⊘", "trash" to "🗑", "info" to "ⓘ", "link" to "🔗",
    )

    private fun icon(activity: Activity, item: JSONObject, palette: Palette): View {
        val name = item.optString("name")
        val size = dp(activity, item.optInt("size", 16) + 4)
        val tint = color(item.opt("color"), palette.muted, activity)
        val drawable = activity.resources.getIdentifier("ic_mana_$name", "drawable", activity.packageName)
        if (drawable != 0) {
            return ImageView(activity).apply {
                setImageResource(drawable)
                setColorFilter(tint)
                layoutParams = LinearLayout.LayoutParams(size, size)
            }
        }
        return TextView(activity).apply {
            text = iconGlyphs[name] ?: "•"
            setTextColor(tint)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, item.optInt("size", 16).toFloat())
            gravity = Gravity.CENTER
        }
    }

    private fun group(rendered: Rendered, item: JSONObject, horizontal: Boolean): View {
        val activity = rendered.activity
        val spacing = dp(activity, item.optInt("spacing", if (horizontal) 10 else 4))
        val layout = LinearLayout(activity).apply {
            orientation = if (horizontal) LinearLayout.HORIZONTAL else LinearLayout.VERTICAL
            gravity = if (horizontal && item.optString("align") != "top") Gravity.CENTER_VERTICAL else Gravity.TOP
        }
        items(item.optJSONArray("items")).forEachIndexed { index, child ->
            val view = viewFor(rendered, child) ?: return@forEachIndexed
            val params = (view.layoutParams as? LinearLayout.LayoutParams)
                ?: LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT)
            if (horizontal && (child.optString("type") == "column" || child.optBoolean("grow"))) {
                params.width = 0
                params.weight = 1f
            }
            if (index > 0) {
                if (horizontal) params.marginStart = spacing else params.topMargin = spacing
            }
            layout.addView(view, params)
        }
        return layout
    }

    private fun buttons(rendered: Rendered, item: JSONObject, palette: Palette): View {
        val activity = rendered.activity
        val column = item.optString("direction") == "column"
        val layout = LinearLayout(activity).apply { orientation = if (column) LinearLayout.VERTICAL else LinearLayout.HORIZONTAL }
        items(item.optJSONArray("buttons")).forEachIndexed { index, spec ->
            val button = Button(activity).apply {
                text = spec.optString("label")
                isAllCaps = false
                setTextColor(Color.WHITE)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
                typeface = Typeface.DEFAULT_BOLD
                stateListAnimator = null
                val fill = when (spec.optString("style")) {
                    "primary" -> 0xFF5865F2.toInt()
                    "danger" -> 0xFFDA373C.toInt()
                    else -> palette.secondaryButton
                }
                background = rounded(fill, dp(activity, CORNER_DP).toFloat())
                val action = spec.optString("action")
                setOnClickListener { if (action.isNotEmpty()) rendered.event(action, "", false) }
            }
            val params = if (column) {
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(activity, 40))
            } else {
                LinearLayout.LayoutParams(0, dp(activity, 40), 1f)
            }
            if (index > 0) {
                if (column) params.topMargin = dp(activity, 8) else params.marginStart = dp(activity, 8)
            }
            layout.addView(button, params)
        }
        return layout
    }

    private fun labelledInput(rendered: Rendered, item: JSONObject, palette: Palette): View {
        val activity = rendered.activity
        val layout = LinearLayout(activity).apply { orientation = LinearLayout.VERTICAL }
        val field = EditText(activity).apply {
            hint = item.optString("placeholder")
            setText(item.optString("value"))
            setTextColor(palette.primary)
            setHintTextColor(palette.muted)
            background = rounded(palette.surface, dp(activity, CORNER_DP).toFloat())
            setPadding(dp(activity, 12), dp(activity, 12), dp(activity, 12), dp(activity, 12))
            minHeight = dp(activity, item.optInt("height", 96))
            gravity = Gravity.TOP or Gravity.START
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
            filters = arrayOf(InputFilter.LengthFilter(MAXIMUM_INPUT))
        }
        val submit = Button(activity).apply {
            text = item.optString("submit_label")
            isAllCaps = false
            setTextColor(Color.WHITE)
            typeface = Typeface.DEFAULT_BOLD
            stateListAnimator = null
            background = rounded(0xFF5865F2.toInt(), dp(activity, CORNER_DP).toFloat())
            val action = item.optString("action")
            setOnClickListener {
                val value = field.text.toString().trim()
                if (value.isEmpty() || action.isEmpty()) return@setOnClickListener
                rendered.event(action, value, false)
                field.setText("")
            }
        }
        layout.addView(field, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        layout.addView(submit, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(activity, 40)).apply { topMargin = dp(activity, 8) })
        return layout
    }

    private fun input(rendered: Rendered, item: JSONObject, palette: Palette): View {
        if (item.optString("submit_label").isNotEmpty()) return labelledInput(rendered, item, palette)
        val activity = rendered.activity
        val container = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.BOTTOM
            background = rounded(palette.surface, dp(activity, 20).toFloat())
            setPadding(dp(activity, 14), dp(activity, 4), dp(activity, 6), dp(activity, 6))
        }
        val text = EditText(activity).apply {
            hint = item.optString("placeholder")
            setText(item.optString("value"))
            setTextColor(palette.primary)
            setHintTextColor(palette.muted)
            background = null
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
            filters = arrayOf(InputFilter.LengthFilter(MAXIMUM_INPUT))
        }
        val send = ImageButton(activity).apply {
            setImageResource(android.R.drawable.ic_menu_send)
            setColorFilter(Color.WHITE)
            background = rounded(0xFF5865F2.toInt(), dp(activity, 16).toFloat())
            val action = item.optString("action")
            setOnClickListener {
                val value = text.text.toString().trim()
                if (value.isEmpty() || action.isEmpty()) return@setOnClickListener
                rendered.event(action, value, false)
                text.setText("")
            }
        }
        container.addView(text, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        container.addView(send, LinearLayout.LayoutParams(dp(activity, 32), dp(activity, 32)))
        return container
    }

    private fun card(activity: Activity, content: View, item: JSONObject, palette: Palette): View {
        val padding = dp(activity, item.optInt("padding", 12))
        return LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            background = rounded(color(item.opt("background"), palette.surface, activity), dp(activity, item.optInt("radius", CORNER_DP)).toFloat())
            setPadding(padding, padding, padding, padding)
            addView(content)
        }
    }

    private fun color(value: Any?, fallback: Int, activity: Activity): Int = when (value) {
        is Number -> value.toLong().let { argb -> if (argb ushr 24 == 0L) (argb or 0xFF000000).toInt() else argb.toInt() }
        is String -> when {
            value == "muted" -> palette(activity).muted
            value == "primary" -> palette(activity).primary
            value == "link" -> 0xFF00A8FC.toInt()
            value == "positive" -> 0xFF23A55A.toInt()
            value == "danger" -> 0xFFF23F43.toInt()
            else -> runCatching { Color.parseColor(if (value.startsWith("#")) value else "#$value") }.getOrDefault(fallback)
        }
        else -> fallback
    }

    private fun palette(activity: Activity) = Palette(DiscordTheme.dark(activity))

    private fun rounded(fill: Int, radius: Float, topOnly: Boolean = false) = GradientDrawable().apply {
        setColor(fill)
        if (topOnly) cornerRadii = floatArrayOf(radius, radius, radius, radius, 0f, 0f, 0f, 0f) else cornerRadius = radius
    }

    private fun dp(activity: Activity, value: Int) = (value * activity.resources.displayMetrics.density).toInt()
}
