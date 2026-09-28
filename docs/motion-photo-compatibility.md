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
The original JPGs each contain an Adobe XMP APP1 packet. Passing those exact
files to the JPEG Motion Photo packager returns
`Unsupported("preexisting XMP packet")`: the direct path accepts only a clean
JPEG or its supported Ultra HDR layout. The native JSON bridge deliberately
reduces this to the stable code `unsupported`, while the Android fallback used
to recognize only the Rust display prefix `unsupported capability:`. It
therefore skipped the image-decoding fallback and marked publication failed.

The Android caller now recognizes the bridge's stable code and decodes these
JPGs to a clean still before packaging with the original paired video. This
fix applies to the input layout, not to a specific iPhone model. The observed
successful HEIC Live Photos use a different packaging path. Receiver history
records a bounded processing error code on new failures, and failed originals
remain available for an explicit processing retry. The Mac transfer receipt
means the phone has the files; it does not mean Android gallery publication or
Google Photos cloud backup has finished.

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
