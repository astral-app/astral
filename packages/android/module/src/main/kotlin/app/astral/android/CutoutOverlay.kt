package app.astral.android

import android.app.Activity
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.ImageView
import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule
import com.caverock.androidsvg.SVG
import java.io.File
import java.lang.ref.WeakReference
import kotlin.math.min

object CutoutOverlay {
    private const val REVEAL_DELAY_MS = 1000L
    private const val REVEAL_MS = 200L
    private const val LOGO_ASPECT = 191.19 / 152.94
    private val main = Handler(Looper.getMainLooper())
    private var active = WeakReference<Activity>(null)
    private var owner = WeakReference<Activity>(null)
    private var overlay: View? = null
    private lateinit var logo: File

    fun install(module: XposedModule, resources: File) {
        logo = File(resources, "assets/galaxy.svg")
        val resume = Activity::class.java.getDeclaredMethod("onResume")
        module.hook(resume).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
            val result = chain.proceed()
            val activity = chain.thisObject as Activity
            active = WeakReference(activity)
            main.postDelayed({ show(activity) }, REVEAL_DELAY_MS)
            result
        }
        val pause = Activity::class.java.getDeclaredMethod("onPause")
        module.hook(pause).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
            val activity = chain.thisObject as Activity
            if (active.get() === activity) active.clear()
            main.post { hide(activity) }
            chain.proceed()
        }
    }

    private fun show(activity: Activity) {
        if (active.get() !== activity || activity.isFinishing || Build.VERSION.SDK_INT < 28) return
        val decor = activity.window.decorView as? ViewGroup ?: return
        val rect = decor.rootWindowInsets?.displayCutout?.boundingRects?.firstOrNull { it.width() > 0 && it.height() > 0 } ?: return
        val width = rect.width().toFloat()
        val height = rect.height().toFloat()
        val island = width > height && rect.top > 0
        val notch = width > height && rect.top <= 0
        val corner = when {
            notch -> height * 0.65f
            island -> height / 2
            else -> min(width, height) / 2
        }
        val logoHeight = min(width, height) * if (notch || island) 0.8f else 0.6f
        val logoWidth = min(logoHeight * LOGO_ASPECT.toFloat(), width)
        val logoFrame = FrameLayout.LayoutParams(logoWidth.toInt().coerceAtLeast(1), (logoWidth / LOGO_ASPECT).toInt().coerceAtLeast(1), android.view.Gravity.CENTER)
        val container = if (owner.get() === activity) overlay as? FrameLayout else null
        if (container == null || container.parent !== decor) {
            (overlay?.parent as? ViewGroup)?.removeView(overlay)
            val root = FrameLayout(activity)
            root.isClickable = false
            root.isFocusable = false
            root.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            root.background = GradientDrawable().apply {
                setColor(Color.BLACK)
                cornerRadii = floatArrayOf(0f, 0f, 0f, 0f, corner, corner, corner, corner)
            }
            val bitmap = readLogo(logoWidth.toInt().coerceAtLeast(1), (logoWidth / LOGO_ASPECT).toInt().coerceAtLeast(1)) ?: return
            val image = ImageView(activity).apply {
                scaleType = ImageView.ScaleType.FIT_CENTER
                isClickable = false
                isFocusable = false
                setImageBitmap(bitmap)
            }
            root.addView(image, logoFrame)
            root.alpha = 0f
            decor.addView(root, FrameLayout.LayoutParams(rect.width(), rect.height()).apply {
                leftMargin = rect.left
                topMargin = rect.top
            })
            owner = WeakReference(activity)
            overlay = root
        }
        val current = overlay ?: return
        (current.layoutParams as? FrameLayout.LayoutParams)?.let { params ->
            params.width = rect.width()
            params.height = rect.height()
            params.leftMargin = rect.left
            params.topMargin = rect.top
            current.layoutParams = params
        }
        current.visibility = View.VISIBLE
        current.bringToFront()
        current.animate().cancel()
        current.animate().alpha(1f).setDuration(REVEAL_MS).start()
    }

    private fun readLogo(width: Int, height: Int): Bitmap? = runCatching {
        val picture = logo.inputStream().buffered().use { SVG.getFromInputStream(it).renderToPicture(width, height) }
        Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888).also { bitmap ->
            Canvas(bitmap).drawPicture(picture)
        }
    }.getOrNull()

    private fun hide(activity: Activity) {
        if (owner.get() !== activity) return
        overlay?.animate()?.cancel()
        overlay?.alpha = 0f
        overlay?.visibility = View.GONE
    }
}
