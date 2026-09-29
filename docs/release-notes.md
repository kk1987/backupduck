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
