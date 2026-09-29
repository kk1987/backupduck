package app.photobridge

import android.content.Context

/** Receiver-local consent for a lossy fallback; the direct writer always runs first. */
internal object MotionConversionSettings {
    private const val preferences = "motion_conversion"
    private const val key = "allow_compatibility_conversion"

    fun enabled(context: Context): Boolean =
        context.getSharedPreferences(preferences, Context.MODE_PRIVATE).getBoolean(key, false)

    fun setEnabled(context: Context, enabled: Boolean) {
        context.getSharedPreferences(preferences, Context.MODE_PRIVATE).edit().putBoolean(key, enabled).apply()
    }
}
