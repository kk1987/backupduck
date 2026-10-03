package app.backupduck

import android.app.Activity
import android.app.Instrumentation
import android.graphics.Bitmap
import android.graphics.Color
import android.os.Bundle
import android.provider.MediaStore
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.UUID

/** Real Android JNI, TLS, codec and MediaStore test with synthetic media only. */
class ReceiverInstrumentation : Instrumentation() {
    private var arguments = Bundle()
    override fun onCreate(arguments: Bundle?) { this.arguments = arguments ?: Bundle(); super.onCreate(arguments); start() }
    override fun onStart() {
        if (arguments.getString("mode") == "motion_container") {
            val result = runCatching { checkMotionContainer(arguments) }
            finish(if (result.isSuccess) Activity.RESULT_OK else Activity.RESULT_CANCELED, Bundle().apply {
                putString("result", result.getOrElse { "FAIL: ${it.javaClass.simpleName}: ${it.message}" })
            })
            return
        }
        if (arguments.getString("mode") == "app_updates_live") {
            val result = runCatching {
                val installed = arguments.getLong("installed", 25)
                val release = checkNotNull(AppUpdates.latest(installed))
                check(release.build > installed && release.url.endsWith("/BackupDuck-${release.version}-arm64.apk"))
                "PASS: ${release.version} build ${release.build} is discoverable"
            }
            finish(if (result.isSuccess) Activity.RESULT_OK else Activity.RESULT_CANCELED, Bundle().apply {
                putString("result", result.getOrElse { "FAIL: ${it.stackTraceToString()}" })
            })
            return
        }
        if (arguments.getString("mode") == "gallery_naming") {
            val result = runCatching { checkGalleryNaming() }
            finish(if (result.isSuccess) Activity.RESULT_OK else Activity.RESULT_CANCELED, Bundle().apply {
                putString("result", result.getOrElse { "FAIL: ${it.stackTraceToString()}" })
            })
            return
        }
        if (arguments.getString("mode") == "media_dates") {
            val result = runCatching { checkMediaDates() }
            finish(if (result.isSuccess) Activity.RESULT_OK else Activity.RESULT_CANCELED, Bundle().apply {
                putString("result", result.getOrElse { "FAIL: ${it.stackTraceToString()}" })
            })
            return
        }
        if (arguments.getString("mode") == "cleanup_unlock") {
            val result = runCatching { checkCleanupUnlock() }
            finish(if (result.isSuccess) Activity.RESULT_OK else Activity.RESULT_CANCELED, Bundle().apply {
                putString("result", result.getOrElse { "FAIL: ${it.stackTraceToString()}" })
            })
            return
        }
        if (arguments.getString("mode") == "app_updates") {
            val result = runCatching { checkAppUpdates(arguments) }
            finish(if (result.isSuccess) Activity.RESULT_OK else Activity.RESULT_CANCELED, Bundle().apply {
                putString("result", result.getOrElse { "FAIL: ${it.stackTraceToString()}" })
            })
            return
        }
        if (arguments.getString("mode") == "receiver_discovery") {
            val result = runCatching { checkReceiverDiscovery() }
            finish(if (result.isSuccess) Activity.RESULT_OK else Activity.RESULT_CANCELED, Bundle().apply {
                putString("result", result.getOrElse { "FAIL: ${it.javaClass.simpleName}: ${it.message}" })
            })
            return
        }
        if (arguments.getString("mode") == "receiver_help") {
            val result = runCatching { checkReceiverHelp(arguments) }
            finish(if (result.isSuccess) Activity.RESULT_OK else Activity.RESULT_CANCELED, Bundle().apply {
                putString("result", result.getOrElse { "FAIL: ${it.javaClass.simpleName}: ${it.message}" })
            })
            return
        }
        if (arguments.getString("mode") == "photos_probe") {
            val result = runCatching { checkPhotosProbe() }
            finish(if (result.isSuccess) Activity.RESULT_OK else Activity.RESULT_CANCELED, Bundle().apply {
                putString("result", result.getOrElse { "FAIL: ${it.javaClass.simpleName}: ${it.message}" })
            })
            return
        }
        if (arguments.getString("mode") == "storage_ui") {
            val result = runCatching { checkStorageUI(arguments) }
            finish(if (result.isSuccess) Activity.RESULT_OK else Activity.RESULT_CANCELED, Bundle().apply {
                putString("result", result.getOrElse { "FAIL: ${it.javaClass.simpleName}: ${it.message}" })
            })
            return
        }
        val results = Bundle()
        var resultCode = Activity.RESULT_CANCELED
        val root = File(targetContext.filesDir, "integration-${UUID.randomUUID()}").apply { mkdirs() }
        val publishedCopies = mutableListOf<android.net.Uri>()
        val publishedNames = mutableSetOf<String>()
        try {
            runBlocking {
                val galleryBefore = GalleryInventory.read(targetContext)
                val photo = File(root, "fixture.jpg")
                val bitmap = Bitmap.createBitmap(128, 96, Bitmap.Config.ARGB_8888).apply { eraseColor(Color.rgb(24, 120, 180)) }
                photo.outputStream().use { check(bitmap.compress(Bitmap.CompressFormat.JPEG, 95, it)) }; bitmap.recycle()
                val movie = File(root, "fixture.mp4")
                context.assets.open("motion.mp4").use { input -> movie.outputStream().use { input.copyTo(it) } }
                val pairing = NativeBridge.request(JSONObject().put("op", "start_receiver").put("root", File(root, "receiver").path)
                    .put("listen", "127.0.0.1:38484").put("capacity", 32 * 1024 * 1024)) as JSONObject
                NativeBridge.request(JSONObject().put("op", "open_sender").put("root", File(root, "sender").path))
                fun profile(directory: File, name: String? = null): JSONObject {
                    val request = JSONObject().put("op", "device_status").put("root", directory.path).put("language", "en")
                    if (name != null) request.put("name", name)
                    return NativeBridge.request(request) as JSONObject
                }
                val receiverDirectory = File(root, "receiver/store")
                val senderDirectory = File(root, "sender")
                val receiverProfile = profile(receiverDirectory, "Amber Otter").getJSONObject("device")
                val senderProfile = profile(senderDirectory, "Moonlit Cedar").getJSONObject("device")
                val exchanged = NativeBridge.request(JSONObject().put("op", "exchange_device").put("pairing", pairing)) as JSONObject
                check(exchanged.getString("name") == "Amber Otter") { "receiver_name_missing" }
                val knownSenders = profile(receiverDirectory).getJSONArray("peers")
                check(knownSenders.length() == 1 && knownSenders.getJSONObject(0).getJSONObject("profile").getString("name") == "Moonlit Cedar") { "sender_name_missing" }
                check(runCatching { profile(senderDirectory, "\n") }.isFailure) { "invalid_name_accepted" }
                check(profile(senderDirectory).getJSONObject("device").getString("name") == "Moonlit Cedar") { "invalid_rename_changed_name" }
                check(profile(senderDirectory, "Kitchen Mac").getJSONObject("device").getString("id") == senderProfile.getString("id")) { "rename_changed_identity" }
                NativeBridge.request(JSONObject().put("op", "exchange_device").put("pairing", pairing))
                check(profile(receiverDirectory).getJSONArray("peers").getJSONObject(0).getJSONObject("profile").getString("name") == "Kitchen Mac") { "rename_not_propagated" }
                // Every fixture shares the name fixture.jpg or fixture.mp4, so publication
                // exercises collision variants. "twin" is a separate asset with the same
                // name; "heic" declares a HEIC name for JPEG bytes.
                val kinds = listOf("photo", "video", "motion", "burst-primary", "burst-secondary", "twin", "heic")
                for (kind in kinds) {
                    val resources = JSONArray()
                    fun resource(file: File, role: String, mime: String, name: String = file.name) = JSONObject().put("role", role).put("filename", name).put("media_type", mime).put("path", file.path)
                    if (kind == "video") resources.put(resource(movie, "video", "video/mp4"))
                    else if (kind == "heic") resources.put(resource(photo, "photo", "image/heic", "fixture.HEIC"))
                    else resources.put(resource(photo, "photo", "image/jpeg"))
                    if (kind == "motion") resources.put(resource(movie, "paired_video", "video/mp4"))
                    val metadata = if (kind.startsWith("burst-")) NativeBridge.request(JSONObject().put("op", "burst_metadata").put("identifier", root.name).put("primary", kind == "burst-primary")) as JSONObject else JSONObject()
                    metadata.put("created_at_ms", "1786761701000")
                    val assetKind = when (kind) { "video", "motion" -> kind; else -> "photo" }
                    NativeBridge.request(JSONObject().put("op", "enqueue").put("receiver_id", pairing.getString("receiver_id"))
                        .put("source_id", "fixture-$kind-${root.name}").put("revision", "1").put("kind", assetKind).put("metadata", metadata).put("resources", resources))
                    val job = NativeBridge.request(JSONObject().put("op", "run_sender").put("pairing", pairing)) as JSONObject
                    check(job.getString("state") == "received") { "receipt_missing" }
                }
                val pending = NativeBridge.request(JSONObject().put("op", "publications")) as JSONArray
                check(pending.length() == kinds.size) { "asset_count" }
                fun fixtureKind(item: JSONObject) = item.getJSONObject("asset").getString("source_id").removeSuffix("-${root.name}").removePrefix("fixture-")
                // Publish in fixture order so the twin and HEIC-named items meet taken names.
                val items = (0 until pending.length()).map(pending::getJSONObject).sortedBy { kinds.indexOf(fixtureKind(it)) }
                var legacyInjected = false
                val publishedLocators = mutableSetOf<String>()
                val namesByKind = mutableMapOf<String, String>()
                for (item in items) {
                    val id = item.getString("id")
                    val kind = fixtureKind(item)
                    val legacy = if (kind == "photo") targetContext.contentResolver.insert(MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY),
                        android.content.ContentValues().apply {
                            put(MediaStore.MediaColumns.DISPLAY_NAME, GalleryNaming.legacyName(item))
                            put(MediaStore.MediaColumns.MIME_TYPE, "image/jpeg")
                            put(MediaStore.MediaColumns.RELATIVE_PATH, "DCIM/BackupDuck/")
                            put(MediaStore.MediaColumns.IS_PENDING, 1)
                        }) else null
                    if (legacy != null) legacyInjected = true
                    val copy = MediaPublisher.publish(targetContext, item)
                    publishedCopies += android.net.Uri.parse(copy.locator)
                    publishedLocators += copy.locator
                    if (legacy != null) check(copy.locator == legacy.toString()) { "legacy_pending_copy_duplicated" }
                    val (actualName, actualMime) = checkNotNull(targetContext.contentResolver.query(android.net.Uri.parse(copy.locator),
                        arrayOf(MediaStore.MediaColumns.DISPLAY_NAME, MediaStore.MediaColumns.MIME_TYPE), null, null, null)?.use { cursor ->
                        if (cursor.moveToFirst()) cursor.getString(0) to cursor.getString(1) else null
                    }) { "delivery_row_missing" }
                    check(actualName == copy.displayName) { "delivery_name_not_recorded" }
                    if (legacy != null) check(actualName == GalleryNaming.legacyName(item)) { "legacy_name_changed" }
                    else check(actualName.startsWith("fixture")) { "original_name_missing" }
                    if (kind == "heic") check(actualName.endsWith(".jpg") && actualMime == "image/jpeg") { "heic_named_jpeg_mislabelled" }
                    publishedNames += actualName
                    namesByKind[kind] = actualName
                    // Publication replay must find the same MediaStore row and name.
                    val replay = MediaPublisher.publish(targetContext, item)
                    check(replay.locator == copy.locator && replay.displayName == copy.displayName) { "replay_changed_publication" }
                    NativeBridge.request(JSONObject().put("op", "processed").put("id", id).put("success", true))
                }
                check(legacyInjected) { "legacy_fixture_missing" }
                check(publishedNames.size == kinds.size && publishedLocators.size == kinds.size) { "delivery_names_not_distinct" }
                check(publishedNames.count { it.endsWith("_MP.jpg") } == 1) { "motion_suffix_missing_or_repeated" }
                val variant = Regex("^fixture \\(\\d+\\)\\.jpg$")
                check(publishedNames.any(variant::matches)) { "collision_variant_missing" }
                check(variant.matches(namesByKind.getValue("twin"))) { "twin_name_not_varied" }
                check((NativeBridge.request(JSONObject().put("op", "publications")) as JSONArray).length() == 0) { "processing_incomplete" }
                var total = 0
                for (collection in listOf(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, MediaStore.Video.Media.EXTERNAL_CONTENT_URI)) {
                    targetContext.contentResolver.query(collection, arrayOf(MediaStore.MediaColumns._ID, MediaStore.MediaColumns.DISPLAY_NAME),
                        "${MediaStore.MediaColumns.RELATIVE_PATH}=?", arrayOf("DCIM/BackupDuck/"), null)?.use { cursor ->
                        while (cursor.moveToNext()) if (cursor.getString(1) in publishedNames) total++
                    }
                }
                check(total == kinds.size) { "publication_duplicate_or_missing" }
                var burstFrames = 0
                var primaryFrames = 0
                val expectedBurst = NativeBridge.request(JSONObject().put("op", "burst_metadata").put("identifier", root.name).put("primary", true)) as JSONObject
                val images = MediaStore.Images.Media.EXTERNAL_CONTENT_URI
                targetContext.contentResolver.query(images, arrayOf(MediaStore.MediaColumns._ID, MediaStore.MediaColumns.DISPLAY_NAME),
                    "${MediaStore.MediaColumns.RELATIVE_PATH}=?", arrayOf("DCIM/BackupDuck/"), null)?.use { cursor ->
                    while (cursor.moveToNext()) if (cursor.getString(1) in publishedNames) {
                        val uri = android.content.ContentUris.withAppendedId(images, cursor.getLong(0))
                        targetContext.contentResolver.openInputStream(uri)?.use { input ->
                            val exif = androidx.exifinterface.media.ExifInterface(input)
                            check(exif.getAttribute(androidx.exifinterface.media.ExifInterface.TAG_DATETIME_ORIGINAL) == "2026:08:15 02:41:41") { "publication_date_missing" }
                            val xmp = exif.getAttribute(androidx.exifinterface.media.ExifInterface.TAG_XMP) ?: ""
                            if (xmp.contains(expectedBurst.getString("burst_group_ref"))) {
                                burstFrames++
                                if (xmp.contains("GCamera:BurstPrimary=\"1\"")) primaryFrames++
                            }
                        }
                    }
                }
                check(burstFrames == 2 && primaryFrames == 1) { "burst_metadata_missing_or_ambiguous" }

                val gallery = GalleryInventory.read(targetContext)
                check(gallery.readyCount == galleryBefore.readyCount + kinds.size && gallery.readyBytes > galleryBefore.readyBytes) { "gallery_usage_missing_or_duplicate" }
                val pendingID = UUID.randomUUID().toString().replace("-", "") + UUID.randomUUID().toString().replace("-", "")
                val pendingURI = checkNotNull(targetContext.contentResolver.insert(images, android.content.ContentValues().apply {
                    put(MediaStore.MediaColumns.DISPLAY_NAME, "BD_${pendingID}.jpg")
                    put(MediaStore.MediaColumns.MIME_TYPE, "image/jpeg")
                    put(MediaStore.MediaColumns.RELATIVE_PATH, "DCIM/BackupDuck/")
                    put(MediaStore.MediaColumns.IS_PENDING, 1)
                }))
                val pendingUsage = GalleryInventory.read(targetContext)
                check(pendingUsage.readyCount == gallery.readyCount && pendingUsage.pendingCount == gallery.pendingCount + 1) { "pending_gallery_usage_incorrect" }
                targetContext.contentResolver.delete(pendingURI, null, null)
                check(GalleryInventory.read(targetContext) == gallery) { "gallery_removal_not_reflected" }
                val receiverRoot = File(root, "receiver").path
                val originalUsage = NativeBridge.request(JSONObject().put("op", "receiver_storage_usage").put("root", receiverRoot)) as JSONObject
                check(originalUsage.getLong("ready_bytes") == photo.length() + movie.length() && originalUsage.getLong("partial_bytes") == 0L) { "original_usage_not_deduplicated" }
                NativeBridge.request(JSONObject().put("op", "stop_receiver"))
                check(!(NativeBridge.request(JSONObject().put("op", "receiver_status")) as JSONObject).getBoolean("running"))
                // The aborted listener releases its writer on the next runtime turn.
                var archiveReady = false
                repeat(40) {
                    if (!archiveReady) {
                        archiveReady = runCatching { NativeBridge.request(JSONObject().put("op", "archive_batch").put("root", receiverRoot)) }.isSuccess
                        if (!archiveReady) kotlinx.coroutines.delay(50)
                    }
                }
                check(archiveReady) { "offline_archive_unavailable" }
                val archiveFile = File(root, "originals.zip")
                val archive = OriginalArchive.export(targetContext, android.net.Uri.fromFile(archiveFile), receiverRoot)
                check(archive.ids.size == kinds.size) { "archive_count" }
                archiveFile.writeBytes(byteArrayOf(0, 1, 2))
                check(runCatching { OriginalArchive.verify(targetContext, archive) }.isFailure) { "tampered_archive_accepted" }
                check((NativeBridge.request(JSONObject().put("op", "archive_batch").put("root", receiverRoot)) as JSONArray).length() == kinds.size) { "premature_reclamation" }
                val verified = OriginalArchive.export(targetContext, android.net.Uri.fromFile(archiveFile), receiverRoot)
                val released = NativeBridge.request(JSONObject().put("op", "release_archived").put("root", verified.receiverRoot).put("ids", JSONArray(verified.ids)).put("verified", JSONArray(verified.resources.toList()))) as JSONObject
                check(released.getLong("bytes") > 0) { "reclamation_missing" }
                check((NativeBridge.request(JSONObject().put("op", "archive_batch").put("root", receiverRoot)) as JSONArray).length() == 0)
                val overview = NativeBridge.request(JSONObject().put("op", "receiver_settings").put("root", receiverRoot)) as JSONObject
                check(overview.getJSONObject("counts").getInt("received") == kinds.size && overview.getLong("used_bytes") == 0L) { "receipt_or_budget_lost" }
                val afterReclaim = NativeBridge.request(JSONObject().put("op", "receiver_storage_usage").put("root", receiverRoot)) as JSONObject
                check(afterReclaim.getLong("ready_bytes") == 0L && afterReclaim.getLong("partial_bytes") == 0L) { "reclaimed_usage_retained" }
                check(GalleryInventory.read(targetContext) == gallery) { "original_reclamation_changed_gallery" }
                val restarted = NativeBridge.request(JSONObject().put("op", "start_receiver").put("root", receiverRoot)
                    .put("listen", "127.0.0.1:38484").put("capacity", 32 * 1024 * 1024)) as JSONObject
                check(restarted.getString("receiver_id") == pairing.getString("receiver_id")) { "identity_changed_after_archive" }
                check(profile(receiverDirectory).getJSONObject("device").toString() == receiverProfile.toString()) { "profile_changed_after_restart" }
                check(profile(senderDirectory).getJSONArray("peers").getJSONObject(0).getJSONObject("profile").getString("name") == "Amber Otter") { "offline_peer_lost" }
                check((NativeBridge.request(JSONObject().put("op", "receiver_overview")) as JSONObject).getInt("received") == kinds.size)
                check(NativeBridge.request(JSONObject().put("op", "run_sender").put("pairing", pairing)) == JSONObject.NULL) { "received_items_requeued" }
                checkRelayRetention(root, photo, movie, restarted, publishedCopies)
                results.putString("result", "PASS: opt-in relay, verified derived copies, changed/missing copy preservation and durable receipts; original/gallery space separation, deduplicated originals, pending publication and reclamation; two grouped burst frames, one primary, original retention; durable names, bidirectional profile exchange, rename and invalid-name rejection; offline verified archive, tamper rejection, original reclamation, receipt retention; Kotlin JNI, paired TLS, photo/video/motion receipt, codec publication and dedupe")
            }
            resultCode = Activity.RESULT_OK
        } catch (error: Throwable) {
            results.putString("result", "FAIL: ${error.javaClass.simpleName}: ${error.message?.take(120)}")
        } finally {
            runCatching { NativeBridge.request(JSONObject().put("op", "stop_receiver")) }
            // Only remove this test's newly-created media, never user originals.
            for (uri in publishedCopies) runCatching { targetContext.contentResolver.delete(uri, null, null) }
            root.deleteRecursively()
        }
        // finish() may terminate the instrumentation process before finally runs.
        // Report completion only after our fixture cleanup has finished.
        finish(resultCode, results)
    }
}

private fun checkGalleryNaming(): String {
    val id = "a1b2c3d4e5f6a7b8" + "0".repeat(48)
    fun item(kind: String, filename: String, captured: String? = "1786761701000", burst: Boolean = false): JSONObject {
        val metadata = JSONObject()
        if (captured != null) metadata.put("created_at_ms", captured)
        if (burst) metadata.put("burst_group_ref", "synthetic")
        return JSONObject().put("id", id).put("asset", JSONObject().put("kind", kind).put("metadata", metadata)
            .put("resources", JSONArray().put(JSONObject().put("filename", filename))))
    }
    val photo = item("photo", "IMG_1234.HEIC")
    check(GalleryNaming.preferred(photo, "image/heic") == "IMG_1234.HEIC")
    check(GalleryNaming.preferred(photo, "image/jpeg") == "IMG_1234.jpg")
    check(GalleryNaming.preferred(photo, "image/heic", variant = 1) == "IMG_1234 (1).HEIC")
    check(GalleryNaming.preferred(item("photo", "IMG_1234.JPEG"), "image/jpeg") == "IMG_1234.JPEG")
    val motion = item("motion", "IMG_1234.HEIC")
    check(GalleryNaming.preferred(motion, "image/heic") == "IMG_1234_MP.HEIC")
    check(GalleryNaming.preferred(motion, "image/jpeg") == "IMG_1234_MP.jpg")
    check(GalleryNaming.preferred(motion, "image/heic", variant = 2) == "IMG_1234 (2)_MP.HEIC")
    check(GalleryNaming.preferred(item("photo", "IMG_1235.HEIC", burst = true), "image/heic") == "IMG_1235.HEIC")
    check(GalleryNaming.preferred(item("photo", "IMG_1235.HEIC", burst = true), "image/jpeg") == "IMG_1235.jpg")
    check(GalleryNaming.preferred(item("video", "clip.mov"), "video/quicktime") == "clip.mov")
    check(GalleryNaming.preferred(item("photo", "照片 2024.HEIC"), "image/heic") == "照片 2024.HEIC")
    check(GalleryNaming.preferred(item("photo", "scan.tiff"), "image/tiff") == "scan.tiff")
    check(GalleryNaming.preferred(item("photo", "a:b?.HEIC"), "image/heic") == "a_b_.HEIC")
    check(GalleryNaming.preferred(item("photo", ".."), "image/jpeg") == "BD_a1b2c3d4.jpg")
    check(GalleryNaming.preferred(item("photo", ".HEIC"), "image/heic") == "BD_a1b2c3d4.HEIC")
    // Four-byte code points: truncation must stay on code point boundaries.
    val long = item("motion", "\uD83D\uDCF7".repeat(300) + ".HEIC")
    val variants = (0..GalleryNaming.MAX_COLLISION_VARIANTS).map { GalleryNaming.preferred(long, "image/heic", it) }
    check(variants.all { it.toByteArray(Charsets.UTF_8).size <= 255 && it.endsWith("_MP.HEIC") }) { "name_too_long" }
    check(variants.distinct().size == variants.size) { "variants_not_distinct" }
    check(variants[0] == "\uD83D\uDCF7".repeat(60) + "_MP.HEIC") { "truncation_not_maximal" }
    check(variants.all { name -> name.indices.all { index ->
        !name[index].isHighSurrogate() || index + 1 < name.length && name[index + 1].isLowSurrogate() } }) { "split_code_point" }
    val resume = GalleryNaming.resumeCandidates(motion, "image/heic")
    check(resume.first() == "IMG_1234_MP.HEIC" && "IMG_1234 (20)_MP.HEIC" in resume)
    check(GalleryNaming.legacyName(motion, "image/heic") in resume && GalleryNaming.name(motion, 4, "image/heic") in resume)
    check("BD_20260815_024141Z_a1b2.heic" in resume) { "motion_name_without_mp_missing" }
    check(GalleryNaming.name(photo) == "BD_20260815_024141Z_a1b2.HEIC")
    check(GalleryNaming.name(item("photo", "IMG_1234.HEIC", captured = null)).startsWith("BD_undated_"))
    check(GalleryNaming.legacyName(photo) == "BD_${id}.HEIC")
    return "PASS: original names, output-format extensions, motion suffixes, collision variants, UTF-8 limits and fallback names"
}
