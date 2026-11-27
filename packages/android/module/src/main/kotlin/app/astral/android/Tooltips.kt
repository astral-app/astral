package app.astral.android

import android.app.Activity
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.os.Handler
import android.os.Looper
import android.util.TypedValue
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.widget.PopupWindow
import android.widget.TextView
import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule
import java.lang.ref.WeakReference

object Tooltips {
    private const val VISIBLE_MS = 2500L
    private const val GAP_DP = 18f
    private const val BACKGROUND = 0xFA121214.toInt()
    private val main = Handler(Looper.getMainLooper())
    private var lastX = 0f
    private var lastY = 0f
    private var lastActivity = WeakReference<Activity>(null)
    private var shown: PopupWindow? = null

    fun install(module: XposedModule) {
        val dispatch = Activity::class.java.getDeclaredMethod("dispatchTouchEvent", MotionEvent::class.java)
        module.hook(dispatch).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
            val event = chain.getArg(0) as? MotionEvent
            if (event?.actionMasked == MotionEvent.ACTION_DOWN) {
                lastX = event.rawX
                lastY = event.rawY
                lastActivity = WeakReference(chain.thisObject as Activity)
            }
            chain.proceed()
        }
    }

    fun show(text: String) {
        main.post {
            val activity = lastActivity.get() ?: return@post
            if (activity.isFinishing) return@post
            shown?.dismiss()
            val density = activity.resources.displayMetrics.density
            val label = TextView(activity).apply {
                this.text = text
                setTextColor(Color.WHITE)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
                paint.isFakeBoldText = true
                maxLines = 3
                maxWidth = (260 * density).toInt()
                gravity = Gravity.CENTER
                setPadding((12 * density).toInt(), (8 * density).toInt(), (12 * density).toInt(), (8 * density).toInt())
                background = GradientDrawable().apply {
                    setColor(BACKGROUND)
                    cornerRadius = 8 * density
                }
            }
            label.measure(View.MeasureSpec.UNSPECIFIED, View.MeasureSpec.UNSPECIFIED)
            val width = label.measuredWidth
            val height = label.measuredHeight
            val screen = activity.resources.displayMetrics.widthPixels
            val inset = (8 * density).toInt()
            val gap = (GAP_DP * density).toInt()
            val x = (lastX - width / 2f).toInt().coerceIn(inset, (screen - width - inset).coerceAtLeast(inset))
            val above = (lastY - height - gap).toInt()
            val y = if (above > inset * 4) above else (lastY + gap).toInt()
            val popup = PopupWindow(label, width, height, false).apply {
                isTouchable = false
                animationStyle = android.R.style.Animation_Toast
            }
            popup.showAtLocation(activity.window.decorView, Gravity.TOP or Gravity.START, x, y)
            shown = popup
            main.postDelayed({
                if (shown === popup) {
                    popup.dismiss()
                    shown = null
                }
            }, VISIBLE_MS)
        }
    }
}
