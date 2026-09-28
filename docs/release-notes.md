# Beta 25

- Fix cloud playback for JPG+MOV Live Photos by packaging the original JPEG with an MP4/H.264/AAC motion clip. Compatible video samples are copied into the new container; audio is converted when needed. The JPEG image is not re-encoded.
- Correct JPEG Motion Photo metadata for MPF auxiliary images and remove trailing data after the motion clip.
- Distinguish "sent to phone" from "added to phone gallery" on Mac and iOS. Gallery processing failures remain visible and can be retried on the receiver without retransmitting originals.

Mac requires Apple Silicon and macOS 14+. Android requires arm64 and Android 10+.
iOS 17+ is available from source. Existing gallery/cloud copies are not changed by updating the app. Receiver gallery publication and Google Photos cloud backup remain separate states.
