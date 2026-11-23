package app.astral.android

import android.app.Application
import io.github.libxposed.api.XposedModule
import io.github.libxposed.api.XposedModuleInterface.PackageReadyParam

class Astral : XposedModule() {
    companion object {
        const val DISCORD_PACKAGE = "com.discord"
        private const val SESSION_ATTEMPTS = 120
        private const val SESSION_RETRY_MS = 5_000L
    }

    override fun onPackageReady(param: PackageReadyParam) {
        if (!param.isFirstPackage || param.packageName != DISCORD_PACKAGE) return
        if (Application.getProcessName() != DISCORD_PACKAGE) return
        runCatching { start(param) }.onFailure { Log.error("Failed to start astral", it) }
    }

    private fun loadSession(dataDirectory: String) {
        Thread({
            repeat(SESSION_ATTEMPTS) {
                val token = runCatching { SessionToken.read(dataDirectory) }
                    .onFailure { Log.warning("Failed to read Discord's session: ${it.javaClass.simpleName}") }
                    .getOrNull()
                if (token != null) {
                    Native.setSessionToken(token)
                    Log.info("Loaded Discord's session")
                    return@Thread
                }
                Thread.sleep(SESSION_RETRY_MS)
            }
        }, "astral-session").apply { isDaemon = true }.start()
    }

    private fun start(param: PackageReadyParam) {
        NativeLibrary.load(moduleApplicationInfo, param.applicationInfo)
        val names = runCatching { PropertyTable.names(param.applicationInfo, param.classLoader) }
            .onFailure { Log.error("Failed to read Phoenix's property table", it) }
            .getOrNull()
        if (names == null) Log.warning("Phoenix's property table was not found; astral uses its built-in property ids")
        val resources = Resources.extract(moduleApplicationInfo, param.applicationInfo)
        val languages = android.os.LocaleList.getDefault().let { list -> Array(list.size()) { list[it].toLanguageTag() } }
        Native.initialize(names, param.applicationInfo.dataDir, resources.absolutePath, languages)
        Capture.configure(param.applicationInfo.dataDir)
        PhoenixBridge.install(this, param.classLoader)
        runCatching { NetworkHooks.install(this, param.classLoader) }.onFailure { Log.error("Failed to hook Discord's network stack", it) }
        loadSession(param.applicationInfo.dataDir)
    }
}
