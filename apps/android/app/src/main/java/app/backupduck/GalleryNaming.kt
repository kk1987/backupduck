package app.backupduck

import org.json.JSONObject
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.Locale

/** Stable, readable delivery names. The full asset identity remains in the receiver store. */
internal object GalleryNaming {
    private val dateFormat = DateTimeFormatter.ofPattern("uuuuMMdd_HHmmss'Z'", Locale.ROOT).withZone(ZoneOffset.UTC)
    private val assetID = Regex("[0-9a-f]{64}")
    private val safeExtension = Regex("[A-Za-z0-9]{1,12}")

    private fun extension(item: JSONObject, outputMime: String? = null): String {
        val asset = item.getJSONObject("asset")
        if (asset.getString("kind") == "motion") return if (outputMime == "image/heic") ".heic" else ".jpg"
        if (asset.optJSONObject("metadata")?.has("burst_group_ref") == true) return ".jpg"
        val resource = asset.getJSONArray("resources").getJSONObject(0)
        val raw = resource.getString("filename").substringAfterLast('.', "")
        if (raw.isEmpty()) return ""
        check(safeExtension.matches(raw)) { "unsupported_extension" }
        return ".$raw"
    }

    fun legacyName(item: JSONObject, outputMime: String? = null): String = "BD_${item.getString("id")}${extension(item, outputMime)}"

    fun name(item: JSONObject, suffixLength: Int = 4, outputMime: String? = null): String {
        val id = item.getString("id")
        check(assetID.matches(id)) { "invalid_asset_id" }
        check(suffixLength in listOf(4, 8, 12, 16, 32, 64)) { "invalid_suffix_length" }
        val asset = item.getJSONObject("asset")
        val captured = MediaDates.captured(asset.optJSONObject("metadata"))
        val date = captured?.let { dateFormat.format(Instant.ofEpochMilli(it)) } ?: "undated"
        // The Motion Photo format recommends an MP suffix. Readers may ignore
        // motion metadata when a delivery name does not follow that pattern.
        val motion = if (asset.getString("kind") == "motion") "_MP" else ""
        return "BD_${date}_${id.take(suffixLength)}${motion}${extension(item, outputMime)}"
    }

    fun candidates(item: JSONObject, outputMime: String? = null): List<String> {
        val current = listOf(4, 8, 12, 16, 32, 64).map { name(item, it, outputMime) }
        // Retain the previous names for interrupted publications. New copies
        // choose a current name first; confirmed copies use their stored URI.
        if (item.getJSONObject("asset").getString("kind") != "motion") return current
        return current + current.map { it.replace("_MP.", ".") }
    }
}
