package app.backupduck

import android.content.Context

/** Receiver-local opt-in to re-encode HEIC burst frames; lossless HEIC is the default. */
internal object BurstDeliverySettings {
    private const val preferences = "burst_delivery"
    private const val key = "convert_heic_bursts_to_jpeg"

    fun convertHeicToJpeg(context: Context): Boolean =
        context.getSharedPreferences(preferences, Context.MODE_PRIVATE).getBoolean(key, false)

    fun setConvertHeicToJpeg(context: Context, enabled: Boolean) {
        context.getSharedPreferences(preferences, Context.MODE_PRIVATE).edit().putBoolean(key, enabled).apply()
    }
}
