package app.backupduck

import android.content.Context
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import org.json.JSONArray
import org.json.JSONObject

/**
 * Receiver-only relay: native settings are rechecked at the destructive commit.
 * Also adds SHA-1 to gallery evidence recorded before cloud verification existed.
 */
internal class GalleryRetention(private val receiverRoot: String? = null) {
    private var cursor = ""
    private var warned = false
    // Copies that could not be re-read are retried after the service restarts,
    // not re-hashed on every sweep.
    private val backfillSkipped = mutableSetOf<String>()
    suspend fun step(context: Context) {
        val root = receiverRoot ?: "${context.filesDir}/receiver"
        val items = NativeBridge.request(JSONObject().put("op", "gallery_candidates").put("root", root).put("after", cursor)) as JSONArray
        if (items.length() == 0) { cursor = ""; return }
        for (index in 0 until items.length()) {
            currentCoroutineContext().ensureActive()
            val candidate = items.getJSONObject(index)
            val item = candidate.getJSONObject("publication")
            cursor = item.getString("id")
            val stored = candidate.optJSONObject("copy")
            if (candidate.optBoolean("confirmed", false) && stored != null && !stored.has("sha1")) {
                if (cursor in backfillSkipped) continue
                try {
                    val proof = MediaPublisher.verify(context, GalleryCopy.parse(stored))
                    NativeBridge.request(JSONObject().put("op", "gallery_sha1").put("root", root).put("id", cursor).put("copy", proof.json()))
                } catch (cancelled: CancellationException) { throw cancelled }
                catch (_: Exception) { backfillSkipped += cursor }
                continue
            }
            try {
                val asset = item.getJSONObject("asset")
                if (stored == null && (asset.getString("kind") == "motion" || asset.optJSONObject("metadata")?.has("burst_group_ref") == true)) {
                    // Older transformed copies have no durable byte evidence.
                    // Re-encoding on every sweep is both inconclusive and costly.
                    error("relay_evidence_missing")
                }
                val proof = if (stored == null) MediaPublisher.publish(context, item, existingOnly = true)
                    else MediaPublisher.verify(context, GalleryCopy.parse(stored))
                NativeBridge.request(JSONObject().put("op", if (candidate.optBoolean("confirmed", false)) "release_gallery" else "gallery_publication")
                    .put("root", root).put("id", cursor).put("copy", proof.json()))
                warned = false
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (_: Exception) {
                // Never recreate a removed gallery copy to justify deleting originals.
                if (!warned) NativeBridge.request(JSONObject().put("op", "record_event").put("receiver", true).put("root", root).put("code", "relay_verification_waiting"))
                warned = true
            }
        }
    }
}
