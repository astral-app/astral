package app.astral.android

import android.app.Activity
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.window.OnBackInvokedCallback
import android.window.OnBackInvokedDispatcher
import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule
import java.lang.ref.WeakReference

object NavigationBack {
    private const val MAIN_ACTIVITY = "com.discord.nativeapp.MainActivity"
    private const val HEALTHY_LAUNCH_DELAY_MS = 30_000L
    private val main = Handler(Looper.getMainLooper())
    private var active = WeakReference<Activity>(null)
    private val markHealthy = Runnable { if (active.get() != null) Native.markReady() }
    private object Predictive {
        var registered = WeakReference<Activity>(null)
        val callback = OnBackInvokedCallback {
            Native.back()
            refresh()
        }
    }

    fun install(module: XposedModule, loader: ClassLoader) {
        val resume = Activity::class.java.getDeclaredMethod("onResume")
        module.hook(resume).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
            val result = chain.proceed()
            val activity = chain.thisObject as Activity
            active = WeakReference(activity)
            refresh()
            if (activity.javaClass.name == MAIN_ACTIVITY) {
                ExitDiagnostics.record(activity)
                Native.markVisible()
                main.removeCallbacks(markHealthy)
                main.postDelayed(markHealthy, HEALTHY_LAUNCH_DELAY_MS)
            }
            result
        }
        val pause = Activity::class.java.getDeclaredMethod("onPause")
        module.hook(pause).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
            if (active.get() === chain.thisObject) {
                active.clear()
                refresh()
            }
            if ((chain.thisObject as? Activity)?.javaClass?.name == MAIN_ACTIVITY) {
                main.removeCallbacks(markHealthy)
                Native.markHidden()
            }
            chain.proceed()
        }
        var type: Class<*>? = loader.loadClass(MAIN_ACTIVITY)
        while (type != null) {
            type.declaredMethods.firstOrNull { it.name == "onBackPressed" && it.parameterTypes.isEmpty() }?.let { method ->
                module.hook(method).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
                    if (Native.back()) null else chain.proceed()
                }
            }
            type = type.superclass
        }
    }

    fun refresh() {
        if (Build.VERSION.SDK_INT < 33) return
        main.post {
            val desired = active.get()?.takeIf { Native.pageOpen() && !it.isFinishing }
            val previous = Predictive.registered.get()
            if (desired === previous) return@post
            previous?.onBackInvokedDispatcher?.unregisterOnBackInvokedCallback(Predictive.callback)
            Predictive.registered = WeakReference(desired)
            desired?.onBackInvokedDispatcher?.registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_OVERLAY, Predictive.callback)
        }
    }
}
