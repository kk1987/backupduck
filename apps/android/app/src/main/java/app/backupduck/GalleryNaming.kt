package app.backupduck

import org.json.JSONObject
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.Locale

/**
 * Gallery file names. New copies keep the sender's original filename; dated
 * `BD_` names remain the collision fallback and match older publications.
 * The full asset identity remains in the receiver store.
 */
internal object GalleryNaming {
    const val MAX_COLLISION_VARIANTS = 20
    private const val MAX_NAME_BYTES = 255
    // Widest counter plus the motion suffix, reserved so every variant fits.
    private const val RESERVED_SUFFIX = " (99)_MP"
    private val dateFormat = DateTimeFormatter.ofPattern("uuuuMMdd_HHmmss'Z'", Locale.ROOT).withZone(ZoneOffset.UTC)
    private val assetID = Regex("[0-9a-f]{64}")
    private val safeExtension = Regex("[A-Za-z0-9]{1,12}")
    private val reservedCharacters = Regex("[\"*:<>?|]")
    private val mimeExtensions = mapOf(
        "image/jpeg" to "jpg", "image/heic" to "heic", "image/heif" to "heic", "image/png" to "png",
        "image/gif" to "gif", "image/webp" to "webp", "image/x-adobe-dng" to "dng",
        "video/quicktime" to "mov", "video/mp4" to "mp4",
    )
    // Other spellings of the same format; the original's spelling is kept.
    private val extensionAliases = mapOf("jpg" to setOf("jpg", "jpeg"), "heic" to setOf("heic", "heif"))

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

    /** Dated names: the collision fallback, plus pre-MP motion names of older copies. */
    fun candidates(item: JSONObject, outputMime: String? = null): List<String> {
        val current = listOf(4, 8, 12, 16, 32, 64).map { name(item, it, outputMime) }
        if (item.getJSONObject("asset").getString("kind") != "motion") return current
        return current + current.map { it.replace("_MP.", ".") }
    }

    /** Dated names, or none: they never existed for an unsafe original extension. */
    fun datedOrEmpty(item: JSONObject, outputMime: String? = null): List<String> =
        runCatching { candidates(item, outputMime) }.getOrDefault(emptyList())

    /** The legacy name, or null when the original extension was never publishable. */
    fun legacyOrNull(item: JSONObject, outputMime: String? = null): String? =
        runCatching { legacyName(item, outputMime) }.getOrNull()

    /** The extension of the bytes actually written, spelled like the original when it agrees. */
    fun extensionFor(outputMime: String?, originalFilename: String): String {
        val original = baseName(originalFilename).substringAfterLast('.', "")
        val mapped = mimeExtensions[outputMime?.lowercase(Locale.ROOT)]
            ?: return if (safeExtension.matches(original)) ".$original" else ""
        val accepted = extensionAliases[mapped] ?: setOf(mapped)
        return if (original.lowercase(Locale.ROOT) in accepted) ".$original" else ".$mapped"
    }

    /** The original filename without its extension, safe as a MediaStore display name. */
    fun originalStem(item: JSONObject): String {
        val id = item.getString("id")
        check(assetID.matches(id)) { "invalid_asset_id" }
        val base = baseName(originalFilename(item))
        val stem = (if ('.' in base) base.substringBeforeLast('.') else base).trim(::trimmable)
        return stem.ifEmpty { "BD_${id.take(8)}" }
    }

    /** Variant 0 is the original name; collisions add " (n)" before any _MP suffix. */
    fun preferred(item: JSONObject, outputMime: String?, variant: Int = 0): String {
        check(variant in 0..MAX_COLLISION_VARIANTS) { "invalid_name_variant" }
        val extension = extensionFor(outputMime, originalFilename(item))
        val budget = MAX_NAME_BYTES - utf8Length(RESERVED_SUFFIX) - utf8Length(extension)
        val stem = truncate(originalStem(item), budget).ifEmpty { "BD_${item.getString("id").take(8)}" }
        val counter = if (variant == 0) "" else " ($variant)"
        // Motion Photo readers expect MP immediately before the extension.
        val motion = if (item.getJSONObject("asset").getString("kind") == "motion") "_MP" else ""
        return "$stem$counter$motion$extension"
    }

    /** Every name this asset may have been published under, newest scheme first. */
    fun resumeCandidates(item: JSONObject, outputMime: String?): List<String> =
        ((0..MAX_COLLISION_VARIANTS).map { preferred(item, outputMime, it) } +
            datedOrEmpty(item, outputMime) + listOfNotNull(legacyOrNull(item, outputMime))).distinct()

    private fun originalFilename(item: JSONObject): String =
        item.getJSONObject("asset").getJSONArray("resources").getJSONObject(0).optString("filename")

    // Leading dots stay until the extension is split, so ".HEIC" has no stem.
    private fun baseName(filename: String): String = filename.substringAfterLast('/').substringAfterLast('\\')
        .filterNot(Char::isISOControl).replace(reservedCharacters, "_").trimEnd(::trimmable).trimStart(Char::isWhitespace)

    private fun trimmable(character: Char) = character.isWhitespace() || character == '.'

    private fun utf8Length(value: String) = value.toByteArray(Charsets.UTF_8).size

    /** Cut by code points, never inside a surrogate pair. */
    private fun truncate(stem: String, limit: Int): String {
        if (utf8Length(stem) <= limit) return stem
        val out = StringBuilder()
        var bytes = 0
        var index = 0
        while (index < stem.length) {
            val point = stem.codePointAt(index)
            val size = utf8Length(String(Character.toChars(point)))
            if (bytes + size > limit) break
            out.appendCodePoint(point); bytes += size; index += Character.charCount(point)
        }
        return out.toString().trimEnd(::trimmable)
    }
}
