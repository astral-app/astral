package app.astral.android

import android.app.Activity
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.os.Handler
import android.os.Looper
import android.text.TextUtils
import android.util.TypedValue
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import org.json.JSONObject

object Toasts {
    private const val DEFAULT_DURATION_MS = 5000L
    private const val ANIMATION_MS = 220L
    private const val MAX_SHOWN = 3
    private const val MAX_WIDTH_DP = 500
    private const val SURFACE = 0xFF2C2D32.toInt()
    private const val BORDER = 0x1FFFFFFF
    private const val TITLE = 0xFFF2F3F5.toInt()
    private const val CONTENT = 0xFFB5BAC1.toInt()
    private const val ICON = 0xFFDBDEE1.toInt()
    private const val CLOSE = 0xFF949BA4.toInt()
    private const val BRAND = 0xFF5865F2.toInt()
    private const val SECONDARY = 0xFF4E5058.toInt()
    private const val DISMISS_DRAG_DP = 24

    private class Shown(val toast: Long, val view: LinearLayout, val event: ((String, String, Boolean) -> Unit)?) {
        var generation = 0L
        var closing = false
    }

    private val main = Handler(Looper.getMainLooper())
    private val shown = mutableListOf<Shown>()
    private var column: LinearLayout? = null
    private var anonymous = 0L

    fun show(activity: Activity, toast: Long, spec: JSONObject, event: ((String, String, Boolean) -> Unit)?) {
        main.post {
            val existing = shown.firstOrNull { it.toast == toast && toast != 0L && !it.closing }
            if (existing != null) {
                render(activity, existing, spec)
                return@post
            }
            if (activity.isFinishing || activity.isDestroyed) {
                event?.invoke("", "", true)
                return@post
            }
            val card = LinearLayout(activity).apply { orientation = LinearLayout.VERTICAL }
            val entry = Shown(if (toast != 0L) toast else Long.MIN_VALUE + ++anonymous, card, event)
            render(activity, entry, spec)
            present(activity, entry)
        }
    }

    fun close(toast: Long) {
        main.post { shown.firstOrNull { it.toast == toast && !it.closing }?.let(::dismiss) }
    }

    private fun dp(activity: Activity, value: Int): Int = (value * activity.resources.displayMetrics.density).toInt()

    private fun column(activity: Activity): LinearLayout {
        val root = activity.window.decorView as ViewGroup
        column?.takeIf { it.parent === root }?.let { return it }
        column?.let { (it.parent as? ViewGroup)?.removeView(it) }
        val screen = activity.resources.displayMetrics.widthPixels
        val width = minOf((screen * 0.9f).toInt(), dp(activity, MAX_WIDTH_DP))
        val created = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            elevation = dp(activity, 12).toFloat()
            setOnApplyWindowInsetsListener { view, insets ->
                view.setPadding(0, insets.systemWindowInsetTop + dp(activity, 8), 0, 0)
                insets
            }
        }
        root.addView(created, FrameLayout.LayoutParams(width, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.TOP or Gravity.CENTER_HORIZONTAL))
        created.requestApplyInsets()
        column = created
        return created
    }

    private fun present(activity: Activity, entry: Shown) {
        val column = column(activity)
        column.bringToFront()
        while (shown.count { !it.closing } >= MAX_SHOWN) dismiss(shown.first { !it.closing })
        shown.add(entry)
        column.addView(entry.view, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
            bottomMargin = dp(activity, 8)
        })
        entry.view.alpha = 0f
        entry.view.translationY = -dp(activity, 24).toFloat()
        entry.view.animate().alpha(1f).translationY(0f).setDuration(ANIMATION_MS).start()
    }

    private fun dismiss(entry: Shown) {
        if (entry.closing) return
        entry.closing = true
        entry.generation++
        val view = entry.view
        view.animate().alpha(0f).translationY(-view.height / 3f).setDuration(ANIMATION_MS).withEndAction {
            (view.parent as? ViewGroup)?.removeView(view)
            shown.remove(entry)
        }.start()
        entry.event?.invoke("", "", true)
    }

    private fun render(activity: Activity, entry: Shown, spec: JSONObject) {
        val card = entry.view
        card.removeAllViews()
        card.background = GradientDrawable().apply {
            setColor(SURFACE)
            cornerRadius = dp(activity, 18).toFloat()
            setStroke(dp(activity, 1), BORDER)
        }
        card.clipToOutline = true
        card.setPadding(0, dp(activity, 4), 0, dp(activity, 4))
        card.addView(header(activity, entry, spec))
        buttons(activity, entry, spec)?.let(card::addView)
        progress(activity, spec)?.let(card::addView)
        card.contentDescription = listOf(spec.optString("title"), spec.optString("content")).filter { it.isNotEmpty() }.joinToString(", ")
        dragToDismiss(activity, entry)
        schedule(entry, spec)
    }

    private fun header(activity: Activity, entry: Shown, spec: JSONObject): View {
        val row = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(activity, 12), dp(activity, 8), dp(activity, 6), dp(activity, 8))
        }
        icon(activity, spec)?.let { row.addView(it, LinearLayout.LayoutParams(dp(activity, 24), dp(activity, 24)).apply { marginEnd = dp(activity, 12) }) }
        val texts = LinearLayout(activity).apply { orientation = LinearLayout.VERTICAL }
        spec.optString("title").takeIf { it.isNotEmpty() }?.let { texts.addView(label(activity, it, 15f, TITLE, 2, true)) }
        spec.optString("content").takeIf { it.isNotEmpty() }?.let { texts.addView(label(activity, it, 13f, CONTENT, 5, false)) }
        row.addView(texts, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        row.addView(closeButton(activity, entry), LinearLayout.LayoutParams(dp(activity, 32), dp(activity, 32)))
        return row
    }

    private fun label(activity: Activity, text: String, size: Float, color: Int, lines: Int, bold: Boolean) = TextView(activity).apply {
        this.text = text
        setTextColor(color)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, size)
        maxLines = lines
        ellipsize = TextUtils.TruncateAt.END
        paint.isFakeBoldText = bold
    }

    private fun drawableId(activity: Activity, uri: String): Int {
        val name = uri.removePrefix("res://")
        if (name.isEmpty()) return 0
        return activity.resources.getIdentifier(name, "drawable", activity.packageName)
    }

    private fun icon(activity: Activity, spec: JSONObject): View? {
        val drawable = drawableId(activity, spec.optString("icon"))
        if (drawable == 0) return null
        return ImageView(activity).apply {
            setImageResource(drawable)
            if (spec.optBoolean("tinted_icon", true)) setColorFilter(ICON)
            scaleType = ImageView.ScaleType.FIT_CENTER
        }
    }

    private fun closeButton(activity: Activity, entry: Shown): View {
        val drawable = drawableId(activity, "ic_mana_x_small")
        val button: View = if (drawable != 0) {
            ImageView(activity).apply {
                setImageResource(drawable)
                setColorFilter(CLOSE)
                scaleType = ImageView.ScaleType.CENTER_INSIDE
            }
        } else {
            label(activity, "✕", 16f, CLOSE, 1, false).apply { gravity = Gravity.CENTER }
        }
        button.contentDescription = "Close"
        button.setOnClickListener { dismiss(entry) }
        return button
    }

    private fun buttons(activity: Activity, entry: Shown, spec: JSONObject): View? {
        val entries = spec.optJSONArray("buttons") ?: return null
        if (entries.length() == 0) return null
        val row = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            setPadding(dp(activity, 12), 0, dp(activity, 12), dp(activity, 8))
        }
        for (index in 0 until entries.length()) {
            val button = entries.optJSONObject(index) ?: continue
            val variant = button.optString("variant")
            val fill = when (variant) {
                "", "primary" -> BRAND
                "tertiary" -> Color.TRANSPARENT
                else -> SECONDARY
            }
            val view = label(activity, button.optString("content"), 14f, Color.WHITE, 1, true).apply {
                gravity = Gravity.CENTER
                background = GradientDrawable().apply {
                    setColor(fill)
                    cornerRadius = dp(activity, 16).toFloat()
                }
                setOnClickListener { entry.event?.invoke("button", (index + 1).toString(), false) }
            }
            row.addView(view, LinearLayout.LayoutParams(0, dp(activity, 32), 1f).apply {
                if (index > 0) marginStart = dp(activity, 5)
            })
        }
        return row
    }

    private fun progress(activity: Activity, spec: JSONObject): View? {
        if (!spec.has("progress") || spec.isNull("progress")) return null
        val fraction = spec.optDouble("progress", 0.0).coerceIn(0.0, 1.0).toFloat()
        val track = LinearLayout(activity).apply { orientation = LinearLayout.HORIZONTAL }
        val bar = View(activity).apply {
            background = GradientDrawable().apply {
                setColor(BRAND)
                cornerRadius = dp(activity, 2).toFloat()
            }
        }
        track.addView(bar, LinearLayout.LayoutParams(0, dp(activity, 3), fraction.coerceAtLeast(0.001f)))
        track.addView(View(activity), LinearLayout.LayoutParams(0, dp(activity, 3), (1f - fraction).coerceAtLeast(0.001f)))
        return track
    }

    private fun schedule(entry: Shown, spec: JSONObject) {
        val generation = ++entry.generation
        if (spec.has("progress") && !spec.isNull("progress")) return
        val duration = if (spec.has("duration")) spec.optLong("duration") else DEFAULT_DURATION_MS
        if (duration <= 0) return
        main.postDelayed({ if (entry.generation == generation) dismiss(entry) }, duration)
    }

    private fun dragToDismiss(activity: Activity, entry: Shown) {
        var start = 0f
        entry.view.setOnTouchListener { view, event ->
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> start = event.rawY
                MotionEvent.ACTION_MOVE -> view.translationY = minOf(0f, event.rawY - start)
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    if (view.translationY < -dp(activity, DISMISS_DRAG_DP)) {
                        dismiss(entry)
                    } else {
                        view.animate().translationY(0f).setDuration(ANIMATION_MS).start()
                    }
                }
            }
            true
        }
    }
}
