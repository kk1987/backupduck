use backupduck_core::{BurstMetadata, Error};
use backupduck_pixel::{write_heic_burst, write_photo_date};
use little_exif::{exif_tag::ExifTag, metadata::Metadata};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[path = "common/mod.rs"]
mod common;
use common::{boxes, heic_with_xmp, items, primary_xmp, single_item_heic};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "backupduck-heic-burst-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn burst(primary: bool) -> BurstMetadata {
    BurstMetadata::from_identifier("heic-burst-fixture", primary).unwrap()
}

fn burst_fields(burst: &BurstMetadata) -> String {
    format!(
        "GCamera:BurstID=\"{}\" GCamera:BurstPrimary=\"{}\"",
        burst.group_id,
        u8::from(burst.primary)
    )
}

/// A burst copy is a plain HEIF: no Motion Photo video box or SEF footer.
fn assert_plain_heif(data: &[u8]) {
    let top: Vec<_> = boxes(data, 0, data.len())
        .into_iter()
        .map(|b| b.2)
        .collect();
    assert_eq!(top, [*b"ftyp", *b"meta", *b"mdat"]);
    assert!(!data.windows(4).any(|w| w == b"mpvd"));
    assert!(!data.ends_with(b"SEFT"));
    assert!(!data.windows(11).any(|w| w == b"MotionPhoto"));
}

fn write(source: &Path, output: &Path, burst: &BurstMetadata) -> Vec<u8> {
    write_heic_burst(source, output, burst).unwrap();
    fs::read(output).unwrap()
}

#[test]
fn input_without_xmp_gains_one_primary_xmp_item() {
    let root = scratch();
    let pixels = b"abcdefghijk";
    let original = single_item_heic(pixels);
    let source = root.join("source.heic");
    fs::write(&source, &original).unwrap();
    let frame = burst(true);
    let result = write(&source, &root.join("burst.heic"), &frame);
    assert_plain_heif(&result);
    let table = items(&result);
    assert_eq!(table.xmp.len(), 1);
    assert_eq!(table.primary_xmp(), table.xmp);
    let (xmp, refs) = primary_xmp(&result);
    assert_eq!(refs, 1);
    assert!(String::from_utf8(xmp)
        .unwrap()
        .contains(&burst_fields(&frame)));
    let (offset, length) = table.location(table.primary);
    assert_eq!(&result[offset..offset + length], pixels);
    assert_eq!(fs::read(&source).unwrap(), original);
    assert!(!root.join("burst.burst.partial").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn existing_primary_xmp_is_merged_into_the_same_item() {
    let root = scratch();
    let primary = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description xmlns:apple="urn:apple:fixture" apple:Color="HDR"/></r:RDF></x:xmpmeta>"#;
    let auxiliary = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description xmlns:depth="urn:depth:fixture" depth:Value="preserved"/></r:RDF></x:xmpmeta>"#;
    let (original, payload) = heic_with_xmp(primary, auxiliary);
    let source = root.join("source.heic");
    fs::write(&source, &original).unwrap();
    let frame = burst(false);
    let result = write(&source, &root.join("burst.heic"), &frame);
    assert_plain_heif(&result);
    let table = items(&result);
    assert_eq!(table.xmp, [3, 4], "no new XMP item");
    assert_eq!(table.primary_xmp(), [3], "same item, single reference");
    let xmp = String::from_utf8(primary_xmp(&result).0).unwrap();
    assert!(xmp.contains("apple:Color=\"HDR\""));
    assert!(xmp.contains(&burst_fields(&frame)));
    let (offset, length) = table.location(4);
    assert_eq!(&result[offset..offset + length], auxiliary);
    for (id, pixels) in [(1, b"abcde"), (2, b"fghij")] {
        let (offset, length) = table.location(id);
        assert_eq!(&result[offset..offset + length], pixels);
    }
    assert!(result.windows(payload.len()).any(|w| w == payload));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn matching_burst_values_are_idempotent_and_conflicts_are_unsupported() {
    let root = scratch();
    let source = root.join("source.heic");
    let (original, _) = heic_with_xmp(
        br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"></rdf:RDF></x:xmpmeta>"#,
        b"<x/>",
    );
    fs::write(&source, original).unwrap();
    let frame = burst(true);
    let first = root.join("first.heic");
    let once = write(&source, &first, &frame);
    let twice = write(&first, &root.join("second.heic"), &frame);
    assert_eq!(once, twice);
    let new_item = root.join("new-item.heic");
    fs::write(&new_item, single_item_heic(b"pixels")).unwrap();
    let annotated = write(&new_item, &root.join("new-item-1.heic"), &frame);
    assert_eq!(
        write(
            &root.join("new-item-1.heic"),
            &root.join("new-item-2.heic"),
            &frame
        ),
        annotated
    );

    let other_group = BurstMetadata::from_identifier("another-burst", true).unwrap();
    let other_role = burst(false);
    for (input, conflict) in [(&first, &other_group), (&first, &other_role)] {
        let output = root.join("conflict.heic");
        assert!(matches!(
            write_heic_burst(input, &output, conflict),
            Err(Error::Unsupported(_))
        ));
        assert!(!output.exists());
        assert!(!root.join("conflict.burst.partial").exists());
    }
    for xmp in [
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:GCamera="http://ns.google.com/photos/1.0/camera/" GCamera:MotionPhoto="1"/></rdf:RDF></x:xmpmeta>"#,
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:Container="http://ns.google.com/photos/1.0/container/" Container:Version="1"/></rdf:RDF></x:xmpmeta>"#,
        &format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:GCamera="http://ns.google.com/photos/1.0/camera/" GCamera:BurstID="{}"/></rdf:RDF></x:xmpmeta>"#,
            frame.group_id
        ),
        &format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:GCamera="http://ns.google.com/photos/1.0/camera/"><GCamera:BurstID>{}</GCamera:BurstID></rdf:Description></rdf:RDF></x:xmpmeta>"#,
            frame.group_id
        ),
    ] {
        let input = root.join("camera.heic");
        fs::write(&input, heic_with_xmp(xmp.as_bytes(), b"<x/>").0).unwrap();
        assert!(matches!(
            write_heic_burst(&input, &root.join("camera-out.heic"), &frame),
            Err(Error::Unsupported(_))
        ));
    }
    assert!(matches!(
        write_heic_burst(&source, &source, &frame),
        Err(Error::Invalid(_))
    ));
    let invalid = BurstMetadata {
        group_id: "not-a-digest".into(),
        primary: true,
    };
    assert!(matches!(
        write_heic_burst(&source, &root.join("invalid.heic"), &invalid),
        Err(Error::Invalid(_))
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dated_fixture_round_trips_through_the_burst_writer() {
    let root = scratch();
    let bytes = include_bytes!("fixtures/date.heic");
    let source = root.join("source.heic");
    fs::write(&source, bytes).unwrap();
    let frame = burst(true);
    let date = "2026:08:15 02:41:41";
    let dated = root.join("dated.heic");
    write_photo_date(&source, &dated, date, 0).unwrap();
    // The undated fixture has no iref box; little_exif writes a version 1 iref.
    for (input, name) in [
        (&source, "undated-burst.heic"),
        (&dated, "dated-burst.heic"),
    ] {
        let before = fs::read(input).unwrap();
        let output = root.join(name);
        let result = write(input, &output, &frame);
        assert_plain_heif(&result);
        let (xmp, refs) = primary_xmp(&result);
        assert_eq!(refs, 1);
        assert!(String::from_utf8(xmp)
            .unwrap()
            .contains(&burst_fields(&frame)));
        let (old_table, new_table) = (items(&before), items(&result));
        for (id, offset, length) in old_table.locations {
            let (moved, same_length) = new_table.location(id);
            assert_eq!(same_length, length);
            assert_eq!(
                result[moved..moved + length],
                before[offset..offset + length]
            );
        }
        if input == &dated {
            let metadata = Metadata::new_from_path(&output).unwrap();
            assert!(matches!(
                metadata.get_tag(&ExifTag::DateTimeOriginal(String::new())).next(),
                Some(ExifTag::DateTimeOriginal(value)) if value == date
            ));
        }
    }
    assert_eq!(fs::read(&source).unwrap(), bytes);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn real_heic_fixture_when_available() {
    // Run locally with an exported Live Photo still; never commit personal media.
    let Ok(dir) = std::env::var("BACKUPDUCK_LIVE_PHOTO_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(dir);
    let output = root.join("rust-burst.heic");
    let _ = fs::remove_file(&output);
    let frame = burst(true);
    let result = write(&root.join("still.heic"), &output, &frame);
    assert_plain_heif(&result);
    let (xmp, refs) = primary_xmp(&result);
    assert_eq!(refs, 1);
    assert!(String::from_utf8_lossy(&xmp).contains(&burst_fields(&frame)));
}
