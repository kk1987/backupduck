package app.backupduck

import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.net.Uri
import android.provider.MediaStore
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/** User-initiated export of Pixel gallery publication receipts, without media bytes. */
internal object GalleryReceiptExport {
    fun create(context: Context, receiverRoot: String): JSONObject {
        val database = File(receiverRoot, "store/receiver.sqlite3")
        check(database.isFile) { "receiver_store_missing" }
        val entries = JSONArray()
        val unresolved = JSONArray()
        val previousNames = context.getSharedPreferences("cloud_audit_names", Context.MODE_PRIVATE)
        val backfill = previousNames.edit()
        SQLiteDatabase.openDatabase(database.path, null, SQLiteDatabase.OPEN_READONLY).use { db ->
            // The store adds cloud columns when the receiver next opens it.
            val hasCloud = db.rawQuery("SELECT EXISTS(SELECT 1 FROM pragma_table_info('assets') WHERE name='cloud_state')", null).use {
                it.moveToFirst() && it.getInt(0) == 1
            }
            val cloudColumn = if (hasCloud) "a.cloud_state" else "'unknown'"
            db.rawQuery(
                "SELECT a.id,a.manifest,a.published_at_ms,g.copy,$cloudColumn FROM assets a " +
                    "LEFT JOIN gallery_copies g ON g.asset_id=a.id " +
                    "WHERE a.received=1 AND a.processing='complete' ORDER BY a.rowid",
                null
            ).use { rows ->
                while (rows.moveToNext()) {
                    val id = rows.getString(0)
                    val copy = rows.getString(3)?.let(::JSONObject)
                    val name = copy?.optString("display_name")?.takeIf(String::isNotBlank)
                        ?: previousNames.getString(id, null)
                        ?: copy?.optString("locator")?.let { locator -> lookupName(context, locator) }
                    if (name == null || copy == null) {
                        unresolved.put(JSONObject().put("id", id).put("reason", "publication_name_unavailable"))
                        continue
                    }
                    check(!name.contains('/') && !name.contains('\\') && name.length <= 255) { "invalid_publication_name" }
                    val size = copy.getLong("size")
                    val hash = copy.getString("sha256")
                    check(size > 0 && hash.matches(Regex("[0-9a-f]{64}"))) { "invalid_publication_evidence" }
                    val sha1 = copy.optString("sha1").takeIf(String::isNotEmpty)
                    check(sha1?.matches(Regex("[0-9a-f]{40}")) != false) { "invalid_publication_evidence" }
                    val kind = JSONObject(rows.getString(1)).getString("kind")
                    val item = JSONObject().put("id", id).put("name", name).put("size", size)
                        .put("sha256", hash).put("sha1", sha1 ?: JSONObject.NULL).put("kind", kind)
                        .put("published_at_ms", if (rows.isNull(2)) JSONObject.NULL else rows.getLong(2))
                        .put("cloud_state", rows.getString(4))
                    entries.put(item)
                    if (copy.optString("display_name").isBlank()) backfill.putString(id, name)
                }
            }
        }
        backfill.apply()
        return JSONObject().put("schema_version", 1).put("source", "BackupDuck Pixel receiver")
            .put("exported_at_ms", System.currentTimeMillis())
            .put("published_count", entries.length() + unresolved.length())
            .put("items", entries).put("unresolved", unresolved)
    }

    private fun lookupName(context: Context, locator: String): String? = runCatching {
        val uri = Uri.parse(locator)
        if (uri.scheme != "content" || uri.authority != "media") return@runCatching null
        context.contentResolver.query(uri, arrayOf(MediaStore.MediaColumns.DISPLAY_NAME), null, null, null)?.use {
            if (it.moveToFirst()) it.getString(0) else null
        }
    }.getOrNull()
}
