package app.backupduck

import android.os.PowerManager
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ReceiverThermalTest {
    @Test fun batteryThresholdHysteresisAndMissingReading() {
        val cool = ThermalReading(399, PowerManager.THERMAL_STATUS_NONE)
        val hot = ThermalReading(400, PowerManager.THERMAL_STATUS_NONE)
        assertFalse(ThermalDecision().next(true, 40, cool).held)
        val held = ThermalDecision().next(true, 40, hot)
        assertTrue(held.held)
        assertTrue(held.next(true, 40, ThermalReading(null, PowerManager.THERMAL_STATUS_NONE)).held)
        assertTrue(held.next(true, 40, ThermalReading(381, PowerManager.THERMAL_STATUS_NONE)).held)
        assertFalse(held.next(true, 40, ThermalReading(380, PowerManager.THERMAL_STATUS_NONE)).held)
        assertFalse(held.next(false, 40, hot).held)
    }

    @Test fun systemThermalWarningReleasesAfterCooling() {
        val held = ThermalDecision().next(true, 45, ThermalReading(null, PowerManager.THERMAL_STATUS_SEVERE))
        assertTrue(held.held)
        assertTrue(held.next(true, 45, ThermalReading(null, PowerManager.THERMAL_STATUS_MODERATE)).held)
        assertFalse(held.next(true, 45, ThermalReading(null, PowerManager.THERMAL_STATUS_LIGHT)).held)
    }
}
