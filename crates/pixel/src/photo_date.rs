//! Date metadata belongs to the delivery copy, never to the received original.
use backupduck_core::{Error, Result};
use little_exif::{exif_tag::ExifTag, filetype::FileExtension, metadata::Metadata};
use std::{fs, path::Path};

const MAX_SOURCE_BYTES: u64 = 64 << 20;

/// little_exif 0.6.23 distinguishes absent metadata from malformed metadata
/// with these exact errors. Never treat any other parse failure as absence.
fn metadata_absent(error: &std::io::Error) -> bool {
    matches!(
        error.to_string().as_str(),
        "No EXIF item found!" | "No EXIF data found!" | "No metadata found!"
    )
}

/// `get_tag` only yields tags with the requested tag id.
fn tag_value(metadata: &Metadata, tag: ExifTag) -> Option<String> {
    match metadata.get_tag(&tag).next() {
        Some(
            ExifTag::DateTimeOriginal(value)
            | ExifTag::OffsetTimeOriginal(value)
            | ExifTag::SubSecTimeOriginal(value),
        ) => Some(value.clone()),
        _ => None,
    }
}

/// Return the EXIF DateTimeOriginal already embedded in a HEIC/HEIF original,
/// or `None` when the container carries no EXIF metadata or no such tag.
///
/// The container is parsed from its bytes as HEIF. Received originals are
/// stored without a file extension, and little_exif's content sniffing does
/// not recognise every HEIF major brand (`mif1`, for example). Android 10's
/// own HEIF stack cannot parse some valid variants either, so the receiver
/// must not rely on it to decide whether a capture date exists.
///
/// A container that cannot be parsed is an error
/// (`Unsupported("publication_date_unreadable")`), never `None`: publishing
/// a copy whose metadata cannot be reasoned about could replace a real
/// capture date.
pub fn read_photo_date(source: &Path) -> Result<Option<String>> {
    if source.metadata()?.len() > MAX_SOURCE_BYTES {
        return Err(Error::Unsupported("photo date metadata".into()));
    }
    let unreadable = || Error::Unsupported("publication_date_unreadable".into());
    let mut bytes = fs::read(source)?;
    match Metadata::new_from_vec(&bytes, FileExtension::HEIF) {
        Ok(metadata) => Ok(tag_value(
            &metadata,
            ExifTag::DateTimeOriginal(String::new()),
        )),
        Err(error) if metadata_absent(&error) => {
            // little_exif 0.6.23 also reports "No EXIF item found!" when it
            // finds no parseable meta box at all, e.g. for non-HEIF bytes.
            // The date is only absent if the container accepts an EXIF item,
            // which is exactly what `write_photo_date` will need to do.
            Metadata::new()
                .write_to_vec(&mut bytes, FileExtension::HEIF)
                .map_err(|_| unreadable())?;
            Ok(None)
        }
        Err(_) => Err(unreadable()),
    }
}

/// Write a capture date into a new copy and verify it by re-reading the copy.
/// Returns the verified DateTimeOriginal. Callers may trust this verification
/// for HEIC/HEIF; the platform image stack is not needed to confirm it.
pub fn write_photo_date(
    source: &Path,
    output: &Path,
    date: &str,
    subsecond: u16,
) -> Result<String> {
    let valid_date = date.len() == 19
        && date.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 | 13 | 16 => b == b':',
            10 => b == b' ',
            _ => b.is_ascii_digit(),
        });
    if subsecond > 999 || !valid_date || source == output || output.exists() {
        return Err(Error::Invalid("photo date destination".into()));
    }
    if !matches!(
        output.extension().and_then(|s| s.to_str()),
        Some("heic" | "heif" | "jpg" | "png")
    ) || source.metadata()?.len() > MAX_SOURCE_BYTES
    {
        return Err(Error::Unsupported("photo date metadata".into()));
    }
    // The caller only requests this when the original capture date is absent.
    // Reading and writing containers does not decode/re-encode image pixels.
    fs::copy(source, output)?;
    let subsecond = format!("{subsecond:03}");
    let result = (|| {
        let mut metadata = match Metadata::new_from_path(output) {
            Ok(value) => value,
            Err(error) if metadata_absent(&error) => Metadata::new(),
            Err(error) => return Err(error.into()),
        };
        metadata.set_tag(ExifTag::DateTimeOriginal(date.into()));
        metadata.set_tag(ExifTag::SubSecTimeOriginal(subsecond.clone()));
        metadata.set_tag(ExifTag::SubSecTimeDigitized(subsecond.clone()));
        metadata.set_tag(ExifTag::OffsetTimeOriginal("+00:00".into()));
        metadata.set_tag(ExifTag::CreateDate(date.into()));
        metadata.set_tag(ExifTag::OffsetTimeDigitized("+00:00".into()));
        metadata.write_to_file(output)?;
        // The same checks the Android receiver applies to JPEG/PNG copies:
        // the capture instant, its UTC offset and its fractional seconds.
        let reread = Metadata::new_from_path(output)?;
        let verified = tag_value(&reread, ExifTag::DateTimeOriginal(String::new()));
        if verified.as_deref() != Some(date)
            || tag_value(&reread, ExifTag::OffsetTimeOriginal(String::new())).as_deref()
                != Some("+00:00")
            || tag_value(&reread, ExifTag::SubSecTimeOriginal(String::new())).as_deref()
                != Some(subsecond.as_str())
        {
            return Err(Error::Integrity);
        }
        Ok(date.to_owned())
    })();
    if result.is_err() {
        let _ = fs::remove_file(output);
    }
    result
}
