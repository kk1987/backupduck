# Preserving capture dates during gallery publication

PhotoKit's `creationDate` is separate from the date embedded in an exported
original. A file can have no EXIF capture date even when its Apple Photos asset
has a valid date. On Android 10, a media rescan can replace an insert-time
`MediaStore.DATE_TAKEN` with an unknown date. A newly created file then exposes
its import-time modification date as a fallback.

The sender's `metadata.created_at_ms` carries the capture instant. The Android
receiver enriches a delivery copy when an image lacks an EXIF capture date,
without modifying the verified original or re-encoding its pixels:

- `MediaDates.kt` handles JPEG/PNG metadata and coordinates HEIC/HEIF processing.
- `crates/pixel/src/photo_date.rs` adds date metadata to HEIC/HEIF copies through
  the `write_photo_date` native command, and reads an existing capture date
  through `read_photo_date`.
- DateTimeOriginal, digitized time, UTC offsets and fractional seconds describe
  the same instant. Existing valid EXIF capture dates remain unchanged.
- The receiver stamps file modification time after writing the last byte and
  before clearing `IS_PENDING`. MediaStore milliseconds and seconds are handled
  separately.
- Motion and burst dates (JPEG and HEIC) are set before packaging, so later
  EXIF rewrites cannot invalidate the motion video's XMP offsets.
- Publication verification uses the delivered copy's size and hash. Original
  transfer integrity and gallery-copy verification remain separate.

## HEIF that Android cannot parse

Android 10's HEIF stack cannot parse some valid HEIF files. For one received
photo (ftyp `mif1` with compatible `mif1, MiHA, miaf, heix`, no EXIF), the
Pixel's media scanner reported no width, height or date, and `ExifInterface`
could not read back the date written into the delivery copy. Publication
failed with `publication_date_failed` although the dated copy was correct.

For HEIC/HEIF the Rust container code therefore decides and verifies:

- If `ExifInterface` finds a capture date, the original is used unchanged.
  Otherwise `read_photo_date` parses the container itself; a date it finds is
  kept, so a date Android could not see is never overwritten.
- `write_photo_date` re-reads its own output and checks DateTimeOriginal,
  OffsetTimeOriginal and SubSecTimeOriginal. Android does not re-read HEIF
  copies; JPEG/PNG copies are still verified with `ExifInterface`.
- A container the Rust code cannot parse fails with
  `publication_date_unreadable` and is not published. little_exif reports
  non-HEIF bytes as "no EXIF item", so the reader only reports a missing date
  when the container also accepts a new EXIF item.

## Validation and limits

Rust tests cover round trips, absent EXIF, malformed input, subsecond consistency
and preservation of originals. Android instrumentation mode `media_dates`
compares publication before and after a full rescan, hashes and decoded pixels.
Its fixtures are synthetic. The regular integration test checks dates on motion
and burst publications alongside receipt and cache-retention behavior.

Pixel XL / Android 10 testing confirmed JPEG and HEIC capture dates survive a
rescan. Android 10's scanner did not use the PNG EXIF capture date; the PNG's
embedded metadata and modification-time fallback were nevertheless corrected.
This does not establish Google Photos cloud behavior for every media format.
Video embedded dates are preserved, not rewritten; video metadata conflicts need
separate investigation. Existing valid EXIF dates are not overridden by a later
Apple Photos date edit.

This fix affects new publications. It does not update dates on existing Google
Photos cloud items, republish verified gallery copies, or delete cloud media.
Historical repair must match each cloud item to its correct source date and
skip already-correct items. Neither a date range nor a filename alone is enough
to resolve ambiguous matches safely.
