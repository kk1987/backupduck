package app.backupduck

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.provider.MediaStore
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import org.json.JSONObject
import java.io.File
import java.io.InputStream
import java.security.MessageDigest

/** [sha1] is the digest cloud lookups use; evidence recorded before it existed omits it. */
internal data class GalleryCopy(val locator: String, val sha256: String, val size: Long, val displayName: String? = null, val sha1: String? = null) {
    fun json() = JSONObject().put("locator", locator).put("sha256", sha256).put("size", size).apply {
        displayName?.let { put("display_name", it) }
        sha1?.let { put("sha1", it) }
    }
    companion object { fun parse(value: JSONObject) = GalleryCopy(value.getString("locator"), value.getString("sha256"), value.getLong("size"), value.optString("display_name").ifEmpty { null }, value.optString("sha1").ifEmpty { null }) }
}
private data class Digests(val sha256: String, val sha1: String, val size: Long)
internal object MediaPublisher {
    private val publicationLock = Any()
    suspend fun publish(context: Context, item: JSONObject, existingOnly: Boolean = false): GalleryCopy {
        val evidence = NativeBridge.request(JSONObject().put("op", "gallery_evidence").put("id", item.getString("id"))) as JSONObject
        var resumeLocator: String? = null
        evidence.optJSONObject("copy")?.let { stored ->
            val copy = GalleryCopy.parse(stored)
            try { return verify(context, copy) }
            catch (cancelled: kotlinx.coroutines.CancellationException) { throw cancelled }
            catch (error: Exception) {
                // An incomplete write can resume. A complete but changed/missing
                // copy must never be overwritten merely to reclaim originals.
                if (evidence.getBoolean("confirmed") || !ownedPending(context, Uri.parse(copy.locator))) throw error
                resumeLocator = copy.locator
            }
        }
        val asset = item.getJSONObject("asset")
        if (asset.getString("kind") == "motion") return MotionProcessor.publish(context, item, existingOnly, resumeLocator)
        if (asset.optJSONObject("metadata")?.has("burst_group_ref") == true) return BurstProcessor.publish(context, item, existingOnly, resumeLocator)
        val resource = asset.getJSONArray("resources").getJSONObject(0)
        val source = File(item.getJSONObject("resources").getString(resource.getString("sha256")))
        val declaredMime = resource.getString("media_type")
        val mime = if (declaredMime.startsWith("image/")) MediaContainer.imageMime(source, declaredMime) else declaredMime
        val dated = MediaDates.prepare(context, source, mime, asset.optJSONObject("metadata"))
        try {
            return publishFile(context, dated, item, mime,
                asset.optJSONObject("metadata"), existingOnly, if (dated == source) resource.getString("sha256") else null, resumeLocator)
        } finally { if (dated != source) dated.delete() }
    }
    private fun ownedPending(context: Context, uri: Uri): Boolean {
        if (uri.scheme != "content" || uri.authority != "media") return false
        return context.contentResolver.query(uri, arrayOf(MediaStore.MediaColumns.IS_PENDING, MediaStore.MediaColumns.OWNER_PACKAGE_NAME), null, null, null)?.use {
            it.moveToFirst() && it.getInt(0) == 1 && it.getString(1) == context.packageName
        } ?: false
    }
    /** One pass over the bytes yields both the receipt and the cloud lookup digest. */
    private suspend fun hash(input: InputStream, limit: Long, write: ((ByteArray, Int) -> Unit)? = null): Digests {
        val sha256 = MessageDigest.getInstance("SHA-256"); val sha1 = MessageDigest.getInstance("SHA-1")
        val buffer = ByteArray(65_536); var bytes = 0L
        while (true) {
            currentCoroutineContext().ensureActive()
            val count = input.read(buffer); if (count < 0) break
            check(count.toLong() <= limit - bytes) { "gallery_copy_changed" }
            sha256.update(buffer, 0, count); sha1.update(buffer, 0, count); bytes += count; write?.invoke(buffer, count)
        }
        fun hex(value: ByteArray) = value.joinToString("") { "%02x".format(it) }
        return Digests(hex(sha256.digest()), hex(sha1.digest()), bytes)
    }
    private fun Digests.matches(copy: GalleryCopy) = sha256 == copy.sha256 && size == copy.size && (copy.sha1 == null || sha1 == copy.sha1)
    /**
     * Reopen the owned, ready MediaStore item; indexed size alone is not proof.
     * Returns the copy with the SHA-1 of the bytes just read.
     */
    suspend fun verify(context: Context, copy: GalleryCopy): GalleryCopy {
        val uri = Uri.parse(copy.locator)
        check(uri.scheme == "content" && uri.authority == "media" && copy.size > 0 && copy.sha256.matches(Regex("[0-9a-f]{64}")) &&
            copy.sha1?.matches(Regex("[0-9a-f]{40}")) != false) { "gallery_copy_changed" }
        checkReady(context, uri)
        val actual = checkNotNull(context.contentResolver.openInputStream(uri)).use { hash(it, copy.size) }
        check(actual.matches(copy)) { "gallery_copy_changed" }
        checkReady(context, uri)
        return copy.copy(sha1 = actual.sha1)
    }
    /**
     * True only when MediaStore has no row for the locator, pending included.
     * Rows owned by another app are invisible here too; callers only use it
     * for copies whose bytes the cloud already matched.
     */
    fun absent(context: Context, copy: GalleryCopy): Boolean {
        val uri = Uri.parse(copy.locator)
        if (uri.scheme != "content" || uri.authority != "media") return false
        @Suppress("DEPRECATION")
        val lookup = MediaStore.setIncludePending(uri)
        return runCatching {
            context.contentResolver.query(lookup, arrayOf(MediaStore.MediaColumns._ID), null, null, null)?.use { !it.moveToFirst() }
        }.getOrNull() ?: false
    }
    private fun checkReady(context: Context, uri: Uri) {
        val columns = mutableListOf(MediaStore.MediaColumns.IS_PENDING, MediaStore.MediaColumns.OWNER_PACKAGE_NAME, MediaStore.MediaColumns.RELATIVE_PATH)
        if (Build.VERSION.SDK_INT >= 30) columns += MediaStore.MediaColumns.IS_TRASHED
        checkNotNull(context.contentResolver.query(uri, columns.toTypedArray(), null, null, null)).use { cursor ->
            check(cursor.moveToFirst() && cursor.getInt(0) == 0 && cursor.getString(1) == context.packageName && cursor.getString(2) == "DCIM/BackupDuck/" && (Build.VERSION.SDK_INT < 30 || cursor.getInt(3) == 0)) { "gallery_copy_missing" }
        }
    }
    private data class Row(val uri: Uri, val ready: Boolean, val owner: String?)
    suspend fun publishFile(context: Context, source: File, item: JSONObject, mime: String, metadata: JSONObject?, existingOnly: Boolean = false, originalHash: String? = null, resumeLocator: String? = null): GalleryCopy {
        val resolver = context.contentResolver
        val collection = if (mime.startsWith("video/")) MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
            else MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        // Collection queries omit pending rows on Android 10 unless requested.
        // Include them so a name held by any interrupted write counts as taken;
        // ownership checks below still prevent changing another application's media.
        @Suppress("DEPRECATION")
        val lookupCollection = MediaStore.setIncludePending(collection)
        val relative = "DCIM/BackupDuck/"
        val captured = MediaDates.captured(metadata)
        val columns = arrayOf(MediaStore.MediaColumns._ID, MediaStore.MediaColumns.IS_PENDING, MediaStore.MediaColumns.OWNER_PACKAGE_NAME)
        val selection = "${MediaStore.MediaColumns.DISPLAY_NAME}=? AND ${MediaStore.MediaColumns.RELATIVE_PATH}=?"
        fun rows(name: String): List<Row> = checkNotNull(resolver.query(lookupCollection, columns, selection, arrayOf(name, relative), null)).use { cursor ->
            buildList { while (cursor.moveToNext()) add(Row(ContentUris.withAppendedId(collection, cursor.getLong(0)), cursor.getInt(1) == 0, cursor.getString(2))) }
        }
        fun displayName(uri: Uri): String? = resolver.query(uri, arrayOf(MediaStore.MediaColumns.DISPLAY_NAME, MediaStore.MediaColumns.RELATIVE_PATH), null, null, null)?.use {
            if (it.moveToFirst() && it.getString(1) == relative) it.getString(0)?.takeIf(String::isNotBlank) else null
        }
        val size = source.length(); check(size > 0) { "gallery_copy_missing" }
        // The source is read once for SHA-1 even when its SHA-256 is already known.
        suspend fun expected() = source.inputStream().use { hash(it, size) }.also {
            check(it.size == size && (originalHash == null || it.sha256 == originalHash)) { "gallery_copy_changed" }
        }

        if (resumeLocator != null) {
            // Resume only the stored row. Its name is whatever MediaStore kept.
            val uri = Uri.parse(resumeLocator)
            val name = synchronized(publicationLock) { if (ownedPending(context, uri)) displayName(uri) else null }
            check(name != null && !existingOnly) { "gallery_copy_missing" }
            val digests = expected()
            return write(context, source, item, GalleryCopy(uri.toString(), digests.sha256, size, name, digests.sha1), captured)
        }

        if (existingOnly) {
            // Pre-evidence relay: adopt a finished copy under an older name, never insert.
            val found = synchronized(publicationLock) {
                (listOfNotNull(GalleryNaming.legacyOrNull(item, mime)) + GalleryNaming.datedOrEmpty(item, mime)).distinct().flatMap { name ->
                    rows(name).filter { it.ready && it.owner == context.packageName }.map { it.uri to name }
                }
            }
            check(found.isNotEmpty()) { "gallery_copy_missing" }
            val digests = expected()
            // A short dated suffix may belong to another asset; only matching bytes count.
            var failure: Exception? = null
            for ((uri, name) in found) {
                try { return verify(context, GalleryCopy(uri.toString(), digests.sha256, size, name, digests.sha1)) }
                catch (cancelled: kotlinx.coroutines.CancellationException) { throw cancelled }
                catch (error: Exception) { failure = error }
            }
            throw checkNotNull(failure)
        }

        val (row, name) = synchronized(publicationLock) {
            // Pre-evidence receivers wrote under the legacy name before any receipt.
            val legacy = GalleryNaming.legacyOrNull(item, mime)
            val interrupted = legacy?.let(::rows)?.also { check(it.size <= 1) { "gallery_copy_ambiguous" } }
                ?.firstOrNull { it.owner == context.packageName }
            if (interrupted != null) return@synchronized interrupted to checkNotNull(legacy)
            // Any row holding a name, pending or not, ours or not, makes it taken.
            val requested = ((0..GalleryNaming.MAX_COLLISION_VARIANTS).map { GalleryNaming.preferred(item, mime, it) } +
                GalleryNaming.datedOrEmpty(item, mime)).distinct().firstOrNull { rows(it).isEmpty() } ?: error("publication_name_unavailable")
            val inserted = checkNotNull(resolver.insert(collection, ContentValues().apply {
                put(MediaStore.MediaColumns.DISPLAY_NAME, requested); put(MediaStore.MediaColumns.MIME_TYPE, mime)
                put(MediaStore.MediaColumns.RELATIVE_PATH, relative); put(MediaStore.MediaColumns.IS_PENDING, 1)
                putAll(MediaDates.values(captured))
            })) { "publication_failed" }
            // MediaProvider may sanitise or uniquify the requested name; record what it kept.
            val actual = displayName(inserted)
            if (actual == null) { resolver.delete(inserted, null, null); error("publication_name_unavailable") }
            Row(inserted, false, context.packageName) to actual
        }
        val digests = expected()
        val copy = GalleryCopy(row.uri.toString(), digests.sha256, size, name, digests.sha1)
        if (row.ready) return verify(context, copy)
        // A crash before prepare_gallery leaves an owned pending row without
        // evidence. The next attempt treats its name as taken and inserts again;
        // the orphan stays pending, hidden from the gallery until MediaProvider
        // expires it.
        return write(context, source, item, copy, captured)
    }
    private suspend fun write(context: Context, source: File, item: JSONObject, copy: GalleryCopy, captured: Long?): GalleryCopy {
        val resolver = context.contentResolver
        val uri = Uri.parse(copy.locator)
        NativeBridge.request(JSONObject().put("op", "prepare_gallery").put("id", item.getString("id")).put("copy", copy.json()))
        val copied = source.inputStream().use { input ->
            checkNotNull(resolver.openOutputStream(uri, "wt")).use { output -> hash(input, copy.size) { buffer, count -> output.write(buffer, 0, count) } }
        }
        check(copied.matches(copy)) { "gallery_copy_changed" }
        MediaDates.stampPending(context, uri, captured)
        check(resolver.update(uri, MediaDates.values(captured).apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null) == 1) { "publication_failed" }
        return verify(context, copy)
    }
}
