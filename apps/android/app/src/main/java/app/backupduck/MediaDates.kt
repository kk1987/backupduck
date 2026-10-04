package app.backupduck

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.provider.MediaStore
import android.system.Os
import org.json.JSONObject
import java.io.File
import java.text.ParsePosition
import java.text.SimpleDateFormat
import androidx.exifinterface.media.ExifInterface
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.Locale
import java.util.UUID

/** Preserve the sender's capture time without changing verified media bytes. */
internal object MediaDates {
    fun captured(metadata: JSONObject?): Long? = metadata?.optString("created_at_ms")
        ?.toLongOrNull()?.takeIf { it > 0 && it <= 253402300799000L }

    /** Return a temporary date-enriched copy only when EXIF has no capture date. */
    fun prepare(context: Context, source: File, mime: String, metadata: JSONObject?): File {
        val captured = captured(metadata) ?: return source
        val extension = when (mime) {
            "image/jpeg" -> "jpg"
            "image/png" -> "png"
            "image/heic", "image/heif" -> "heic"
            else -> return source
        }
        val heif = extension == "heic"
        // Android 10's HEIF stack cannot parse some valid HEIF variants (seen
        // with a `heix`-branded file); ExifInterface then reports no date at all.
        // For HEIF, the Rust container parser decides when Android sees none.
        val androidDate = if (heif) runCatching { ExifInterface(source).dateTimeOriginal }.getOrNull()
            else ExifInterface(source).dateTimeOriginal
        if (androidDate != null) return source
        if (heif && hasCaptureDate(nativePhotoDate(source))) return source
        val output = File(context.cacheDir, "dated-${UUID.randomUUID()}.$extension")
        val date = DateTimeFormatter.ofPattern("uuuu:MM:dd HH:mm:ss", Locale.ROOT)
            .withZone(ZoneOffset.UTC).format(Instant.ofEpochMilli(captured))
        try {
            if (heif) {
                // Rust re-reads its own output and verifies DateTimeOriginal,
                // OffsetTimeOriginal and SubSecTimeOriginal. Android may be
                // unable to parse this container, so it cannot verify it.
                val written = NativeBridge.request(JSONObject().put("op", "write_photo_date")
                    .put("source", source.path).put("output", output.path).put("date", date).put("subsecond", captured % 1000)) as JSONObject
                check(written.optString("date_time_original") == date) { "publication_date_failed" }
                return output
            } else {
                source.copyTo(output)
                ExifInterface(output).apply {
                    setAttribute(ExifInterface.TAG_SUBSEC_TIME_ORIGINAL, "%03d".format(Locale.ROOT, captured % 1000))
                    setAttribute(ExifInterface.TAG_SUBSEC_TIME_DIGITIZED, "%03d".format(Locale.ROOT, captured % 1000))
                    setAttribute(ExifInterface.TAG_DATETIME_ORIGINAL, date)
                    setAttribute(ExifInterface.TAG_OFFSET_TIME_ORIGINAL, "+00:00")
                    setAttribute(ExifInterface.TAG_DATETIME_DIGITIZED, date)
                    setAttribute(ExifInterface.TAG_OFFSET_TIME_DIGITIZED, "+00:00")
                    saveAttributes()
                }
            }
            val exif = ExifInterface(output)
            check(exif.getAttribute(ExifInterface.TAG_DATETIME_ORIGINAL) == date &&
                exif.getAttribute(ExifInterface.TAG_OFFSET_TIME_ORIGINAL) == "+00:00" && exif.dateTimeOriginal == captured) { "publication_date_failed" }
            return output
        } catch (error: Exception) { output.delete(); throw error }
    }

    /** Throws `publication_date_unreadable` when the container cannot be parsed. */
    private fun nativePhotoDate(source: File): String? {
        val value = NativeBridge.request(JSONObject().put("op", "read_photo_date").put("source", source.path)) as JSONObject
        return if (value.isNull("date_time_original")) null else value.getString("date_time_original")
    }

    /** The acceptance rule of androidx ExifInterface 1.3.7 getDateTimeOriginal:
     * a non-zero digit, then a lenient `yyyy:MM:dd HH:mm:ss` or dashed parse.
     * HEIF and JPEG/PNG therefore agree on what counts as an existing date. */
    internal fun hasCaptureDate(value: String?): Boolean = value != null && value.any { it in '1'..'9' } &&
        listOf("yyyy:MM:dd HH:mm:ss", "yyyy-MM-dd HH:mm:ss").any { SimpleDateFormat(it, Locale.US).parse(value, ParsePosition(0)) != null }

    fun values(captured: Long?): ContentValues = ContentValues().apply {
        captured?.let {
            put(MediaStore.MediaColumns.DATE_TAKEN, it)
            // DATE_TAKEN is milliseconds; file modification dates are seconds.
            put(MediaStore.MediaColumns.DATE_MODIFIED, it / 1000)
        }
    }

    /** Do this after the last byte is written and before releasing IS_PENDING.
     * Android's scanner can replace the insert-time DATE_TAKEN with a date inferred
     * from the file. Missing EXIF must therefore fall back to the capture time,
     * not the time this receiver created the copy. No original file is edited.
     */
    fun stampPending(context: Context, uri: Uri, captured: Long?) {
        if (captured == null) return
        checkNotNull(context.contentResolver.openFileDescriptor(uri, "rw")).use { descriptor ->
            check(File("/proc/self/fd/${descriptor.fd}").setLastModified(captured)) { "publication_date_failed" }
            check(Os.fstat(descriptor.fileDescriptor).st_mtime == captured / 1000) { "publication_date_failed" }
        }
    }
}
