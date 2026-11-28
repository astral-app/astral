package app.astral.android

import android.app.Activity
import android.app.PictureInPictureParams
import android.graphics.Rect
import android.util.Rational
import android.view.SurfaceView
import android.view.TextureView
import android.view.View
import android.view.ViewGroup
import android.view.ViewTreeObserver
import java.lang.ref.WeakReference
import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule

object PictureInPicture {
    private const val MAX_RATIO = 2.39
    private const val DEFAULT_WIDTH = 16
    private const val DEFAULT_HEIGHT = 9
    private const val ACTIVITY = "com.discord.nativeapp.MainActivity"
    private const val MIN_IMAGE_SHARE = 0.2

    @Volatile private var ours = false
    private var zoomed: WeakReference<Activity>? = null
    private var zoomListener: ViewTreeObserver.OnGlobalLayoutListener? = null

    fun install(module: XposedModule, loader: ClassLoader) {
        val activity = loader.loadClass(ACTIVITY)
        for (method in activity.declaredMethods.filter { it.name == "onPictureInPictureModeChanged" }) {
            module.hook(method).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
                if (!ours) return@intercept chain.proceed()
                val activity = chain.thisObject as? Activity
                if (chain.getArg(0) == true) {
                    activity?.let(::zoomToVideo)
                    Native.emitEvent("picture_in_picture.start")
                } else {
                    ours = false
                    clearZoom()
                    Native.emitEvent("picture_in_picture.stop")
                }
                null
            }
        }
    }

    fun enter(activity: Activity, width: Long, height: Long): Boolean {
        val video = largestVideo(activity.window.decorView)
        val (ratioWidth, ratioHeight) = when {
            width > 0 && height > 0 -> width.toInt() to height.toInt()
            video != null -> video.width to video.height
            else -> DEFAULT_WIDTH to DEFAULT_HEIGHT
        }
        val builder = PictureInPictureParams.Builder().setAspectRatio(clamped(ratioWidth, ratioHeight))
        video?.let { view ->
            val bounds = Rect()
            if (view.getGlobalVisibleRect(bounds)) builder.setSourceRectHint(bounds)
        }
        ours = true
        return runCatching { activity.enterPictureInPictureMode(builder.build()) }
            .onSuccess { entered -> if (!entered) ours = false }
            .onFailure { Log.warning("Failed to enter picture in picture: ${it.javaClass.simpleName}") }
            .getOrDefault(false)
    }

    private fun content(activity: Activity): ViewGroup? = activity.findViewById(android.R.id.content)

    private fun zoomToVideo(activity: Activity) {
        clearZoom()
        val root = content(activity) ?: return
        val listener = ViewTreeObserver.OnGlobalLayoutListener { fit(root) }
        root.viewTreeObserver.addOnGlobalLayoutListener(listener)
        zoomed = WeakReference(activity)
        zoomListener = listener
        fit(root)
    }

    private fun fit(root: ViewGroup) {
        val target = largestVideo(root) ?: largestImage(root) ?: return
        if (target.width <= 0 || root.width <= 0) return
        var left = 0f
        var top = 0f
        var view: View = target
        while (view !== root) {
            left += view.left - view.scrollX + view.translationX
            top += view.top - view.scrollY + view.translationY
            view = view.parent as? View ?: return
        }
        val scale = minOf(root.width.toFloat() / target.width, root.height.toFloat() / target.height)
        root.pivotX = 0f
        root.pivotY = 0f
        root.scaleX = scale
        root.scaleY = scale
        root.translationX = -left * scale + (root.width - target.width * scale) / 2
        root.translationY = -top * scale + (root.height - target.height * scale) / 2
    }

    private fun clearZoom() {
        val activity = zoomed?.get()
        val root = activity?.let(::content)
        zoomListener?.let { listener -> root?.viewTreeObserver?.removeOnGlobalLayoutListener(listener) }
        root?.apply {
            scaleX = 1f
            scaleY = 1f
            translationX = 0f
            translationY = 0f
        }
        zoomed = null
        zoomListener = null
    }

    private fun largestImage(root: ViewGroup): View? {
        var largest: View? = null
        val pending = ArrayDeque<View>().apply { add(root) }
        while (pending.isNotEmpty()) {
            val view = pending.removeFirst()
            if (view.visibility != View.VISIBLE) continue
            if (view is android.widget.ImageView && view.width * view.height > (largest?.let { it.width * it.height } ?: 0)) largest = view
            if (view is ViewGroup) for (index in 0 until view.childCount) pending.add(view.getChildAt(index))
        }
        val share = (largest?.let { it.width.toDouble() * it.height } ?: 0.0) / (root.width.toDouble() * root.height).coerceAtLeast(1.0)
        return largest?.takeIf { share >= MIN_IMAGE_SHARE }
    }

    private fun clamped(width: Int, height: Int): Rational {
        val ratio = (width.toDouble() / height.coerceAtLeast(1)).coerceIn(1 / MAX_RATIO, MAX_RATIO)
        return Rational((ratio * 1000).toInt(), 1000)
    }

    private fun largestVideo(root: View): View? {
        var largest: View? = null
        val pending = ArrayDeque<View>().apply { add(root) }
        while (pending.isNotEmpty()) {
            val view = pending.removeFirst()
            if (view.visibility != View.VISIBLE) continue
            if ((view is TextureView || view is SurfaceView) && view.width > 0 && view.height > 0) {
                if (largest == null || view.width * view.height > largest.width * largest.height) largest = view
            }
            if (view is ViewGroup) for (index in 0 until view.childCount) pending.add(view.getChildAt(index))
        }
        return largest
    }
}
