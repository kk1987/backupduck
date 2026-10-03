package app.backupduck

import android.content.Context
import android.provider.MediaStore
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive

internal data class GalleryUsage(val readyBytes: Long, val readyCount: Long, val pendingBytes: Long, val pendingCount: Long, val unknownSizes: Long)

/** Indexed file sizes for this installation's delivery copies, never cloud state. */
internal object GalleryInventory {
    @Suppress("DEPRECATION")
    suspend fun read(context: Context): GalleryUsage {
        var bytes = 0L; var count = 0L; var pendingBytes = 0L; var pendingCount = 0L; var unknown = 0L
        val columns = arrayOf(MediaStore.MediaColumns.SIZE, MediaStore.MediaColumns.IS_PENDING, MediaStore.MediaColumns.OWNER_PACKAGE_NAME)
        for (collection in listOf(MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY),
            MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY))) {
            currentCoroutineContext().ensureActive()
            // API 29 supports this query parameter; owner and path restrict scope.
            // Copies keep original filenames, so ownership, not the name, marks ours.
            // No shared-library permission or file decoding is needed.
            val uri = MediaStore.setIncludePending(collection)
            val cursor = checkNotNull(context.contentResolver.query(uri, columns,
                "${MediaStore.MediaColumns.RELATIVE_PATH}=?", arrayOf("DCIM/BackupDuck/"), null))
            cursor.use {
                while (it.moveToNext()) {
                    currentCoroutineContext().ensureActive()
                    if (it.getString(2) != context.packageName) continue
                    val size = if (it.isNull(0) || it.getLong(0) < 0) { unknown++; 0L } else it.getLong(0)
                    if (it.getInt(1) == 0) { bytes = Math.addExact(bytes, size); count++ }
                    else { pendingBytes = Math.addExact(pendingBytes, size); pendingCount++ }
                }
            }
        }
        return GalleryUsage(bytes, count, pendingBytes, pendingCount, unknown)
    }
}
