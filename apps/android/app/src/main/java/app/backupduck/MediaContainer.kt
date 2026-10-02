package app.backupduck

import java.io.File

/** Read at most 4 KiB: camera-export filenames can disagree with their bytes. */
internal object MediaContainer {
    fun imageMime(source: File, declared: String): String = source.inputStream().use { input ->
        val header = ByteArray(4096)
        var count = 0
        while (count < header.size) {
            val read = input.read(header, count, header.size - count)
            if (read < 0) break
            count += read
        }
        imageMime(header.copyOf(count)) ?: declared
    }

    internal fun imageMime(header: ByteArray): String? {
        if (header.size >= 3 && header[0] == 0xff.toByte() && header[1] == 0xd8.toByte() && header[2] == 0xff.toByte()) return "image/jpeg"
        val png = byteArrayOf(0x89.toByte(), 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a)
        if (header.size >= png.size && header.copyOfRange(0, png.size).contentEquals(png)) return "image/png"
        if (header.size < 16 || String(header, 4, 4, Charsets.US_ASCII) != "ftyp") return null
        val size = header.take(4).fold(0L) { value, byte -> (value shl 8) or (byte.toLong() and 255) }
        if (size < 16 || size > header.size || size % 4 != 0L) return null
        val heicBrands = setOf("heic", "heix", "hevc", "hevx")
        // The minor-version field at 12 is not a compatible brand.
        val brands = listOf(8) + (16 until size.toInt() step 4).toList()
        return if (brands.any { String(header, it, 4, Charsets.US_ASCII) in heicBrands }) "image/heic" else null
    }
}
