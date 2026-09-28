package app.photobridge

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MotionProcessorTest {
    @Test fun nativeUnsupportedCodeFallsBackToConversion() {
        assertTrue(MotionProcessor.isUnsupportedContainer(IllegalStateException("unsupported")))
        assertTrue(MotionProcessor.isUnsupportedContainer(IllegalStateException("unsupported capability: motion JPEG input")))
        assertFalse(MotionProcessor.isUnsupportedContainer(IllegalStateException("integrity")))
    }
}
