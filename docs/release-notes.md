# Beta 26

- Preserve the original video and audio when packaging supported JPG+MOV Live Photos. Four cloud samples that were static or unable to load their animation were verified to play in Google Photos on iOS after correcting only their Motion Photo container metadata and layout, with no image, video, or audio encoding.
- Keep the JPEG container fixes from beta.25: describe the MPF auxiliary image and put the video at the exact end of the file. Codec conversion remains a fallback for unsupported inputs instead of running for every JPG+MOV pair.

Mac requires Apple Silicon and macOS 14+. Android requires arm64 and Android 10+.
iOS 17+ is available from source. Existing gallery/cloud copies are not changed by updating the app. Receiver gallery publication and Google Photos cloud backup remain separate states.
