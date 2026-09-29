# Beta 33

- On the Pixel receiver, pairing can now start receiving automatically. Choose the iPhone code or Mac scanner even when the receiver is stopped; the requested pairing step opens once the receiver is ready.
- Explain the pairing choices and show a useful error when the receiver cannot start, instead of leaving the pairing buttons disabled without a reason.

# Beta 32

- Show live Pixel battery temperature, charge level, free space, and PhotoBridge original usage in browser management, including the existing heat-pause state and threshold.
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
