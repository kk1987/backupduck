# Beta 23

- The Android receiver can pause incoming transfers when the Pixel gets hot. Temperature protection is enabled by default at 40°C; Settings lets you choose 35–45°C or turn it off. Receiving resumes after the phone cools, preserving confirmed progress.
- The receiver also responds to Android's severe thermal warning. Its status and notification show when temperature has paused receiving.
- App update checks now use the official website's cached release listing first, falling back to GitHub. This avoids a failed check when GitHub's anonymous API is rate-limited. Update-check and download errors now have separate, accurate messages, and the Android help text describes the available update channel correctly.

Mac requires Apple Silicon and macOS 14+. Android requires arm64 and Android 10+.
iOS 17+ is available from source. Receiver delivery and cloud backup remain separate states.
