use backupduck_pixel::{write_heic_motion_with_burst, write_heic_motion_with_burst_and_video_mime};
use std::{fs, path::PathBuf};

#[path = "common/mod.rs"]
mod common;
use common::{atom, heic_with_xmp, primary_xmp, single_item_heic};

#[test]
fn heic_and_mov_payloads_are_preserved() {
    let root = std::env::temp_dir().join(format!("backupduck-motion-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let still = root.join("still.heic");
    let video = root.join("paired.mov");
    let output = root.join("result.heic");
    let pixels = b"abcde";
    let image = single_item_heic(pixels);
    fs::write(&still, &image).unwrap();
    let movie = atom(b"ftyp", b"qt  \0\0\0\0qt  ");
    fs::write(&video, &movie).unwrap();
    write_heic_motion_with_burst(&still, &video, &output, None).unwrap();
    let result = fs::read(&output).unwrap();
    assert_eq!(fs::read(&still).unwrap(), image);
    assert_eq!(fs::read(&video).unwrap(), movie);
    assert!(result.windows(pixels.len()).any(|w| w == pixels));
    assert!(result.windows(movie.len()).any(|w| w == movie));
    assert!(result
        .windows(b"MotionPhoto".len())
        .any(|w| w == b"MotionPhoto"));
    assert!(result.ends_with(b"SEFT"));
    let mp4_output = root.join("result-mp4.heic");
    write_heic_motion_with_burst_and_video_mime(&still, &video, &mp4_output, None, "video/mp4")
        .unwrap();
    assert!(String::from_utf8_lossy(&fs::read(&mp4_output).unwrap())
        .contains("Item:Mime=\"video/mp4\""));
    assert!(write_heic_motion_with_burst(&still, &video, &output, None).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn real_heic_fixture_when_available() {
    // Run locally with an exported Live Photo pair; never commit personal media.
    let Ok(dir) = std::env::var("BACKUPDUCK_LIVE_PHOTO_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(dir);
    let output = root.join("rust-no-transcode.heic");
    let _ = fs::remove_file(&output);
    write_heic_motion_with_burst(
        &root.join("still.heic"),
        &root.join("paired.mov"),
        &output,
        None,
    )
    .unwrap();
    assert!(output.metadata().unwrap().len() > root.join("still.heic").metadata().unwrap().len());
}

#[test]
fn existing_primary_and_auxiliary_xmp_remain_readable() {
    let root = std::env::temp_dir().join(format!("backupduck-motion-xmp-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let primary = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description xmlns:apple="urn:apple:fixture" apple:Color="HDR"/></r:RDF></x:xmpmeta>"#;
    let auxiliary = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description xmlns:depth="urn:depth:fixture" depth:Value="preserved"/></r:RDF></x:xmpmeta>"#;
    let (original, payload) = heic_with_xmp(primary, auxiliary);
    let video = atom(b"ftyp", b"qt  \0\0\0\0qt  ");
    let still = root.join("still.heic");
    let mov = root.join("paired.mov");
    let out = root.join("motion.heic");
    fs::write(&still, &original).unwrap();
    fs::write(&mov, &video).unwrap();
    write_heic_motion_with_burst(&still, &mov, &out, None).unwrap();
    let result = fs::read(&out).unwrap();
    let (xmp, refs) = primary_xmp(&result);
    assert_eq!(refs, 1, "do not add a second primary XMP reference");
    let xmp = String::from_utf8(xmp).unwrap();
    assert!(xmp.contains("GCamera:MotionPhoto=\"1\""));
    assert!(xmp.contains("apple:Color=\"HDR\""));
    assert!(result.windows(auxiliary.len()).any(|b| b == auxiliary));
    assert!(result.windows(payload.len()).any(|b| b == payload));
    assert_eq!(fs::read(&still).unwrap(), original);
    assert_eq!(fs::read(&mov).unwrap(), video);
    fs::remove_dir_all(root).unwrap();
}
