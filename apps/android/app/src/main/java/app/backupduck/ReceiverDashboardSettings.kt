package app.backupduck

import android.content.Context

/** Explicit, receiver-local control; no browser listener exists by default. */
internal object ReceiverDashboardSettings {
    private fun prefs(context: Context) = context.getSharedPreferences("receiver_dashboard", Context.MODE_PRIVATE)
    fun enabled(context: Context) = prefs(context).getBoolean("enabled", false)
    fun setEnabled(context: Context, enabled: Boolean) {
        prefs(context).edit().putBoolean("enabled", enabled).apply()
    }
}
