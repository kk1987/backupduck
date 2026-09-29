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

New delivery names end in `_MP` before the image extension, following the
[Motion Photo filename recommendation](https://developer.android.com/media/platform/motion-photo-format).
Previous names remain candidates when resuming interrupted publication.
Confirmed copies retain their stored URI; this change does not rewrite existing
gallery files or cloud items.

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
Android's JPG+MOV publication path now uses Media3 Transformer to produce
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
and `30a9` did. The `bd8c` object said it was preparing the Motion Photo, which
is an unresolved state rather than evidence of failure. All seven embedded
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

The sender now follows Android's processing state after its transfer receipt.
It reports a gallery publication failure independently of transfer success and
keeps polling the receiver after a processing retry without retransmitting the
originals. "Added to phone gallery" is still distinct from a Google Photos
cloud backup or playable animation; there is no Google cloud success signal in
the PhotoBridge protocol.

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
