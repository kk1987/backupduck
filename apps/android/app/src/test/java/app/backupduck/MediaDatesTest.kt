package app.backupduck

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaDatesTest {
    @Test fun nativeHeifDateUsesExifInterfaceAcceptance() {
        assertTrue(MediaDates.hasCaptureDate("2026:06:14 17:35:13"))
        assertTrue(MediaDates.hasCaptureDate("2026-06-14 17:35:13"))
        assertFalse(MediaDates.hasCaptureDate(null))
        assertFalse(MediaDates.hasCaptureDate(""))
        assertFalse(MediaDates.hasCaptureDate("0000:00:00 00:00:00"))
        assertFalse(MediaDates.hasCaptureDate("    :  :     :  :  "))
        assertFalse(MediaDates.hasCaptureDate("unknown"))
    }
}
