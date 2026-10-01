package app.photobridge

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MediaContainerTest {
    private fun ftyp(major: String, vararg compatible: String): ByteArray {
        val size = 16 + compatible.size * 4
        return byteArrayOf(0, 0, 0, size.toByte()) + "ftyp".toByteArray() + major.toByteArray() +
            byteArrayOf(0, 0, 0, 0) + compatible.joinToString("").toByteArray()
    }
    @Test fun heicBytesOverrideJpegDeclarationAndFilename() {
        val file = File.createTempFile("camera-export", ".JPG")
        try {
            file.writeBytes(ftyp("heic", "mif1", "miaf", "MiHB", "heic"))
            assertEquals("image/heic", MediaContainer.imageMime(file, "image/jpeg"))
            assertEquals("image/heic", MediaContainer.imageMime(ftyp("mif1", "heic")))
        } finally { file.delete() }
    }
    @Test fun ordinaryJpegAndPngRemainTheirActualTypes() {
        assertEquals("image/jpeg", MediaContainer.imageMime(byteArrayOf(0xff.toByte(), 0xd8.toByte(), 0xff.toByte())))
        assertEquals("image/png", MediaContainer.imageMime(byteArrayOf(0x89.toByte(), 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a)))
    }
    @Test fun videoAvifAndIncompleteHeadersAreNotClassifiedAsHeic() {
        assertNull(MediaContainer.imageMime(ftyp("qt  ")))
        assertNull(MediaContainer.imageMime(ftyp("avif", "mif1")))
        assertNull(MediaContainer.imageMime(ftyp("heic").copyOf(12)))
        assertNull(MediaContainer.imageMime(ftyp("heic").also { it[3] = 100 }))
        assertNull(MediaContainer.imageMime(ftyp("mif1").also { "heic".toByteArray().copyInto(it, 12) }))
    }
}
