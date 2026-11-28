package app.astral.android

import android.app.Activity
import android.app.PictureInPictureParams
import android.graphics.Rect
import android.util.Rational
import android.view.SurfaceView
import android.view.TextureView
import android.view.View
import android.view.ViewGroup
import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule

object PictureInPicture {
    private const val MAX_RATIO = 2.39
    private const val DEFAULT_WIDTH = 16
    private const val DEFAULT_HEIGHT = 9
    private const val ACTIVITY = "com.discord.nativeapp.MainActivity"

    @Volatile private var ours = false

    fun install(module: XposedModule, loader: ClassLoader) {
        val activity = loader.loadClass(ACTIVITY)
        for (method in activity.declaredMethods.filter { it.name == "onPictureInPictureModeChanged" }) {
            module.hook(method).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
                if (!ours) return@intercept chain.proceed()
                if (chain.getArg(0) == false) ours = false
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
