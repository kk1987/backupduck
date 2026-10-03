# Unreleased

- Pixel gallery copies keep their original Apple Photos filenames, such as `IMG_1234.HEIC`, instead of `BD_<date>_<id>` names. The extension follows the saved format, Motion Photos end in `_MP`, and each burst frame keeps its own name. Taken names get ` (1)` through ` (20)` before falling back to the dated `BD_` name. Existing gallery copies are not renamed.
- The capture-date audit script accepts the Pixel gallery receipt export (`--receipt-json`) to match original filenames to received assets.
- HEIC burst frames are published on Pixel as HEIC without re-encoding, keeping HDR gain maps and metadata, e.g. `IMG_1234.HEIC`. Only the burst markers are added to the primary XMP. A new receiver setting, "Convert HEIC bursts to JPEG" (off by default), restores the previous JPEG copies; unsupported HEIC layouts still fall back to JPEG. Google Photos cloud grouping of HEIC bursts is unverified.
- The Pixel receiver records a SHA-1 of each gallery copy and tracks a cloud verification state for it, reported by a future Mac auditor over two new authenticated routes. Existing gallery copies gain the SHA-1 in the background. The receipt export includes `sha1` and `cloud_state`. Nothing is deleted based on this state yet.
- macOS can check the Pixel's gallery copies against Google Photos (Settings > Google Photos). Pick a `cookies.txt` exported from a signed-in browser, then run a check or a dry run, or let it check every 10 minutes. Verdicts go to the receiver and show in the transfer list (Backed up to Google Photos, counting against Google storage, or not found yet) and as a filled check in the library grid. Backup pauses by default when newly uploaded photos count against Google storage. This uses an undocumented Google interface; results are advisory and nothing is deleted. iOS and Android do not include it.
- Stop retrying photos that repeatedly fail preparation for the same item-specific reason. After five attempts they are marked as needing attention instead of retrying every five minutes; network, storage-budget and receiver-availability waits still retry automatically. Changing storage settings gives parked photos another chance.
- An item larger than the receiver's entire storage budget, or larger than the local staging cache budget, now fails with a specific reason instead of waiting indefinitely.
- Hidden photos and items outside limited photo access are reported separately from missing originals.
- iPhone and Mac now back up the Photos Hidden album by default. A new "Include Hidden Photos" setting turns this off; hidden items are then reported as excluded instead of as missing originals. A scan already running keeps the photos it started with, so use "Check existing photos again" after changing the setting.
- Raise the default staging cache from 5 GB to 20 GB and the Android receiver's first-start budget from 6 GB to 10 GB, so migrating a whole library does not stall on small budgets. Saved settings are kept; existing receivers keep their budget. The Mac offers a one-time "Apply 20 GB cache" preset in Settings.
- A new "All issues" filter on Mac and iPhone lists photos that stopped retrying preparation, with their reason and attempt count, next to failed transfers. Each photo can be retried or skipped, and "Retry all" retries everything listed. The Mac overview and menu bar show the count.
- Scan library again and re-selection no longer upload a second copy of photos that were already backed up. Favorite, location, date and Photos edits no longer count as new content; a new "Back up edited photos again" setting (off by default) re-checks edited photos, and identical originals are still never published twice. Use "Back up already received items again" to force another copy.

# RC 2 — BackupDuck Android signing identity (0.2.0-rc.2)

- Use a dedicated Android release certificate named BackupDuck. The old certificate is no longer used for new releases.
- Android RC.1 cannot update in place to RC.2 because its signing certificate differs. Preserve originals and receiver records before switching to a fresh installation. Photos already saved to the gallery or cloud are not actively removed by the installer.
- Keep the existing iOS/macOS identities and protocol 2. macOS RC.1 can update normally through Sparkle.

# RC 1 — 备份鸭 / BackupDuck (0.2.0-rc.1)

- Launch BackupDuck / 备份鸭 with the duck icon, a renamed repository and website, and unified app identifier `app.backupduck` on iOS, macOS and Android.
- Install the new app on every sender and Pixel receiver, grant permissions and pair again. The old app database is not migrated. Existing apps, original photos and already saved gallery/cloud copies are not removed or modified.
- Rename Rust crates, native libraries, C ABI, discovery, headers, metadata and export identifiers. Protocol 2 uses `/v2/` routes and a new bundle signature; old apps cannot communicate with the new receiver.
- Keep the existing lossless photo/video processing. Fallback delivery filenames now start with `BD_`; preserved original filenames still follow the current naming rules.
- Downloads and update checks use https://backupduck.vercel.app. iOS TestFlight uses version 0.2.0 (build 44).

# Beta 38

- Recognize JPEG, PNG and HEIC image data before Android date handling and gallery publication, even when a camera export has an incorrect filename extension. Supported HEIC Live Photos keep their original image, video and audio without compatibility conversion. Retained failed originals can be retried after updating.
- Apple senders prefer the PhotoKit resource type when declaring the media format instead of relying only on the filename extension.

# Beta 37

- Publish burst JPEGs that contain multiple standard XMP packets without recompressing the image. Each existing packet retains its unrelated metadata and receives the same burst group marker. Previously received failures can be retried on Pixel without sending the originals again.

# Beta 36

- Distinguish specific lossless burst-JPEG packaging failures, including MPF and XMP variants, while retaining every received original for retry. Receiver diagnostics and browser details now expose the failure category without exporting photo content.
- Separate the Pixel's gallery-record export from log retention and log export in Settings.

# Beta 35

- Restore lossless packaging for older iPhone Live Photos whose paired QuickTime movie starts with `wide` instead of `ftyp`. The Pixel now accepts a complete `wide` + `mdat` + `moov` container, keeping the JPEG image data and original video/audio bytes. Previously received items can be retried without retransmission or compatibility conversion.

# Beta 34

- Add a Pixel settings export for gallery publication records. It lists saved filenames, byte sizes, checksums and times without exporting photo contents or login credentials, so users can reconcile backups with any destination.
- Persist the exact published filename for new transfers so a local Chrome audit can compare Pixel receipts with the Google Photos timeline. Existing receipts are resolved from the Android media index when available.

# Beta 33

- On the Pixel receiver, pairing can now start receiving automatically. Choose the iPhone code or Mac scanner even when the receiver is stopped; the requested pairing step opens once the receiver is ready.
- Explain the pairing choices and show a useful error when the receiver cannot start, instead of leaving the pairing buttons disabled without a reason.

# Beta 32

- Show live Pixel battery temperature, charge level, free space, and BackupDuck original usage in browser management, including the existing heat-pause state and threshold.
- Keep the transfer table to file, state, capture time, and size; receive-complete time remains in item details. Use “实况” consistently in Chinese across the apps, and identify burst frames from their metadata, with a burst filter and primary-frame detail.

# Beta 31

- Simplify the Pixel browser transfer table and add a per-item detail view for sender, media type, three local timestamps, receive progress, retained-original state, and processing guidance.
- Add a status guide and receiver-side numbered pagination with 20/50/100 items per page, page selection, and direct jump. Filters apply to the full history before pagination.

# Beta 30

- Redesign the Pixel browser dashboard with a compact overview, storage status, and a transfer table that filters by state and media type.
- Show capture, receive-complete, and phone-gallery publication times when available. New receiver events record their timestamps; older records stay marked as not recorded.
- Refresh visible data automatically every eight seconds, with an on/off control and manual refresh. Loading older pages pauses list refresh to keep the table stable; the overview continues to update.

# Beta 29

- On Mac and iOS, the “Back up selected” confirmation now includes an off-by-default option to back up items already received by the current Pixel again. Pending transfers are never duplicated.
- A repeat backup creates a new gallery copy. When the Pixel still has the same verified original data, the transfer reuses it instead of uploading identical bytes again.

# Beta 28

- Add optional browser management to the Pixel receiver. From a computer on the same trusted Wi-Fi, view live receiver totals and transfer history, filter failures, and retry failed gallery processing.
- The page is off by default and uses a temporary code shown on the phone. It serves local, unencrypted HTTP while enabled; do not use it on public Wi-Fi. Gallery publication still does not confirm Google Photos cloud backup.

# Beta 27

- Add a receiver setting for Live Photo compatibility conversion, off by default. Every Live Photo first tries to retain its original image and video. Only an input the direct packager cannot handle may be converted when the setting is on.
- When conversion is off and required, keep the received originals and show “Compatibility conversion needed” in Transfers. The failed item can be retried after enabling the setting; it is not counted as published to the phone gallery.

# Beta 26

- Preserve the original video and audio when packaging supported JPG+MOV Live Photos. Four cloud samples that were static or unable to load their animation were verified to play in Google Photos on iOS after correcting only their Motion Photo container metadata and layout, with no image, video, or audio encoding.
- Keep the JPEG container fixes from beta.25: describe the MPF auxiliary image and put the video at the exact end of the file. Codec conversion remains a fallback for unsupported inputs instead of running for every JPG+MOV pair.

Mac requires Apple Silicon and macOS 14+. Android requires arm64 and Android 10+.
iOS 17+ is available from source. Existing gallery/cloud copies are not changed by updating the app. Receiver gallery publication and Google Photos cloud backup remain separate states.
