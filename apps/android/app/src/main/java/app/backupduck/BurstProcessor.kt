package app.backupduck

import android.content.Context
import android.net.Uri
import android.provider.MediaStore
import org.json.JSONObject
import java.io.File

/** The receiver's delivery copy may change format; verified originals never do. */
internal object BurstProcessor {
    suspend fun publish(context: Context, item: JSONObject, existingOnly: Boolean = false, resumeLocator: String? = null): GalleryCopy {
        val asset = item.getJSONObject("asset")
        val metadata = asset.getJSONObject("metadata")
        val resource = asset.getJSONArray("resources").getJSONObject(0)
        val original = File(item.getJSONObject("resources").getString(resource.getString("sha256")))
        val work = File(context.cacheDir, "burst-${item.getString("id")}")
        check(work.exists() || work.mkdirs()) { "storage" }
        val decoded = File(work, "still.jpg")
        val output = File(work, "output.jpg")
        val heicOutput = File(work, "output.heic")
        val partial = File(work, "output.burst.partial")
        try {
            val stillMime = MediaContainer.imageMime(original, resource.getString("media_type"))
            val pendingMime = resumeLocator?.let { locator ->
                context.contentResolver.query(Uri.parse(locator), arrayOf(MediaStore.MediaColumns.MIME_TYPE), null, null, null)?.use { cursor ->
                    if (cursor.moveToFirst()) cursor.getString(0) else null
                }
            }
            if (publishesHeic(stillMime, BurstDeliverySettings.convertHeicToJpeg(context), existingOnly, resumeLocator != null, pendingMime)) {
                // HEIC pixels, HDR gain map and metadata stay; only the primary XMP item changes.
                var dated: File? = null
                try {
                    if (partial.exists()) check(partial.delete()) { "storage" }
                    dated = MediaDates.prepare(context, original, "image/heic", metadata)
                    NativeBridge.request(JSONObject().put("op", "package_heic_burst").put("heic", dated.path)
                        .put("output", heicOutput.path).put("metadata", metadata))
                    return MediaPublisher.publishFile(context, heicOutput, item, "image/heic", metadata, existingOnly, resumeLocator = resumeLocator)
                } catch (error: IllegalStateException) {
                    // A pending HEIC row can only be completed with HEIC bytes.
                    if (pendingMime == "image/heic" || !MotionProcessor.isUnsupportedContainer(error)) throw error
                } finally { if (dated != original) dated?.delete() }
            }
            // JPEG pixels, EXIF, ICC and existing standard XMP remain untouched.
            // HEIC/other supported still formats use the native image decoder.
            val jpeg = if (stillMime == "image/jpeg") original
                else decoded.also { MotionProcessor.prepareStill(original, it) }
            if (partial.exists()) check(partial.delete()) { "storage" }
            val dated = MediaDates.prepare(context, jpeg, "image/jpeg", metadata)
            try {
                NativeBridge.request(JSONObject().put("op", "package_burst").put("jpeg", dated.path)
                    .put("output", output.path).put("metadata", metadata))
            } finally { if (dated != jpeg) dated.delete() }
            return MediaPublisher.publishFile(context, output, item, "image/jpeg", metadata, existingOnly, resumeLocator = resumeLocator)
        } finally {
            listOf(decoded, output, heicOutput, partial).forEach { it.delete() }
            work.delete()
        }
    }

    /**
     * HEIC frames publish as HEIC unless the receiver opted into JPEG. A resumed
     * row keeps the format it was created with. Copies adopted without evidence
     * predate HEIC bursts, so they are always JPEG.
     */
    internal fun publishesHeic(stillMime: String, convertToJpeg: Boolean, existingOnly: Boolean, resuming: Boolean, pendingMime: String?): Boolean {
        if (stillMime !in setOf("image/heic", "image/heif")) return false
        if (resuming) return pendingMime == "image/heic"
        return !convertToJpeg && !existingOnly
    }
}
