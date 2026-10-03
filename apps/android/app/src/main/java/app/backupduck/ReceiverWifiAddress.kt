package app.backupduck

import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import java.net.Inet4Address
import java.net.InetAddress

internal data class ReceiverNetworkAddress(
    val wifi: Boolean,
    val vpn: Boolean,
    val addresses: List<InetAddress>,
    val active: Boolean = false,
)

/** A VPN can advertise Wi-Fi as its underlying transport but owns a tunnel address. */
internal fun selectReceiverWifiAddress(networks: List<ReceiverNetworkAddress>, preferred: String? = null): String? {
    val addresses = networks.filter { it.wifi && !it.vpn }.sortedByDescending { it.active }
        .flatMap { it.addresses }.filterIsInstance<Inet4Address>()
        .filter { !it.isAnyLocalAddress && !it.isLoopbackAddress && !it.isLinkLocalAddress && !it.isMulticastAddress }
        .mapNotNull { it.hostAddress }
    return preferred?.takeIf { it in addresses } ?: addresses.firstOrNull()
}

internal object ReceiverWifiAddress {
    // The receiver already rechecks its listening address every five seconds.
    // Inspect all visible networks rather than the app's VPN-routed default.
    @Suppress("DEPRECATION")
    fun read(manager: ConnectivityManager, preferred: String? = null): String {
        val active = manager.activeNetwork
        val networks = manager.allNetworks.mapNotNull { network ->
            val capabilities = manager.getNetworkCapabilities(network) ?: return@mapNotNull null
            val links = manager.getLinkProperties(network) ?: return@mapNotNull null
            ReceiverNetworkAddress(
                wifi = capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI),
                vpn = capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN),
                addresses = links.linkAddresses.map { it.address },
                active = network == active,
            )
        }
        return selectReceiverWifiAddress(networks, preferred) ?: error("wifi_required")
    }
}
