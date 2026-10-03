package app.backupduck

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class GalleryCopyTest {
    @Test
    fun sha1IsSerializedOnlyWhenPresent() {
        val old = GalleryCopy("content://media/external_primary/images/media/1", "a".repeat(64), 2, "IMG_0001.JPG")
        assertFalse(old.json().has("sha1"))
        assertEquals(old, GalleryCopy.parse(JSONObject(old.json().toString())))
        val fresh = old.copy(sha1 = "b".repeat(40))
        assertEquals("b".repeat(40), fresh.json().getString("sha1"))
        assertEquals(fresh, GalleryCopy.parse(JSONObject(fresh.json().toString())))
    }
}
