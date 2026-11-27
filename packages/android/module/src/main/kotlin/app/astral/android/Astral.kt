package app.astral.android

import android.app.Application
import android.os.Build
import io.github.libxposed.api.XposedModule
import io.github.libxposed.api.XposedModuleInterface.PackageReadyParam
import org.json.JSONObject
import java.util.Locale

class Astral : XposedModule() {
    companion object {
        const val DISCORD_PACKAGE = "com.discord"
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

    private fun start(param: PackageReadyParam) {
        EnvironmentFacts.framework = runCatching { "$frameworkName $frameworkVersion".trim() }.getOrDefault("")
        NativeLibrary.load(moduleApplicationInfo, param.applicationInfo)
        runCatching { ForcePhoenix.apply(param.applicationInfo.dataDir) }.onFailure { Log.error("Failed to keep Phoenix as Discord's client", it) }
        val names = runCatching { PropertyTable.names(param.applicationInfo, param.classLoader) }
            .onFailure { Log.error("Failed to read Phoenix's property table", it) }
            .getOrNull()
        if (names == null) Log.warning("Phoenix's property table was not found; astral uses its built-in property ids")
        val resources = Resources.extract(moduleApplicationInfo, param.applicationInfo)
        val languages = android.os.LocaleList.getDefault().let { list -> Array(list.size()) { list[it].toLanguageTag() } }
        val environment = JSONObject()
            .put("schema", 2)
            .put("platform", "android")
            .put("install_mode", "injected")
            .put("device", "${Build.MANUFACTURER} ${Build.MODEL}".trim())
            .put("system", "Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})")
            .put("os_build", Build.ID)
            .put("architecture", Build.SUPPORTED_ABIS.firstOrNull().orEmpty())
            .put("abi", Build.SUPPORTED_ABIS.firstOrNull().orEmpty())
            .put("locale", Locale.getDefault().toLanguageTag())
            .toString()
        Native.attachBridge(PhoenixBridge::class.java)
        Native.initialize(names, param.applicationInfo.dataDir, resources.absolutePath, languages, discordVersion(), environment)
        runCatching { ResourceImages.install(this, param.applicationInfo, resources) }.onFailure { Log.error("Failed to install astral's images", it) }
        Capture.configure(param.applicationInfo.dataDir)
        PhoenixBridge.install(this, param.classLoader)
        runCatching { EmojiComposer.install(this) }.onFailure { Log.error("Failed to install emoji composer support", it) }
        runCatching { NavigationBack.install(this, param.classLoader) }.onFailure { Log.error("Failed to route system back", it) }
        runCatching { CutoutOverlay.install(this, resources) }.onFailure { Log.error("Failed to install the display cutout overlay", it) }
        runCatching { PhoenixClasses.resolve(param.applicationInfo, param.classLoader) }.onFailure { Log.error("Failed to find Phoenix's OkHttp classes", it) }
        runCatching { NetworkHooks.install(this, param.classLoader) }.onFailure { Log.error("Failed to hook Discord's network stack", it) }
    }
}
