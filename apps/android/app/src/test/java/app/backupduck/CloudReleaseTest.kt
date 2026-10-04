package app.backupduck

import org.junit.Assert.assertEquals
import org.junit.Test

class CloudReleaseTest {
    @Test
    fun onlyFreshProofDeletesAndOnlyAbsentRowsCountAsMissing() {
        assertEquals(CloudReleaseAction.DELETE, cloudReleaseAction(null, rowAbsent = false))
        assertEquals(CloudReleaseAction.MISSING, cloudReleaseAction("gallery_copy_missing", rowAbsent = true))
        // Present but pending, moved, trashed or not ours: keep everything.
        assertEquals(CloudReleaseAction.KEEP, cloudReleaseAction("gallery_copy_missing", rowAbsent = false))
        assertEquals(CloudReleaseAction.KEEP, cloudReleaseAction("gallery_copy_changed", rowAbsent = true))
        assertEquals(CloudReleaseAction.KEEP, cloudReleaseAction("gallery_copy_ambiguous", rowAbsent = true))
        assertEquals(CloudReleaseAction.KEEP, cloudReleaseAction(null.toString(), rowAbsent = true))
    }

    private fun item(cloud: String = "unknown", released: Boolean = false, originals: Boolean = false, reason: String? = null) =
        HistoryItem(1, "id", "IMG_0001.JPG", "photo", 1, 1, "received", "complete", null, originals, reason,
            cloudState = cloud, galleryReleased = released)

    @Test
    fun historyPrefersCloudStatesOverLocalRetention() {
        assertEquals(R.string.history_cloud_released, item("verified", released = true, originals = true, reason = "cloud").rowStatus)
        assertEquals(R.string.history_cloud_verified, item("verified", originals = true, reason = "gallery").rowStatus)
        assertEquals(R.string.history_cloud_elsewhere, item("verified_elsewhere").rowStatus)
        assertEquals(R.string.history_cloud_released, item("verified_elsewhere", released = true, originals = true, reason = "cloud").rowStatus)
        assertEquals(R.string.history_cloud_quota, item("verified_counts_against_quota").rowStatus)
        assertEquals(R.string.history_cloud_missing, item("missing").rowStatus)
        assertEquals(R.string.history_relay_reclaimed, item("pending", originals = true, reason = "gallery").rowStatus)
        assertEquals(R.string.history_archived, item(originals = true, reason = "archive").rowStatus)
        assertEquals(R.string.filter_published, item("pending").rowStatus)
        assertEquals(R.string.receiver_item_receiving, item().copy(receipt = "receiving", processing = "not_requested").rowStatus)
    }
}
