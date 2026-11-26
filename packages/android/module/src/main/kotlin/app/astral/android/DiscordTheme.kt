package app.astral.android

import android.app.Activity
import android.content.res.Configuration
import android.os.Build
import android.view.WindowInsetsController

object DiscordTheme {
    fun dark(activity: Activity): Boolean {
        val controller = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) activity.window?.insetsController else null
        if (controller != null) return controller.systemBarsAppearance and WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS == 0
        return activity.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK != Configuration.UI_MODE_NIGHT_NO
    }
}
