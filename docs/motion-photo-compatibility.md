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
were removed after testing. This verifies local recognition, not the app's
retry flow or Google Photos cloud processing.

Receiver history records a bounded processing error code on new failures, and
failed originals remain available for an explicit processing retry. The Mac
transfer receipt means the phone has the files; it does not mean Android
gallery publication or Google Photos cloud backup has finished. On-device
receiver retry and cloud recognition of these repaired copies still require
separate validation after installing the updated receiver.

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
