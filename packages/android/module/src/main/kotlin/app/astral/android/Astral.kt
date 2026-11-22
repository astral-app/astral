package app.astral.android

import android.app.Application
import io.github.libxposed.api.XposedModule
import io.github.libxposed.api.XposedModuleInterface.PackageReadyParam

class Astral : XposedModule() {
    companion object {
        const val DISCORD_PACKAGE = "com.discord"
    }

    override fun onPackageReady(param: PackageReadyParam) {
        if (!param.isFirstPackage || param.packageName != DISCORD_PACKAGE) return
        if (Application.getProcessName() != DISCORD_PACKAGE) return
        runCatching { start(param) }.onFailure { Log.error("Failed to start astral", it) }
    }

    private fun start(param: PackageReadyParam) {
        NativeLibrary.load(moduleApplicationInfo, param.applicationInfo)
        val names = runCatching { PropertyTable.names(param.applicationInfo, param.classLoader) }
            .onFailure { Log.error("Failed to read Phoenix's property table", it) }
            .getOrNull()
        if (names == null) Log.warning("Phoenix's property table was not found; astral uses its built-in property ids")
        Native.initialize(names)
        PhoenixBridge.install(this, param.classLoader)
    }
}
