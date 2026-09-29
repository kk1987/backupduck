# Beta 26

- Restore the original MOV as the default for new JPG+MOV Live Photos. Seven real photos had the same H.264/PCM codecs but different Google Photos cloud results, so converting every one was not justified.
- Add an optional receiver setting to improve cloud compatibility for *future* JPG+MOV photos by producing MP4/H.264/AAC. This can convert audio or video. Direct packaging still falls back when the source cannot be processed locally.
- Keep the corrected JPEG Motion Photo container metadata, including MPF auxiliary images and an exact video tail.

Mac requires Apple Silicon and macOS 14+. Android requires arm64 and Android 10+.
iOS 17+ is available from source. Existing gallery/cloud copies are not changed by updating the app. The receiver cannot observe whether Google Photos cloud plays a gallery copy, so cloud-specific fallback requires a user choice for future assets; it cannot happen automatically.
