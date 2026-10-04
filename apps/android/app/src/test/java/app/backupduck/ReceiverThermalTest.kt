package app.backupduck

import android.os.PowerManager
import org.junit.Assert.assertEquals
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

    @Test fun thresholdChoicesRunFrom35To50() {
        assertEquals((35..50).toList(), ReceiverThermalSettings.choices)
        assertTrue((46..50).all { it in ReceiverThermalSettings.choices })
        assertEquals(40, ReceiverThermalSettings.DEFAULT_CELSIUS)
    }

    @Test fun highThresholdHoldsOnlyAtThatTemperature() {
        val warm = ThermalReading(459, PowerManager.THERMAL_STATUS_NONE)
        assertFalse(ThermalDecision().next(true, 46, warm).held)
        val held = ThermalDecision().next(true, 46, ThermalReading(460, PowerManager.THERMAL_STATUS_NONE))
        assertTrue(held.held)
        assertTrue(held.next(true, 46, ThermalReading(441, PowerManager.THERMAL_STATUS_NONE)).held)
        assertFalse(held.next(true, 46, ThermalReading(440, PowerManager.THERMAL_STATUS_NONE)).held)
        // Severe system status still holds below the battery threshold.
        assertTrue(ThermalDecision().next(true, 50, ThermalReading(300, PowerManager.THERMAL_STATUS_SEVERE)).held)
    }

    @Test fun systemThermalWarningReleasesAfterCooling() {
        val held = ThermalDecision().next(true, 45, ThermalReading(null, PowerManager.THERMAL_STATUS_SEVERE))
        assertTrue(held.held)
        assertTrue(held.next(true, 45, ThermalReading(null, PowerManager.THERMAL_STATUS_MODERATE)).held)
        assertFalse(held.next(true, 45, ThermalReading(null, PowerManager.THERMAL_STATUS_LIGHT)).held)
    }
}
