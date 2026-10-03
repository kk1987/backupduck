package app.backupduck

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BurstProcessorTest {
    @Test fun heicFramesStayHeicByDefault() {
        assertTrue(BurstProcessor.publishesHeic("image/heic", convertToJpeg = false, existingOnly = false, resuming = false, pendingMime = null))
        assertTrue(BurstProcessor.publishesHeic("image/heif", convertToJpeg = false, existingOnly = false, resuming = false, pendingMime = null))
        assertFalse(BurstProcessor.publishesHeic("image/jpeg", convertToJpeg = false, existingOnly = false, resuming = false, pendingMime = null))
        assertFalse(BurstProcessor.publishesHeic("image/png", convertToJpeg = false, existingOnly = false, resuming = false, pendingMime = null))
    }

    @Test fun jpegOptInAndPreEvidenceCopiesUseJpeg() {
        assertFalse(BurstProcessor.publishesHeic("image/heic", convertToJpeg = true, existingOnly = false, resuming = false, pendingMime = null))
        assertFalse(BurstProcessor.publishesHeic("image/heic", convertToJpeg = false, existingOnly = true, resuming = false, pendingMime = null))
    }

    @Test fun resumedRowsKeepTheirFormat() {
        assertTrue(BurstProcessor.publishesHeic("image/heic", convertToJpeg = true, existingOnly = false, resuming = true, pendingMime = "image/heic"))
        assertFalse(BurstProcessor.publishesHeic("image/heic", convertToJpeg = false, existingOnly = false, resuming = true, pendingMime = "image/jpeg"))
        assertFalse(BurstProcessor.publishesHeic("image/heic", convertToJpeg = false, existingOnly = false, resuming = true, pendingMime = null))
    }
}
