package app.backupduck

import android.text.format.Formatter
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.lifecycle.lifecycleScope
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import com.google.android.material.materialswitch.MaterialSwitch
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** Opt-in cloud-verified release. Native settings gate every destructive step. */
internal class CloudReleaseControls(private val activity: MainActivity, parent: LinearLayout, private val changed: () -> Unit) {
    private var applying = false
    private var saving = false
    private var graceMs = 3_600_000L
    private val toggle = MaterialSwitch(activity).apply {
        setText(R.string.cloud_release_toggle); minimumHeight = activity.dp(56)
    }
    private val counts: TextView
    private val early = MaterialSwitch(activity).apply {
        setText(R.string.early_release_toggle); minimumHeight = activity.dp(56)
    }
    init {
        parent.addView(toggle)
        activity.label(parent, activity.getString(R.string.cloud_release_note), 14, activity.secondaryColor())
        counts = activity.label(parent, "", 13, activity.secondaryColor())
        parent.addView(early)
        activity.label(parent, activity.getString(R.string.early_release_note), 14, activity.secondaryColor())
        early.setOnCheckedChangeListener { _, checked ->
            if (!applying && !saving) saveSetting("release_originals_on_publication", checked) { renderEarly(!checked) }
        }
        toggle.setOnCheckedChangeListener { _, checked ->
            if (!applying && !saving) {
                if (checked) {
                    render(false)
                    MaterialAlertDialogBuilder(activity).setTitle(R.string.cloud_release_confirm_title)
                        .setMessage(activity.getString(R.string.cloud_release_confirm_note, (graceMs / 60_000L).toInt()))
                        .setNegativeButton(R.string.receiver_close, null)
                        .setPositiveButton(R.string.cloud_release_enable) { _, _ -> save(true) }.show()
                } else save(false)
            }
        }
    }
    /** [settings] and [counts] come from `receiver_settings`. */
    fun render(settings: JSONObject, counts: JSONObject) {
        graceMs = settings.optLong("cloud_release_grace_ms", graceMs)
        render(settings.optBoolean("cloud_release", false))
        renderEarly(settings.optBoolean("release_originals_on_publication", false))
        this.counts.text = activity.getString(R.string.cloud_release_counts, counts.optInt("cloud_verified"),
            counts.optInt("gallery_released"), Formatter.formatFileSize(activity, counts.optLong("gallery_released_bytes")))
    }
    private fun render(enabled: Boolean) {
        if (saving) return
        applying = true; toggle.isChecked = enabled; applying = false
    }
    private fun renderEarly(enabled: Boolean) {
        if (saving) return
        applying = true; early.isChecked = enabled; applying = false
    }
    /** Saves one receiver setting; [revert] restores the switch on failure. */
    private fun saveSetting(key: String, value: Boolean, revert: () -> Unit) {
        saving = true; early.isEnabled = false
        activity.lifecycleScope.launch {
            val result = withContext(Dispatchers.IO) { runCatching {
                val root = "${activity.filesDir}/receiver"
                val current = NativeBridge.request(JSONObject().put("op", "receiver_settings").put("root", root)) as JSONObject
                val settings = current.getJSONObject("settings").put(key, value)
                NativeBridge.request(JSONObject().put("op", "receiver_settings").put("root", root).put("settings", settings))
            } }
            saving = false; early.isEnabled = true
            if (result.isFailure) {
                revert()
                Toast.makeText(activity, R.string.settings_failed, Toast.LENGTH_LONG).show()
            } else changed()
        }
    }
    private fun save(enabled: Boolean) {
        saving = true; toggle.isEnabled = false
        activity.lifecycleScope.launch {
            val result = withContext(Dispatchers.IO) { runCatching {
                val root = "${activity.filesDir}/receiver"
                val current = NativeBridge.request(JSONObject().put("op", "receiver_settings").put("root", root)) as JSONObject
                val settings = current.getJSONObject("settings").put("cloud_release", enabled)
                NativeBridge.request(JSONObject().put("op", "receiver_settings").put("root", root).put("settings", settings))
            } }
            saving = false; toggle.isEnabled = true
            render(if (result.isSuccess) enabled else !enabled)
            Toast.makeText(activity, if (result.isSuccess) (if (enabled) R.string.cloud_release_enabled else R.string.cloud_release_disabled) else R.string.settings_failed, Toast.LENGTH_LONG).show()
            if (result.isSuccess) changed()
        }
    }
}
