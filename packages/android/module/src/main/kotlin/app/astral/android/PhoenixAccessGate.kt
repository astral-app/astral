package app.astral.android

import android.content.pm.ApplicationInfo

object PhoenixAccessGate {
    fun install(app: ApplicationInfo, loader: ClassLoader) {
        val sources = arrayOf(app.sourceDir, *(app.splitSourceDirs ?: emptyArray()))
        val prepared = Native.preparePhoenixAccess(app.nativeLibraryDir.orEmpty(), sources)
        check(prepared == "ready") { prepared }
        Class.forName("com.discord.nativeapp.bridge.WasmRuntime", true, loader)
        val result = Native.openPhoenixAccess()
        Log.info("Phoenix access: $result")
        check(result == "opened" || result == "already-open") { result }
    }
}
