package app.backupduck

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MotionProcessorTest {
    @Test fun nativeUnsupportedCodeFallsBackToConversion() {
        assertTrue(MotionProcessor.isUnsupportedContainer(IllegalStateException("unsupported")))
        assertTrue(MotionProcessor.isUnsupportedContainer(IllegalStateException("unsupported capability: motion JPEG input")))
        assertFalse(MotionProcessor.isUnsupportedContainer(IllegalStateException("integrity")))
        // A HEIF whose metadata cannot be parsed must not enter the codec fallback.
        assertFalse(MotionProcessor.isUnsupportedContainer(IllegalStateException("publication_date_unreadable")))
    }
}
