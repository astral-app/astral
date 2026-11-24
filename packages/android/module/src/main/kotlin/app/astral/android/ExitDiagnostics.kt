package app.astral.android

import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.content.Context
import android.os.Build
import java.util.concurrent.atomic.AtomicBoolean

object ExitDiagnostics {
    private const val DISCORD_PACKAGE = "com.discord"
    private const val MAXIMUM_EXITS = 20
    private const val MAXIMUM_TRACE_BYTES = 8192
    private val recorded = AtomicBoolean(false)

    private fun anrTrace(exit: ApplicationExitInfo): String =
        runCatching { exit.traceInputStream?.use { input -> input.readNBytes(MAXIMUM_TRACE_BYTES).toString(Charsets.UTF_8) } }
            .getOrNull().orEmpty()

    fun record(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R || !recorded.compareAndSet(false, true)) return
        val manager = context.getSystemService(ActivityManager::class.java) ?: return
        val exits = runCatching { manager.getHistoricalProcessExitReasons(DISCORD_PACKAGE, 0, MAXIMUM_EXITS) }
            .onFailure { Log.error("Failed to read Discord's exit history", it) }
            .getOrNull().orEmpty()
        for (exit in exits) {
            val kind = when (exit.reason) {
                ApplicationExitInfo.REASON_CRASH, ApplicationExitInfo.REASON_CRASH_NATIVE -> "crash"
                ApplicationExitInfo.REASON_ANR -> "hang"
                else -> continue
            }
            val trace = if (exit.reason == ApplicationExitInfo.REASON_ANR) anrTrace(exit) else ""
            Native.recordExitReport(kind, exit.timestamp / 1000, exit.reason, exit.description.orEmpty(), trace)
        }
    }
}
