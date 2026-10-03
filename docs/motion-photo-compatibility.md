# HEIC Motion Photo metadata

An Apple HEIC can contain several XMP items describing its primary image and
auxiliary images. Appending a new Motion Photo XMP item and a second `cdsc`
reference to the primary image is insufficient: Android's ItemTable reads the
first primary XMP reference, which still points to the camera's metadata.

## Why earlier validation passed

The privately retained earlier successful HEIC sample had auxiliary XMP but no
existing XMP describing the primary image. Adding a new primary XMP item worked
for that input. The later failing samples already had primary XMP; the old
packager appended another item instead of updating the authoritative one.
Presence of a MotionPhoto string somewhere in the file therefore gave a false
sense of correctness. The embedded original video was still present.

This is a difference between input metadata layouts, not an explanation based
on capture date. Successful earlier app validation must not be discarded merely
because a later photo fails. These observations do not establish that changing
the Google account, Pixel device, or sending platform caused the failure.

## Packaging fix

HEIC packaging now merges the Motion Photo description into the existing primary
XMP packet, reuses that item's identity and references, and updates its extent.
Other XMP items, original media payloads and auxiliary-image references remain
intact. Inputs without primary XMP still receive one new item. Ambiguous layouts
are rejected for the existing codec fallback.

Motion Photo copies keep the original Apple Photos filename with `_MP` added
before the image extension, for example `IMG_1234_MP.HEIC` or `IMG_1234_MP.jpg`
when the still is converted to JPEG, following the
[Motion Photo filename recommendation](https://developer.android.com/media/platform/motion-photo-format).
A collision numbers the name before the suffix (`IMG_1234 (1)_MP.HEIC`).
Interrupted publications resume through their stored MediaStore URI, and
confirmed copies retain it; this change does not rewrite existing gallery files
or cloud items.

## HEIC bursts

Burst frames use the same primary-XMP rewrite without the Motion Photo parts:
`GCamera:BurstID` and `GCamera:BurstPrimary` are merged into the existing
primary XMP item, or a new one is added, and no `mpvd` box or SEF footer is
appended. Image, gain-map and other item payloads are copied byte-for-byte. An
input that already carries the same burst values is returned unchanged; other
GCamera or Container metadata, a different burst value and unsupported layouts
are reported as `unsupported`, and the receiver then publishes the decoded JPEG
copy. The **Convert HEIC bursts to JPEG** setting skips the HEIC path. HEIC
date writing is limited to 64 MiB; the date writer reports a larger undated
frame as `unsupported` too, so it is published through the JPEG path. Google
Photos cloud grouping of HEIC bursts is unverified.

## Regression and playback evidence

Regression tests follow the primary item's metadata reference and extent,
rather than searching the file for a MotionPhoto string. They cover existing
primary and auxiliary XMP, alternate RDF prefixes, new-XMP inputs, and original
image/video preservation. Three privately held real Live Photo pairs were also
packaged and checked without committing their media or metadata.

Byte integrity, readable Motion Photo metadata and Google Photos playback are
separate checks. A receiver receipt alone does not prove playback or cloud
backup. Existing gallery/cloud items need separate, explicit repair; retrying a
received transfer does not replace a confirmed delivery copy.

Cloud validation of a privately held real HEIC/MOV pair confirmed playback in
Google Photos after this metadata fix. The tested output used the same Rust
packager called by the Android receiver, retaining the original HEIC image
payload and MOV bytes. Cloud video processing took several minutes; its animation
entry was initially unavailable. The downloaded video retained the original MOV
payload followed by the container's SEF footer.

A JPEG containing the same untouched MOV played locally on the tested Pixel but
was not recognized as a Motion Photo by the tested cloud upload. Local playback
and format conformance therefore do not establish cloud compatibility. This
validation supports the HEIC metadata fix; it does not justify switching the
receiver's default output to JPEG or transcoding every Live Photo.

## JPEG Live Photos with existing XMP

Another failure occurs before gallery publication. Two real shared JPG/MOV
Live Photos were received byte-for-byte, but the Pixel reported
`receipt=received, processing=failed`; neither appeared in Android MediaStore.
Both original JPGs have one ordinary Adobe XMP APP1 packet and an MPF APP2
directory pointing to a second JPEG image. They do not have the Ultra HDR
GContainer directory expected by the old direct packager. On the exact received
files that packager returned `Unsupported("preexisting XMP packet")`. The native
JSON bridge deliberately reduces this to the stable code `unsupported`, while
the Android fallback recognized only the Rust display prefix `unsupported
capability:`. It therefore skipped fallback and marked publication failed.

The earlier no-transcode work (`899b357`, `22ca971`, `800a9ce`) is real: the
Android app tries the Rust direct packaging path before any codec conversion,
and keeps a compatible MOV byte-for-byte. The previous successful tests covered
different JPEG or HEIC layouts. They did not demonstrate that an ordinary XMP
packet plus an MPF auxiliary image was supported. This failure depends on the
input layout, not on the iPhone model or Google account.

The direct JPEG packager now inserts Motion Photo metadata into the one existing
XMP packet, leaving unrelated metadata, JPEG scan data, the MPF auxiliary JPEG,
and MOV bytes unchanged. It validates the two-image MPF geometry and adjusts
its primary size and offsets after replacing the XMP packet. Tests cover an
ordinary XMP packet with and without MPF, as well as the earlier Ultra HDR path.
The two privately retained original JPG/MOV pairs were packaged by this exact
Rust path without decoding or re-encoding. Ambiguous or conflicting XMP still
returns `unsupported`; the Android caller now recognizes that stable code and
can use its codec fallback only for those remaining cases.

The two repaired files were temporarily indexed on the test Pixel. Google
Photos displayed the Motion Photo control for one sample, and successive frames
of the other visibly changed during local playback. Those diagnostic copies
were removed after testing. This verifies local recognition, not Google Photos
cloud processing.

Receiver history records a bounded processing error code on new failures, and
failed originals remain available for an explicit processing retry. The Mac
transfer receipt means the phone has the files; it does not mean Android
gallery publication or Google Photos cloud backup has finished. The official
beta.24 build 28 was installed through the app's updater on the test Pixel. Its
retry action processed the seven retained failed originals without retransmission:
each changed from `received/failed` to `received/complete` and gained one
MediaStore gallery copy. Two actual gallery copies were pulled and compared
with the sender originals. Their JPEG scan data, MPF auxiliary JPEG and paired
MOV were byte-identical, confirming that the production receiver took the
direct path without transcoding. Google Photos cloud recognition of these
retried copies has not yet been verified.

## JPEG cloud recognition after local publication

The next check found that local Pixel playback and successful MediaStore
publication still did not guarantee Google Photos cloud animation for the
shared-library JPG/MOV samples. The downloaded cloud object for one static
sample matched its Pixel gallery copy byte-for-byte, so this was not a lost
video during cloud transfer. The old direct JPEG writer described the appended
MOV while omitting the MPF auxiliary JPEG from its XMP container directory,
declared a primary padding gap that did not match the file, and appended a SEF
footer after the MOV. This conflicts with Android's [Motion Photo container
layout](https://developer.android.com/media/platform/motion-photo-format),
which requires a directory item for each concatenated resource and the video
as the final item.

Two authorized copies of one sample were backed up to Google Photos on the
test Pixel. Changing only the container padding left the cloud item static.
The copy with MP4/H.264 video and AAC audio showed an animation control in
Google Photos Web, and the animation opened. The latter retained the original
JPEG and copied its H.264 video samples; only the audio needed encoding. This
comparison did not isolate the cause: the successful copy changed the video
container and audio as well as the JPEG directory and trailing footer. The
padding-only copy still had the other format defects.

The writer now lists the preserved MPF auxiliary JPEG, makes the appended video
the exact end of the file, and removes the invented padding and SEF footer.
The interim beta.25 JPG+MOV publication path used Media3 Transformer to produce
MP4/H.264/AAC before packaging. Media3 can copy compatible compressed video
samples into the new container without re-encoding them; it converts audio
when necessary. The still image and its MPF auxiliary image are not decoded or
re-encoded on this direct path. Regression tests check the directory, MPF
offsets, source bytes, and exact video tail. Previously published copies are
not rewritten by an app update; the final Android output still requires a
separate device-and-cloud acceptance check.

### Seven-file investigation (2026-09-29)

The seven gallery copies from 2026-09-27 were pulled from the test Pixel. The
cloud objects for `d821`, `3658`, and `d894` had no animation control; `3765`
and `30a9` did. Web's `bd8c` object said it was preparing the Motion Photo,
which alone is unresolved rather than evidence of failure. The user later
confirmed that the old `bd8c` cloud copy could not load its animation in iOS.
All seven embedded
videos are QuickTime MOV with AVC/H.264 video and 48 kHz mono PCM audio. All
seven videos decode, and the still/video pairing IDs match. Their old JPEG
Motion Photo copies also share the same structural defects: the MPF gain-map
JPEG is absent from the XMP container directory, the primary item declares a
nonexistent 24-byte padding gap, and the MOV is preceded by an undeclared SEF
prefix and followed by a SEF footer. Their MPF entries themselves point to
the correct image boundaries. Thus MOV versus MP4, PCM versus AAC, and MPF
geometry alone do not distinguish the known cloud outcomes.

The [Android Motion Photo 1.0 format](https://developer.android.com/media/platform/motion-photo-format)
allows `video/quicktime`, but requires a tightly packed directory and the video
as the last file item. Its optional audio track is described as AAC. The
working cloud samples show that Google Photos sometimes tolerates the old
defects, not that the defects are safe. Why it tolerates particular samples is
still unproven.

For a controlled test, reconstructed copies of the three static samples were
made with the production JPEG writer. The JPEG compressed image, MPF auxiliary
JPEG and MOV bytes are preserved; only the Motion Photo container layout and
metadata are corrected. If these copies animate after cloud processing, the
receiver can keep the no-encoding path. If any remain static, the next separate
experiment changes only PCM audio to AAC while copying the H.264 video samples.
No receiver policy should be changed on the basis of the earlier MP4/AAC result
alone.

With the user's explicit approval, all three container-only copies were placed
in the test Pixel's camera folder. Their device SHA-256 hashes matched the local
copies. Google Photos Web confirmed original-quality cloud backup from Android
and showed the Motion Photo preparation control for each exact diagnostic
filename. The user then confirmed that all three played in Google Photos on
both the Pixel and iOS. The iOS cloud playback is the decisive check: the result
does not depend on the Pixel's local MOV cache. Web's preparation state is not
classified as failure.

The user subsequently authorized the same container-only test for `bd8c`.
Its video has an edit list that starts the video 0.1 seconds into its media
timeline, unlike the first three samples. That original edit list, full MOV,
JPEG scan and auxiliary JPEG were preserved. After Android uploaded the new
copy, the user confirmed that it also played in Google Photos on iOS. The
edit list therefore did not require normalization for this sample.

This controlled comparison establishes that correcting the Motion Photo
container is sufficient for all four reported failures without encoding the image,
video or audio. It does not isolate which individual directory/padding/footer
violation Google's closed-source parser rejected, or explain why it tolerated
the older layout for some other photos. All of those layout violations are
fixed. PCM audio conversion and MOV-to-MP4 conversion are not necessary for
these verified samples; they must not be inferred to be universally required
from the earlier multi-variable trial.

Beta.26 therefore restores the direct JPG+MOV path while keeping beta.25's
container fixes. The full original MOV, including its audio, is passed to the
native writer. Existing conversion remains available only when direct
packaging reports an unsupported container. A transfer error, a gallery error
or Web's preparation label does not trigger blanket encoding. This does not
promise that every possible MOV codec/layout will be accepted by Google Photos.
Updating the app does not replace old static cloud copies; the three verified
diagnostic copies plus the verified `bd8c` copy remain beside the originals.

Beta.27 makes that codec fallback an explicit receiver choice, default off. The
switch is checked separately for each received Live Photo after the direct HEIC
and JPEG paths. When the direct writer cannot package an input and conversion is
off, Android records `conversion_required` as a gallery-processing failure;
the verified originals and transfer receipt remain. The Transfers screen offers
the receiver settings and a processing retry. Turning conversion on permits
the existing JPEG/MP4 fallback for unsupported inputs; it does not convert
otherwise-supported HEIC+MOV or JPG+MOV samples. No setting can detect Google
Photos cloud animation or rewrite an already-published gallery/cloud copy.

The sender now follows Android's processing state after its transfer receipt.
It reports a gallery publication failure independently of transfer success and
keeps polling the receiver after a processing retry without retransmitting the
originals. "Added to phone gallery" is still distinct from a Google Photos
cloud backup or playable animation; there is no Google cloud success signal in
the BackupDuck protocol.

## Future investigation checklist

1. Verify the original still and paired video against sender/receiver checksums.
2. Inspect `pitm`, XMP MIME items, `cdsc` references and `iloc` extents. Read the
   first primary-image XMP as an independent reader would; a whole-file text
   search is insufficient. Cover inputs both with and without existing primary
   XMP, and preserve auxiliary metadata.
3. Confirm the formal Android path calls the tested packager. Check still-image
   orientation, motion playback and original media payload integrity separately.
4. Test device-folder playback and cloud playback independently. Treat a
   preparing-animation message as pending until playback is observed; do not
   infer success from an upload receipt or a download-video menu alone.
5. Record the tested app version, input layout, packaging path and observed
   results privately. Keep personal media, account IDs and device identifiers
   out of public tests and documentation.
6. Repair already received gallery/cloud copies explicitly. An application
   update or transfer retry does not rewrite an existing confirmed delivery.
