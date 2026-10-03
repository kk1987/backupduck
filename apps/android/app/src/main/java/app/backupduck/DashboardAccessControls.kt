package app.backupduck

import android.text.InputType
import android.view.WindowManager
import android.widget.EditText
import android.widget.Toast
import androidx.lifecycle.lifecycleScope
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

internal fun MainActivity.manageDashboardCode() {
    val edit = EditText(this).apply {
        hint = getString(R.string.dashboard_custom_hint)
        inputType = InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD
        filters = arrayOf(android.text.InputFilter.LengthFilter(10))
    }
    val dialog = MaterialAlertDialogBuilder(this).setTitle(R.string.dashboard_manage_code)
        .setMessage(R.string.dashboard_code_note).setView(edit)
        .setNegativeButton(R.string.receiver_close, null)
        .setNeutralButton(R.string.dashboard_reset_code) { _, _ ->
            MaterialAlertDialogBuilder(this).setTitle(R.string.dashboard_reset_code).setMessage(R.string.dashboard_reset_note)
                .setNegativeButton(R.string.receiver_close, null).setPositiveButton(R.string.dashboard_reset_code) { _, _ -> changeDashboardAccess(reset = true) }.show()
        }
        .setPositiveButton(R.string.settings_save, null).create()
    dialog.setOnShowListener {
        dialog.window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        dialog.getButton(android.content.DialogInterface.BUTTON_POSITIVE).setOnClickListener {
            val code = edit.text.toString()
            if (!code.matches(Regex("[0-9]{10}"))) edit.error = getString(R.string.dashboard_custom_hint)
            else { dialog.dismiss(); changeDashboardAccess(code = code) }
        }
    }
    dialog.show()
}
internal fun MainActivity.revokeDashboardLogins() {
    MaterialAlertDialogBuilder(this).setTitle(R.string.dashboard_revoke).setMessage(R.string.dashboard_revoke_note)
        .setNegativeButton(R.string.receiver_close, null).setPositiveButton(R.string.dashboard_revoke) { _, _ -> changeDashboardAccess(revoke = true) }.show()
}
private fun MainActivity.changeDashboardAccess(code: String? = null, reset: Boolean = false, revoke: Boolean = false) {
    lifecycleScope.launch {
        val result = withContext(Dispatchers.IO) { runCatching {
            NativeBridge.request(JSONObject().put("op", "dashboard_access").put("root", "$filesDir/receiver")
                .put("code", code).put("reset", reset).put("revoke", revoke))
        } }
        Toast.makeText(this@changeDashboardAccess, if (result.isSuccess) R.string.dashboard_access_saved else R.string.settings_failed, Toast.LENGTH_LONG).show()
    }
}
