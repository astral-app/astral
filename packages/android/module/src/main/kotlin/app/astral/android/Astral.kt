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

    private fun discordVersion(): String =
        runCatching {
            val thread = Class.forName("android.app.ActivityThread")
            val current = thread.getMethod("currentActivityThread").invoke(null)
            val context = thread.getMethod("getSystemContext").invoke(current) as android.content.Context
            val info = context.packageManager.getPackageInfo(DISCORD_PACKAGE, 0)
            "${info.versionName} · ${info.longVersionCode}"
        }.getOrDefault("")

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
        runCatching { ForcePhoenix.apply(param.applicationInfo.dataDir) }.onFailure { Log.error("Failed to keep Phoenix as Discord's client", it) }
        NativeLibrary.load(moduleApplicationInfo, param.applicationInfo)
        val names = runCatching { PropertyTable.names(param.applicationInfo, param.classLoader) }
            .onFailure { Log.error("Failed to read Phoenix's property table", it) }
            .getOrNull()
        if (names == null) Log.warning("Phoenix's property table was not found; astral uses its built-in property ids")
        val resources = Resources.extract(moduleApplicationInfo, param.applicationInfo)
        val languages = android.os.LocaleList.getDefault().let { list -> Array(list.size()) { list[it].toLanguageTag() } }
        Native.initialize(names, param.applicationInfo.dataDir, resources.absolutePath, languages, discordVersion())
        runCatching { ResourceImages.install(this, param.applicationInfo, resources) }.onFailure { Log.error("Failed to install astral's images", it) }
        PhoenixBridge.pressEvent = names?.indexOf("press") ?: -1
        PhoenixBridge.changeEvent = names?.indexOf("change") ?: -1
        Capture.configure(param.applicationInfo.dataDir)
        PhoenixBridge.install(this, param.classLoader)
        runCatching { NavigationBack.install(this, param.classLoader) }.onFailure { Log.error("Failed to route system back", it) }
        runCatching { CutoutOverlay.install(this, resources) }.onFailure { Log.error("Failed to install the display cutout overlay", it) }
        runCatching { NetworkHooks.install(this, param.classLoader) }.onFailure { Log.error("Failed to hook Discord's network stack", it) }
        loadSession(param.applicationInfo.dataDir)
    }
}
