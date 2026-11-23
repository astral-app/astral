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
    private val main = Handler(Looper.getMainLooper())
    private var active = WeakReference<Activity>(null)
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
            active = WeakReference(chain.thisObject as Activity)
            refresh()
            result
        }
        val pause = Activity::class.java.getDeclaredMethod("onPause")
        module.hook(pause).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
            if (active.get() === chain.thisObject) {
                active.clear()
                refresh()
            }
            chain.proceed()
        }
        var type: Class<*>? = loader.loadClass("com.discord.nativeapp.MainActivity")
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
