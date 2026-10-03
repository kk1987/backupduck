package app.backupduck

import android.app.Instrumentation
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Bundle
import org.json.JSONObject
import java.io.File
import java.util.UUID

/** Uses a separate receiver root and port; never reads or modifies production records. */
internal fun Instrumentation.checkReceiverWifi(arguments: Bundle): String {
    check(targetContext.packageName.endsWith(".validation"))
    val manager = targetContext.getSystemService(ConnectivityManager::class.java)
    if (arguments.getString("require_vpn") == "true") {
        check(manager.getNetworkCapabilities(manager.activeNetwork)?.hasTransport(NetworkCapabilities.TRANSPORT_VPN) == true) { "vpn_not_active" }
    }
    val address = ReceiverWifiAddress.read(manager)
    val root = File(targetContext.filesDir, "wifi-${UUID.randomUUID()}").apply { mkdirs() }
    try {
        val pairing = NativeBridge.request(JSONObject().put("op", "start_receiver")
            .put("root", File(root, "receiver").path).put("listen", "$address:38584").put("capacity", 1024 * 1024)) as JSONObject
        NativeBridge.request(JSONObject().put("op", "open_sender").put("root", File(root, "sender").path))
        NativeBridge.request(JSONObject().put("op", "check_pairing").put("pairing", pairing))
        sendStatus(0, Bundle().apply { putString("result", "READY: physical Wi-Fi listener $address:38584; authenticated TLS probe passed") })
        Thread.sleep(45_000)
        check(ReceiverWifiAddress.read(manager, address) == address) { "wifi_address_changed" }
        return "PASS: physical Wi-Fi address, authenticated TLS and stable receiver under current VPN policy"
    } finally {
        runCatching { NativeBridge.request(JSONObject().put("op", "stop_receiver")) }
        root.deleteRecursively()
    }
}
