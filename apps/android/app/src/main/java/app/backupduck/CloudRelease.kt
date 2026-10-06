package app.backupduck

import android.content.Context
import android.net.Uri
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import org.json.JSONArray
import org.json.JSONObject

/** What to do with a cloud-verified candidate after re-reading its gallery copy. */
internal enum class CloudReleaseAction { DELETE, MISSING, KEEP }

/**
 * Only a fresh matching read deletes. A copy that is truly gone (not merely
 * unreadable, moved or not ours) releases originals: the cloud matched its SHA-1.
 */
internal fun cloudReleaseAction(verifyError: String?, rowAbsent: Boolean): CloudReleaseAction = when {
    verifyError == null -> CloudReleaseAction.DELETE
    verifyError == "gallery_copy_missing" && rowAbsent -> CloudReleaseAction.MISSING
    else -> CloudReleaseAction.KEEP
}

/**
 * Deletes owned gallery copies once the cloud auditor verified them and the
 * grace passed. Native settings are rechecked at every command. Order per item:
 * fresh verify, durable originals release, MediaStore delete, gallery mark; a
 * crash in between repeats the item on the next sweep.
 */
internal class CloudRelease(private val receiverRoot: String? = null) {
    private var cursor = ""
    private var supersededCursor = ""
    private var lastFailure: String? = null
    private var warned = false
    // Kept or not-owned copies wait for the next service start, not every sweep.
    private val skipped = mutableSetOf<String>()

    /**
     * Releases candidates for up to [budgetMs]. The receiver loop calls this
     * once per pass, and a pass can spend minutes publishing a page of new
     * items; four releases per pass let verified copies pile up faster than
     * they were deleted (27 per hour against 300 publications per hour).
     */
    suspend fun step(context: Context, budgetMs: Long = 45_000): Int {
        val deadline = android.os.SystemClock.elapsedRealtime() + budgetMs
        var total = 0
        while (true) {
            val released = sweep(context)
            if (released <= 0) return if (total > 0) total else released
            total += released
            if (android.os.SystemClock.elapsedRealtime() >= deadline) return total
        }
    }

    private suspend fun sweep(context: Context): Int {
        if (RelayMaintenance.manualActive.get()) return -1
        val root = receiverRoot ?: "${context.filesDir}/receiver"
        // An optional policy must not stop reception; the next tick retries.
        val items = runCatching {
            NativeBridge.request(JSONObject().put("op", "cloud_release_candidates").put("root", root).put("after", cursor)) as JSONArray
        }.getOrElse { error ->
            // Record each distinct failure once; a silent -1 hid a stalled sweep.
            val code = safeError(error as? Exception ?: Exception())
            if (code != lastFailure) runCatching {
                NativeBridge.request(JSONObject().put("op", "record_event").put("receiver", true).put("root", root)
                    .put("code", "cloud_release_candidates_" + code.take(24)))
            }
            lastFailure = code
            return -1
        }
        lastFailure = null
        if (items.length() == 0) { cursor = ""; return sweepSuperseded(context, root) }
        for (index in 0 until items.length()) {
            currentCoroutineContext().ensureActive()
            val candidate = items.getJSONObject(index)
            val id = candidate.getString("id")
            cursor = id
            if (id in skipped) continue
            try {
                release(context, root, id, GalleryCopy.parse(candidate.getJSONObject("copy")))
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (_: Exception) { keep(root, id) }
        }
        return items.length()
    }

    /**
     * Copies in a format the cloud never backs up (AVIF) whose photo was sent
     * again as HEIC and verified in that form. Same verify, release, delete,
     * mark order as a cloud-verified copy.
     */
    private suspend fun sweepSuperseded(context: Context, root: String): Int {
        val items = runCatching {
            NativeBridge.request(JSONObject().put("op", "superseded_candidates").put("root", root).put("after", supersededCursor)) as JSONArray
        }.getOrElse { return -1 }
        if (items.length() == 0) { supersededCursor = ""; return 0 }
        for (index in 0 until items.length()) {
            currentCoroutineContext().ensureActive()
            val candidate = items.getJSONObject(index)
            val id = candidate.getString("id")
            supersededCursor = id
            if (id in skipped) continue
            try {
                release(context, root, id, GalleryCopy.parse(candidate.getJSONObject("copy")), superseded = true)
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (_: Exception) { keep(root, id) }
        }
        return items.length()
    }

    private suspend fun release(context: Context, root: String, id: String, stored: GalleryCopy, superseded: Boolean = false) {
        val releaseOp = if (superseded) "release_superseded" else "release_cloud_verified"
        val verified = try { MediaPublisher.verify(context, stored) }
            catch (cancelled: CancellationException) { throw cancelled }
            catch (error: Exception) {
                when (cloudReleaseAction(error.message, MediaPublisher.absent(context, stored))) {
                    CloudReleaseAction.MISSING -> {
                        command(root, releaseOp, id).put("copy", stored.json()).put("missing", true).let(NativeBridge::request)
                        mark(root, id, "missing", superseded)
                    }
                    else -> keep(root, id)
                }
                return
            }
        currentCoroutineContext().ensureActive()
        command(root, releaseOp, id).put("copy", verified.json()).let(NativeBridge::request)
        val deleted = try { context.contentResolver.delete(Uri.parse(verified.locator), null, null) }
            catch (_: SecurityException) {
                // Ownership was lost, e.g. after a reinstall. Originals are already
                // released; the copy stays until the user removes it.
                skipped += id
                NativeBridge.request(JSONObject().put("op", "record_event").put("receiver", true).put("root", root).put("code", "cloud_release_not_owned"))
                return
            }
        when {
            deleted == 1 -> mark(root, id, "cloud", superseded)
            MediaPublisher.absent(context, verified) -> mark(root, id, "missing", superseded)
            else -> keep(root, id)
        }
        warned = false
    }

    private fun command(root: String, op: String, id: String) = JSONObject().put("op", op).put("root", root).put("id", id)
    private fun mark(root: String, id: String, reason: String, superseded: Boolean = false) {
        if (superseded) NativeBridge.request(command(root, "mark_superseded", id))
        else NativeBridge.request(command(root, "mark_gallery_released", id).put("reason", reason))
    }
    private fun keep(root: String, id: String) {
        skipped += id
        if (!warned) runCatching {
            NativeBridge.request(JSONObject().put("op", "record_event").put("receiver", true).put("root", root).put("code", "cloud_release_waiting"))
        }
        warned = true
    }
}
