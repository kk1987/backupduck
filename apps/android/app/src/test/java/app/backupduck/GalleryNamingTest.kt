package app.backupduck

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class GalleryNamingTest {
    private val id = "a1b2c3d4e5f6a7b8" + "0".repeat(48)

    private fun item(kind: String, filename: String, burst: Boolean = false): JSONObject {
        val metadata = JSONObject().put("created_at_ms", "1786761701000")
        if (burst) metadata.put("burst_group_ref", "synthetic")
        return JSONObject().put("id", id).put("asset", JSONObject().put("kind", kind).put("metadata", metadata)
            .put("resources", JSONArray().put(JSONObject().put("filename", filename))))
    }

    @Test fun stillsAndVideosKeepTheirOriginalName() {
        assertEquals("IMG_1234.HEIC", GalleryNaming.preferred(item("photo", "IMG_1234.HEIC"), "image/heic"))
        assertEquals("clip.mov", GalleryNaming.preferred(item("video", "clip.mov"), "video/quicktime"))
        assertEquals("照片 2024.HEIC", GalleryNaming.preferred(item("photo", "照片 2024.HEIC"), "image/heic"))
        assertEquals("IMG_1235.HEIC", GalleryNaming.preferred(item("photo", "IMG_1235.HEIC", burst = true), "image/heic"))
    }

    @Test fun extensionFollowsTheWrittenFormat() {
        assertEquals("IMG_1234.jpg", GalleryNaming.preferred(item("photo", "IMG_1234.HEIC"), "image/jpeg"))
        assertEquals("IMG_1235.jpg", GalleryNaming.preferred(item("photo", "IMG_1235.HEIC", burst = true), "image/jpeg"))
        assertEquals(".JPEG", GalleryNaming.extensionFor("image/jpeg", "IMG_1234.JPEG"))
        assertEquals(".heif", GalleryNaming.extensionFor("image/heif", "a.heif"))
        assertEquals(".mp4", GalleryNaming.extensionFor("video/mp4", "clip.MOV"))
        assertEquals(".tiff", GalleryNaming.extensionFor("image/tiff", "scan.tiff"))
        assertEquals("", GalleryNaming.extensionFor("image/tiff", "scan.ti-ff"))
        assertEquals("", GalleryNaming.extensionFor(null, "noextension"))
    }

    @Test fun motionPhotosEndInMp() {
        val motion = item("motion", "IMG_1234.HEIC")
        assertEquals("IMG_1234_MP.HEIC", GalleryNaming.preferred(motion, "image/heic"))
        assertEquals("IMG_1234_MP.jpg", GalleryNaming.preferred(motion, "image/jpeg"))
        assertEquals("IMG_1234 (3)_MP.HEIC", GalleryNaming.preferred(motion, "image/heic", 3))
    }

    @Test fun collisionVariantsAreNumbered() {
        assertEquals("IMG_1234 (1).HEIC", GalleryNaming.preferred(item("photo", "IMG_1234.HEIC"), "image/heic", variant = 1))
        assertEquals(20, GalleryNaming.MAX_COLLISION_VARIANTS)
    }

    @Test fun unsafeStemsAreSanitised() {
        assertEquals("a_b_.jpg", GalleryNaming.preferred(item("photo", "a:b?.jpg"), "image/jpeg"))
        assertEquals("x.jpg", GalleryNaming.preferred(item("photo", "dir/x.jpg"), "image/jpeg"))
        assertEquals("x.jpg", GalleryNaming.preferred(item("photo", "dir\\x.jpg"), "image/jpeg"))
        assertEquals("ab.jpg", GalleryNaming.preferred(item("photo", " a\u0001b .jpg"), "image/jpeg"))
        assertEquals("BD_a1b2c3d4.jpg", GalleryNaming.preferred(item("photo", ".."), "image/jpeg"))
        assertEquals("BD_a1b2c3d4.jpg", GalleryNaming.preferred(item("photo", ""), "image/jpeg"))
        assertEquals("BD_a1b2c3d4.HEIC", GalleryNaming.preferred(item("photo", ".HEIC"), "image/heic"))
    }

    @Test fun longStemsFitEveryVariant() {
        for (unit in listOf("a", "照", "📷")) {
            val long = item("motion", unit.repeat(300) + ".HEIC")
            val names = (0..GalleryNaming.MAX_COLLISION_VARIANTS).map { GalleryNaming.preferred(long, "image/heic", it) }
            assertTrue(names.all { it.toByteArray(Charsets.UTF_8).size <= 255 })
            assertEquals(names.size, names.distinct().size)
            assertTrue(names.all { name -> name.indices.all { !name[it].isHighSurrogate() || it + 1 < name.length && name[it + 1].isLowSurrogate() } })
            assertTrue(names[0].toByteArray(Charsets.UTF_8).size > 240)
        }
    }

    @Test fun resumeCandidatesCoverEveryNamingScheme() {
        val motion = item("motion", "IMG_1234.HEIC")
        val names = GalleryNaming.resumeCandidates(motion, "image/heic")
        assertEquals("IMG_1234_MP.HEIC", names.first())
        assertTrue("IMG_1234 (20)_MP.HEIC" in names)
        assertTrue(GalleryNaming.name(motion, 4, "image/heic") in names)
        assertTrue("BD_20260815_024141Z_a1b2.heic" in names)
        assertTrue(GalleryNaming.legacyName(motion, "image/heic") in names)
        assertEquals(names.size, names.distinct().size)
        // An unsafe original extension never had a dated or legacy name.
        val odd = GalleryNaming.resumeCandidates(item("photo", "x.ti-ff"), "image/jpeg")
        assertEquals((0..GalleryNaming.MAX_COLLISION_VARIANTS).map { GalleryNaming.preferred(item("photo", "x.ti-ff"), "image/jpeg", it) }, odd)
    }

    @Test fun datedFallbackIsUnchanged() {
        val photo = item("photo", "IMG_1234.HEIC")
        assertEquals("BD_20260815_024141Z_a1b2.HEIC", GalleryNaming.name(photo))
        assertEquals("BD_${id}.HEIC", GalleryNaming.legacyName(photo))
    }
}
