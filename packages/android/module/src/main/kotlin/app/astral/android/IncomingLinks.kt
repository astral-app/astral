package app.astral.android

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import io.github.libxposed.api.XposedInterface.ExceptionMode
import io.github.libxposed.api.XposedModule

object IncomingLinks {
    private fun claim(intent: Intent?) {
        val url = intent?.dataString ?: return
        if (Native.openLink(url)) intent.data = null
    }

    fun install(module: XposedModule) {
        val create = Activity::class.java.getDeclaredMethod("onCreate", Bundle::class.java)
        module.hook(create).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
            claim((chain.thisObject as Activity).intent)
            chain.proceed()
        }
        val newIntent = Activity::class.java.getDeclaredMethod("onNewIntent", Intent::class.java)
        module.hook(newIntent).setExceptionMode(ExceptionMode.PROTECTIVE).intercept { chain ->
            claim(chain.getArg(0) as? Intent)
            chain.proceed()
        }
    }
}
