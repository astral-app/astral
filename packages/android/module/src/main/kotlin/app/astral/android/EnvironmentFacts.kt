package app.astral.android

import android.app.ActivityManager
import android.app.UiModeManager
import android.app.usage.UsageStatsManager
import android.content.Context
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.os.Build
import android.os.PowerManager
import android.os.Process
import android.os.UserManager
import android.provider.Settings
import android.text.TextUtils
import android.view.View
import android.view.WindowManager
import android.webkit.WebView
import org.json.JSONArray
import org.json.JSONObject
import java.text.DecimalFormatSymbols
import java.util.Locale

object EnvironmentFacts {
    private const val CHROME_OS_FEATURE = "org.chromium.arc"
    private const val PLAY_GAMES_PC_FEATURE = "com.google.android.play.feature.HPE_EXPERIENCE"
    private const val UNFOLDED_WIDTH_DP = 600
    private const val FONT_WEIGHT_ADJUSTMENT = "font_weight_adjustment"
    private const val HARMONY_BUILD_CLASS = "com.huawei.system.BuildEx"
    private const val HARMONY_BRAND = "harmony"

    @Volatile
    var framework = ""

    private fun application(): Context? = runCatching {
        Class.forName("android.app.ActivityThread").getMethod("currentApplication").invoke(null) as? Context
    }.getOrNull()

    @JvmStatic
    fun collect(): String = runCatching {
        val context = application() ?: return "{}"
        JSONObject()
            .put("install", install(context))
            .put("device", device(context))
            .put("framework", framework)
            .put("device_name", "${Build.MANUFACTURER} ${Build.MODEL}".trim())
            .toString()
    }.onFailure { Log.error("Failed to read the environment facts", it) }.getOrDefault("{}")

    private fun install(context: Context): JSONObject = JSONObject()
        .put("splitNames", JSONArray(context.applicationInfo.splitNames?.toList() ?: emptyList<String>()))
        .put("sourceDir", context.applicationInfo.sourceDir ?: "")
        .put("profile", profile(context))

    private fun device(context: Context): JSONObject = JSONObject()
        .put("selinuxFallback", selinuxFallback() ?: JSONObject.NULL)
        .put("webView", webView(context))
        .put("hostFeature", hostFeature(context))
        .put("virtualHost", virtualHost(context) ?: "")
        .put("background", background(context))
        .put("lowRam", context.getSystemService(ActivityManager::class.java)?.isLowRamDevice == true)
        .put("harmony", isHarmonyOS())
        .put("refreshRate", refreshRate(context))
        .put("boldText", isBoldText(context))
        .put("formFactor", formFactor(context))
        .put("duplicateCopy", "")
        .put("locale", localeFacts())

    private fun formFactor(context: Context): String = runCatching {
        when (context.getSystemService(UiModeManager::class.java)?.currentModeType) {
            Configuration.UI_MODE_TYPE_TELEVISION -> "Android TV"
            Configuration.UI_MODE_TYPE_WATCH -> "Wear OS"
            Configuration.UI_MODE_TYPE_CAR -> "Android Automotive"
            else -> foldState(context)
        }
    }.getOrDefault("")

    private fun foldState(context: Context): String {
        if (!context.packageManager.hasSystemFeature(PackageManager.FEATURE_SENSOR_HINGE_ANGLE)) return ""

        val unfolded = context.resources.configuration.smallestScreenWidthDp >= UNFOLDED_WIDTH_DP

        return if (unfolded) "foldable, unfolded" else "foldable, folded"
    }

    private fun isHarmonyOS(): Boolean = runCatching {
        Class.forName(HARMONY_BUILD_CLASS).getMethod("getOsBrand").invoke(null) == HARMONY_BRAND
    }.getOrDefault(false)

    @Suppress("DEPRECATION")
    private fun refreshRate(context: Context): Int = runCatching {
        context.getSystemService(WindowManager::class.java)?.defaultDisplay?.refreshRate?.toInt() ?: 0
    }.getOrDefault(0)

    private fun isBoldText(context: Context): Boolean = runCatching {
        Build.VERSION.SDK_INT >= 31 && Settings.Secure.getInt(context.contentResolver, FONT_WEIGHT_ADJUSTMENT, 0) > 0
    }.getOrDefault(false)

    private fun localeFacts(): JSONObject = runCatching {
        val locale = Locale.getDefault()
        JSONObject()
            .put("calendar", android.icu.util.Calendar.getInstance(locale).type)
            .put("nonLatinDigits", DecimalFormatSymbols.getInstance(locale).zeroDigit != '0')
            .put("rtl", TextUtils.getLayoutDirectionFromLocale(locale) == View.LAYOUT_DIRECTION_RTL)
    }.getOrDefault(JSONObject())

    private fun hostFeature(context: Context): String {
        val packages = context.packageManager
        return when {
            packages.hasSystemFeature(PLAY_GAMES_PC_FEATURE) -> "Play Games PC"
            packages.hasSystemFeature(CHROME_OS_FEATURE) -> "ChromeOS"
            else -> ""
        }
    }

    private fun virtualHost(context: Context): String? = runCatching {
        val packages = context.packageManager
        val owner = packages.getNameForUid(Process.myUid())?.substringBefore(':')
        val dataDir = context.applicationInfo.dataDir.orEmpty()
        when {
            owner != null && owner != context.packageName -> "inside ${labelOf(packages, owner)}"
            dataDir.isNotEmpty() && !isStandardDataDir(dataDir) -> "virtual storage"
            else -> null
        }
    }.getOrNull()

    private fun labelOf(packages: PackageManager, name: String): String = runCatching {
        packages.getApplicationLabel(packages.getApplicationInfo(name, 0)).toString()
    }.getOrNull()?.takeIf { it.isNotBlank() } ?: name

    internal fun isStandardDataDir(path: String): Boolean =
        path.startsWith("/data/user/") || path.startsWith("/data/data/") || path.startsWith("/data/user_de/") ||
            path.startsWith("/mnt/expand/")

    private fun background(context: Context): JSONObject = runCatching {
        val power = context.getSystemService(PowerManager::class.java)
        val activity = context.getSystemService(ActivityManager::class.java)
        val usage = context.getSystemService(UsageStatsManager::class.java)
        JSONObject()
            .put("optimized", power?.isIgnoringBatteryOptimizations(context.packageName) == false)
            .put("restricted", Build.VERSION.SDK_INT >= 28 && activity?.isBackgroundRestricted == true)
            .put("bucket", if (Build.VERSION.SDK_INT >= 28) usage?.appStandbyBucket ?: 0 else 0)
    }.getOrDefault(JSONObject())

    private fun profile(context: Context): String {
        val users = context.getSystemService(UserManager::class.java)
        return when {
            isPrivateProfile(users) -> "Private Space"
            runCatching { Build.VERSION.SDK_INT >= 30 && users?.isManagedProfile == true }.getOrDefault(false) -> "work profile"
            else -> ""
        }
    }

    private fun isPrivateProfile(users: UserManager?): Boolean = runCatching {
        users != null && UserManager::class.java.getMethod("isPrivateProfile").invoke(users) == true
    }.getOrDefault(false)

    private fun selinuxFallback(): Boolean? = runCatching {
        Class.forName("android.os.SELinux").getMethod("isSELinuxEnforced").invoke(null) as? Boolean
    }.getOrNull()

    private fun webView(context: Context): String = runCatching {
        val webView = WebView.getCurrentWebViewPackage() ?: return ""
        val major = webView.versionName?.substringBefore('.') ?: ""
        listOf(webView.packageName, major).filter { it.isNotBlank() }.joinToString(" ")
    }.getOrDefault("")
}
