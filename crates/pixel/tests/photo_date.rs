mod common;
use backupduck_core::Error;
use backupduck_pixel::{read_photo_date, write_photo_date};
use common::{items, single_item_heic, single_item_heif};
use little_exif::{exif_tag::ExifTag, metadata::Metadata};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static SEQ: AtomicU64 = AtomicU64::new(0);
fn scratch() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "backupduck-date-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}
#[test]
fn heic_capture_date_roundtrips_without_touching_original_or_overwriting_output() {
    let root = scratch();
    let source = root.join("source.heic");
    let output = root.join("dated.heic");
    let bytes = include_bytes!("fixtures/date.heic");
    fs::write(&source, bytes).unwrap();
    let date = "2026:08:15 02:41:41";
    assert!(write_photo_date(&source, &source, date, 0).is_err());
    assert!(write_photo_date(&source, &output, "invalid", 0).is_err());
    assert!(!output.exists());
    assert_eq!(write_photo_date(&source, &output, date, 0).unwrap(), date);
    let metadata = Metadata::new_from_path(&output).unwrap();
    assert!(
        matches!(metadata.get_tag(&ExifTag::DateTimeOriginal(String::new())).next(), Some(ExifTag::DateTimeOriginal(v)) if v == date)
    );
    assert!(
        matches!(metadata.get_tag(&ExifTag::OffsetTimeOriginal(String::new())).next(), Some(ExifTag::OffsetTimeOriginal(v)) if v == "+00:00")
    );
    let before = fs::read(&output).unwrap();
    assert!(write_photo_date(&source, &output, date, 0).is_err());
    assert_eq!(fs::read(&output).unwrap(), before);
    assert_eq!(fs::read(&source).unwrap(), bytes);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reader_reports_absent_and_written_dates_regardless_of_file_name() {
    let root = scratch();
    let bytes = include_bytes!("fixtures/date.heic");
    // Received originals are stored by digest, without an extension.
    let blob = root.join("0123abcd");
    fs::write(&blob, bytes).unwrap();
    assert_eq!(read_photo_date(&blob).unwrap(), None);
    let dated = root.join("dated.heic");
    let date = "2026:08:15 02:41:41";
    write_photo_date(&blob, &dated, date, 250).unwrap();
    assert_eq!(read_photo_date(&dated).unwrap().as_deref(), Some(date));
    let dated_blob = root.join("4567ef01");
    fs::copy(&dated, &dated_blob).unwrap();
    assert_eq!(read_photo_date(&dated_blob).unwrap().as_deref(), Some(date));
    assert_eq!(fs::read(&blob).unwrap(), bytes);
    fs::remove_dir_all(root).unwrap();
}

/// Shape of the failing receiver item: ftyp major brand `mif1`, compatible
/// `mif1, MiHA, miaf, heix`, no EXIF item. little_exif's content sniffing does
/// not recognise a `mif1` major brand, so this only works because the reader
/// parses the bytes as HEIF explicitly.
#[test]
fn mif1_heix_without_exif_is_read_dated_and_verified() {
    let root = scratch();
    let original = single_item_heif(b"mif1\0\0\0\0mif1MiHAmiafheix", b"pixels");
    let blob = root.join("89abcdef");
    fs::write(&blob, &original).unwrap();
    assert_eq!(read_photo_date(&blob).unwrap(), None);
    let dated = root.join("dated.heic");
    let date = "2026:06:14 17:35:13";
    assert_eq!(write_photo_date(&blob, &dated, date, 7).unwrap(), date);
    assert_eq!(read_photo_date(&dated).unwrap().as_deref(), Some(date));
    let metadata = Metadata::new_from_path(&dated).unwrap();
    assert!(
        matches!(metadata.get_tag(&ExifTag::SubSecTimeOriginal(String::new())).next(), Some(ExifTag::SubSecTimeOriginal(v)) if v == "007")
    );
    let written = fs::read(&dated).unwrap();
    assert_eq!(&written[4..12], b"ftypmif1");
    // The primary image extent still points at the untouched coded bytes.
    let (offset, length) = items(&written).location(1);
    assert_eq!(&written[offset..offset + length], b"pixels");
    assert_eq!(fs::read(&blob).unwrap(), original);
    fs::remove_dir_all(root).unwrap();
}

/// little_exif reports "No EXIF item found!" for any input without a parseable
/// meta box, including non-HEIF bytes. That must not read as "no date".
#[test]
fn unparseable_container_is_an_error_not_an_absent_date() {
    let root = scratch();
    let blob = root.join("fedcba98");
    for bytes in [
        b"\xff\xd8\xff\xd9".to_vec(),
        b"\0\0\0\x18ftypmif1\0\0\0\0mif1heix\0\0\x01\0meta".to_vec(),
        // A meta box without the mandatory hdlr box.
        single_item_heic(b"pixels"),
    ] {
        fs::write(&blob, &bytes).unwrap();
        assert!(
            matches!(read_photo_date(&blob), Err(Error::Unsupported(reason)) if reason == "publication_date_unreadable"),
            "{bytes:?}"
        );
        // The writer fails on the same inputs, so reader and writer agree.
        assert!(
            write_photo_date(&blob, &root.join("dated.heic"), "2026:06:14 17:35:13", 0).is_err()
        );
        assert!(!root.join("dated.heic").exists());
    }
    fs::remove_dir_all(root).unwrap();
}
