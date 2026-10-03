package app.backupduck

import org.junit.Assert.*
import org.junit.Test
import java.net.InetAddress

class ReceiverWifiAddressTest {
    private fun network(wifi: Boolean, vpn: Boolean, vararg addresses: String, active: Boolean = false) =
        ReceiverNetworkAddress(wifi, vpn, addresses.map { InetAddress.getByName(it) }, active)

    @Test fun wifiTransportOnActiveVpnMustNeverSupplyTheListeningAddress() {
        val vpn = network(true, true, "172.19.0.1", active = true)
        val wifi = network(true, false, "192.168.3.18")
        assertEquals("192.168.3.18", selectReceiverWifiAddress(listOf(vpn, wifi)))
        assertEquals("192.168.3.18", selectReceiverWifiAddress(listOf(wifi, vpn), "172.19.0.1"))
    }

    @Test fun wifiDoesNotNeedToBeTheDefaultOrHaveInternetValidation() {
        val cellular = network(false, false, "10.0.0.2", active = true)
        assertEquals("192.168.3.18", selectReceiverWifiAddress(listOf(cellular, network(true, false, "192.168.3.18"))))
        assertEquals("192.168.3.18", selectReceiverWifiAddress(listOf(network(true, false, "192.168.3.18", active = true))))
    }

    @Test fun noPhysicalWifiDoesNotFallBackToTunnelOrCellular() {
        assertNull(selectReceiverWifiAddress(listOf(network(true, true, "172.19.0.1"), network(false, false, "10.0.0.2"))))
        assertNull(selectReceiverWifiAddress(emptyList()))
    }

    @Test fun unusableAndIpv6AddressesAreNotAdvertisedToTheIpv4Receiver() {
        assertNull(selectReceiverWifiAddress(listOf(network(true, false, "::1", "fe80::1", "0.0.0.0", "127.0.0.1", "169.254.1.2", "224.0.0.1"))))
        assertEquals("192.168.3.18", selectReceiverWifiAddress(listOf(network(true, false, "fe80::1", "192.168.3.18"))))
    }

    @Test fun vpnChangesDoNotMoveAnExistingListenerBetweenPhysicalNetworks() {
        val old = network(true, false, "192.168.3.18")
        val other = network(true, false, "192.168.4.18", active = true)
        assertEquals("192.168.3.18", selectReceiverWifiAddress(listOf(other, old), "192.168.3.18"))
        assertEquals("192.168.4.18", selectReceiverWifiAddress(listOf(other), "192.168.3.18"))
    }
}
