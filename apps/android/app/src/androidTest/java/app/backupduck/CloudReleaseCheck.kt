package app.backupduck

import android.app.Instrumentation
import android.database.sqlite.SQLiteDatabase
import android.graphics.Bitmap
import android.graphics.Color
import android.net.Uri
import kotlinx.coroutines.delay
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.UUID

/**
 * Synthetic end-to-end cloud release: one verified copy is deleted, one that
 * vanished first is recorded as missing. Verdicts are written straight into the
 * stopped receiver's database; no Google service is contacted.
 */
internal suspend fun Instrumentation.checkCloudRelease() {
    val root = File(targetContext.filesDir, "cloud-release-${UUID.randomUUID()}").apply { mkdirs() }
    val receiverRoot = File(root, "receiver").path
    val copies = mutableListOf<GalleryCopy>()
    try {
        val pairing = NativeBridge.request(JSONObject().put("op", "start_receiver").put("root", receiverRoot)
            .put("listen", "127.0.0.1:38484").put("capacity", 32 * 1024 * 1024)) as JSONObject
        NativeBridge.request(JSONObject().put("op", "open_sender").put("root", File(root, "sender").path))
        val ids = mutableListOf<String>()
        for ((index, color) in listOf(Color.CYAN, Color.YELLOW).withIndex()) {
            val photo = File(root, "cloud-$index.jpg")
            val bitmap = Bitmap.createBitmap(64, 48, Bitmap.Config.ARGB_8888).apply { eraseColor(color) }
            photo.outputStream().use { check(bitmap.compress(Bitmap.CompressFormat.JPEG, 95, it)) }; bitmap.recycle()
            NativeBridge.request(JSONObject().put("op", "enqueue").put("receiver_id", pairing.getString("receiver_id"))
                .put("source_id", "cloud-$index-${root.name}").put("revision", "1").put("kind", "photo")
                .put("resources", JSONArray().put(JSONObject().put("path", photo.path).put("role", "photo")
                    .put("filename", photo.name).put("media_type", "image/jpeg"))))
            check((NativeBridge.request(JSONObject().put("op", "run_sender").put("pairing", pairing)) as JSONObject).getString("state") == "received")
            val item = (NativeBridge.request(JSONObject().put("op", "publications")) as JSONArray).getJSONObject(0)
            val copy = MediaPublisher.publish(targetContext, item); copies += copy
            NativeBridge.request(JSONObject().put("op", "gallery_publication").put("id", item.getString("id")).put("copy", copy.json()))
            ids += item.getString("id")
        }
        check(copies.all { it.sha1 != null }) { "sha1_missing" }
        NativeBridge.request(JSONObject().put("op", "stop_receiver"))
        fun settings() = (NativeBridge.request(JSONObject().put("op", "receiver_settings").put("root", receiverRoot)) as JSONObject).getJSONObject("settings")
        check(!settings().getBoolean("cloud_release")) { "cloud_release_enabled_by_default" }
        var opened = false
        repeat(40) { if (!opened) { opened = runCatching { NativeBridge.request(JSONObject().put("op", "cloud_release_candidates").put("root", receiverRoot)) }.isSuccess; if (!opened) delay(50) } }
        check(opened) { "store_not_released" }
        SQLiteDatabase.openDatabase(File(receiverRoot, "store/receiver.sqlite3").path, null, SQLiteDatabase.OPEN_READWRITE).use { db ->
            db.execSQL("UPDATE assets SET cloud_state='verified',cloud_checked_at_ms=0 WHERE id IN (?,?)", arrayOf(ids[0], ids[1]))
        }
        val sweep = CloudRelease(receiverRoot)
        check(sweep.step(targetContext) == 0 && copies.none { MediaPublisher.absent(targetContext, it) }) { "disabled_release_deleted" }
        NativeBridge.request(JSONObject().put("op", "receiver_settings").put("root", receiverRoot)
            .put("settings", settings().put("cloud_release", true).put("cloud_release_grace_ms", 0)))
        // The second copy disappears first, as after a manual cleanup.
        check(targetContext.contentResolver.delete(Uri.parse(copies[1].locator), null, null) == 1)
        repeat(3) { sweep.step(targetContext) }
        check(MediaPublisher.absent(targetContext, copies[0])) { "verified_copy_not_deleted" }
        check((NativeBridge.request(JSONObject().put("op", "cloud_release_candidates").put("root", receiverRoot)) as JSONArray).length() == 0) { "candidates_remain" }
        val history = (NativeBridge.request(JSONObject().put("op", "receiver_history").put("root", receiverRoot)
            .put("state", "all").put("kind", "all")) as JSONObject).getJSONArray("items")
        val rows = (0 until history.length()).map(history::getJSONObject).associateBy { it.getString("id") }
        check(ids.all { rows.getValue(it).getBoolean("originals_released") && rows.getValue(it).getBoolean("gallery_released") }) { "release_not_recorded" }
        check(ids.all { rows.getValue(it).getString("release_reason") == "cloud" }) { "release_reason" }
        val receipt = GalleryReceiptExport.create(targetContext, receiverRoot).getJSONArray("items")
        val exported = (0 until receipt.length()).map(receipt::getJSONObject).associateBy { it.getString("id") }
        check(ids.all { exported[it]?.getBoolean("gallery_released") == true && exported.getValue(it).getString("cloud_state") == "verified" }) { "receipt_dropped_released_copy" }
        val logs = NativeBridge.request(JSONObject().put("op", "receiver_logs").put("root", receiverRoot)) as JSONArray
        val codes = (0 until logs.length()).map { logs.getJSONObject(it).getString("code") }
        check("cloud_gallery_released" in codes && "cloud_gallery_missing" in codes) { "release_events_missing" }
    } finally {
        runCatching { NativeBridge.request(JSONObject().put("op", "stop_receiver")) }
        copies.forEach { copy -> runCatching { targetContext.contentResolver.delete(Uri.parse(copy.locator), null, null) } }
        root.deleteRecursively()
    }
}
