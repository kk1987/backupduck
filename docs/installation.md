# Install BackupDuck

## macOS sender

Download the Apple Silicon ZIP from the official website or GitHub Releases.
Extract it and move BackupDuck to Applications before opening it.
The free release candidate uses an ad hoc integrity signature and signed Sparkle updates, but
is not notarized by Apple. If macOS blocks opening it, review the source and release,
then use System Settings → Privacy & Security → Open Anyway.
Do not disable Gatekeeper globally. Grant Photos and local network access.

Use BackupDuck → Check for Updates, or enable automatic checks in Settings.
Updates pause and persist transfer work before replacing the application.

## Android receiver

Download the arm64 APK on Android 10 or later. Allow your browser to install it
when Android asks. Start reception and pair your sender. If you use Google Photos:
with Google Photos 7.94 on Android 10, items in `DCIM/BackupDuck` were backed up
together with Camera without a separate folder setting. If BackupDuck items are
not being backed up, enable backup for the BackupDuck device folder in Google
Photos.
Settings → App updates checks new releases. The app verifies the downloaded file,
package ID, increasing build number and signing certificate before opening the
system installer. Installation requires your confirmation. Automatic checks run
at most once a day while opening the receiver, and can be disabled.

Development APKs use a different signing key from public releases. Android cannot
update across these identities. Keep your existing development installation until
you have safely exported or preserved its originals and are ready to switch; the
public installer does not erase or migrate that data automatically.

Android RC.2 uses the new BackupDuck signing certificate. If you installed
BackupDuck RC.1, preserve any originals and receiver records before removing
that installation and installing RC.2. The built-in updater rejects a different
certificate; it cannot perform this one-time signing transition. Subsequent
official releases continue using the RC.2 certificate.

## iOS sender

Install the iOS sender through [TestFlight](https://testflight.apple.com/join/wGBxuQKT).
The current approved build remains available while newer builds are processing or under beta review.
App Store distribution is not available yet.

You can also build from source using Xcode, XcodeGen, Rust and the `aarch64-apple-ios` target:

```sh
xcodegen generate --spec apps/ios/project.yml
open apps/ios/BackupDuck.xcodeproj
```

Select your own signing team in Xcode and run on your iPhone or iPad (iOS 17+).
Do not commit team IDs, provisioning profiles or signing credentials. Personal
Team installations require periodic renewal. There is no in-app updater for iOS.

Scan the receiver code on iPhone. For Mac, scan the computer's code using the
Android receiver. Photos/local network permissions are needed. New-photo backup
and existing-photo import are separate choices. iOS schedules background work;
continuous execution while locked is not guaranteed. Avoid force-quitting the app.

## Hidden photos in a whole-library migration

The Apple senders include the Hidden album by default. If Photos protects the
Hidden album with Touch ID, Face ID or a password, the system withholds those
photos from every third-party app, so BackupDuck cannot back them up, and apps
cannot read whether that protection is on. Settings shows how many hidden photos
BackupDuck can currently see. If it shows 0 although you have hidden photos, turn
the protection off for the duration of the migration (Mac: Photos → Settings →
General; iOS: Settings → Photos). The count can take a while to update after the
change; relaunching BackupDuck shows it right away.
