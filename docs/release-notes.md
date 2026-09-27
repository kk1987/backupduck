# Beta 19

- Folder sorting opens directly in a native macOS menu that stays open while backup status refreshes.
- Flat and folder views show each file’s modification date and time beside its size.
- Rescanning an unavailable folder reports the connection or permission problem immediately, clears stale waiting feedback, and preserves the file inventory. Folder paths remain visible when the drive is disconnected.
- Source-level errors no longer claim that files failed to back up. File warnings appear only for recorded file issues.

- Transfer rows and the fixed backup footer retain their height across preparation, transfer, retry and receipt updates. Progress bars and retry information share existing status lines without empty placeholder rows. The footer shows connection/storage warnings only when they block the whole backup.
- HEIC Live Photo packaging merges motion markers into the primary image’s existing metadata, preserving auxiliary metadata and original media payloads. New Motion Photo delivery names follow the MP filename convention. Existing received copies are not automatically rewritten.

Mac requires Apple Silicon and macOS 14+. Android requires arm64 and Android 10+.
iOS 17+ is available from source. Receiver delivery and cloud backup remain separate states.
