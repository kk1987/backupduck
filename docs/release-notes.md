# Beta 21

- Mac and iOS historical imports now keep a persistent list of the photos present when a scan starts. Restarting the app resumes that same list instead of pulling newly added photos into the old scan.
- Turning off automatic new-photo backup preserves work already discovered while stopping later additions. Switching it back on starts a fresh discovery boundary.
- Mac folder backups started with “Back Up Existing Files” exclude files created after that run began. Adding a folder without existing files no longer skips new files that arrive during the initial scan. Automatic folder backup continues to monitor new or changed files separately.

Mac requires Apple Silicon and macOS 14+. Android requires arm64 and Android 10+.
iOS 17+ is available from source. Receiver delivery and cloud backup remain separate states.
