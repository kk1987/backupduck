package app.backupduck

import android.app.Instrumentation
import android.content.Intent
import android.view.accessibility.AccessibilityNodeInfo
import org.json.JSONObject

internal fun Instrumentation.checkDashboardCodeUI(): String {
    check(targetContext.packageName.endsWith(".validation"))
    ReceiverPreferences.setEnabled(targetContext, false)
    val root = "${targetContext.filesDir}/receiver"
    fun access(code: String? = null): JSONObject = NativeBridge.request(JSONObject().put("op", "dashboard_access").put("root", root).put("code", code)) as JSONObject
    val original = access().getString("code")
    val activity = startActivitySync(Intent(targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
    try {
        runOnMainSync { activity.manageDashboardCode() }
        waitForIdleSync(); Thread.sleep(700)
        fun edits(node: AccessibilityNodeInfo): List<AccessibilityNodeInfo> = (if (node.className?.toString()?.endsWith("EditText") == true) listOf(node) else emptyList()) + (0 until node.childCount).flatMap { i -> node.getChild(i)?.let(::edits) ?: emptyList() }
        val edit = edits(uiAutomation.rootInActiveWindow).single()
        check(edit.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, android.os.Bundle().apply { putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, "0123456789") }))
        val save = uiAutomation.rootInActiveWindow.findAccessibilityNodeInfosByText(activity.getString(R.string.settings_save)).first { it.isClickable }
        check(save.performAction(AccessibilityNodeInfo.ACTION_CLICK))
        var saved = false
        repeat(50) { if (!saved) { saved = access().getString("code") == "0123456789"; Thread.sleep(100) } }
        check(saved) { "custom_dashboard_code_not_saved" }
        check(receiverProblem(ReceiverSnapshot(error="receiver_port_in_use"))?.message == R.string.recovery_port)
        return "PASS: custom access code saved from native settings; port-conflict recovery is specific"
    } finally { access(original); runOnMainSync { activity.finish() } }
}
