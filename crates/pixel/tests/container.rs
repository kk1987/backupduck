use backupduck_pixel::{write_jpeg_motion, write_jpeg_motion_with_burst_and_video_mime};
use std::sync::atomic::{AtomicU64, Ordering};
static SEQ: AtomicU64 = AtomicU64::new(0);
#[test]
fn jpeg_container_has_exact_video_tail_and_no_invented_presentation_time() {
    let root = std::env::temp_dir().join(format!(
        "backupduck-motion-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).unwrap();
    // Structural fixture only: actual codec/playback validation is native-device work.
    let jpeg = [
        0xff, 0xd8, 0xff, 0xe0, 0, 4, 1, 2, 0xff, 0xda, 0, 2, 0xff, 0xd9,
    ];
    let video = b"\0\0\0\x14ftypisom\0\0\0\0isom";
    let image = root.join("still.jpg");
    let movie = root.join("video.mp4");
    let out = root.join("output.jpg");
    std::fs::write(&image, jpeg).unwrap();
    std::fs::write(&movie, video).unwrap();
    write_jpeg_motion(&image, &movie, &out, None).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.ends_with(video));
    assert_eq!(&bytes[..4], &[0xff, 0xd8, 0xff, 0xe1]);
    let packet = String::from_utf8_lossy(&bytes);
    assert!(packet.contains("GCamera:MotionPhoto=\"1\""));
    assert!(packet.contains("Item:Length=\"20\""));
    assert!(!packet.contains("PresentationTimestampUs"));
    assert_eq!(std::fs::read(&image).unwrap(), jpeg);
    assert!(write_jpeg_motion(&out, &movie, &root.join("bad.jpg"), None).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn jpeg_live_photo_keeps_original_mov() {
    let root = workdir();
    let image = root.join("still.jpg");
    let movie = root.join("paired.mov");
    let output = root.join("output.jpg");
    let jpeg = [
        0xff, 0xd8, 0xff, 0xe0, 0, 4, 1, 2, 0xff, 0xda, 0, 2, 0xff, 0xd9,
    ];
    let mov = b"\0\0\0\x14ftypqt  \0\0\0\0qt  ";
    std::fs::write(&image, jpeg).unwrap();
    std::fs::write(&movie, mov).unwrap();
    write_jpeg_motion_with_burst_and_video_mime(
        &image,
        &movie,
        &output,
        None,
        None,
        "video/quicktime",
    )
    .unwrap();
    let result = std::fs::read(output).unwrap();
    assert!(result.ends_with(mov));
    assert!(!result.windows(4).any(|window| window == b"SEFH"));
    assert!(String::from_utf8_lossy(&result).contains("Item:Mime=\"video/quicktime\""));
    assert_eq!(std::fs::read(image).unwrap(), jpeg);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_quicktime_without_ftyp_keeps_exact_video_tail() {
    let root = workdir();
    let image = root.join("still.jpg");
    let movie = root.join("paired.mov");
    let output = root.join("output.jpg");
    let jpeg = [
        0xff, 0xd8, 0xff, 0xe0, 0, 4, 1, 2, 0xff, 0xda, 0, 2, 0xff, 0xd9,
    ];
    let mov = b"\0\0\0\x08wide\0\0\0\x0cmdatDATA\0\0\0\x0cmoovMETA";
    std::fs::write(&image, jpeg).unwrap();
    std::fs::write(&movie, mov).unwrap();
    write_jpeg_motion_with_burst_and_video_mime(
        &image,
        &movie,
        &output,
        None,
        None,
        "video/quicktime",
    )
    .unwrap();
    let result = std::fs::read(&output).unwrap();
    assert!(result.ends_with(mov));
    assert_eq!(std::fs::read(&image).unwrap(), jpeg);
    assert_eq!(std::fs::read(&movie).unwrap(), mov);
    assert!(String::from_utf8_lossy(&result).contains("Item:Mime=\"video/quicktime\""));
    assert!(write_jpeg_motion_with_burst_and_video_mime(
        &image,
        &movie,
        &root.join("wrong-mime.jpg"),
        None,
        None,
        "video/mp4",
    )
    .is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn truncated_legacy_quicktime_is_rejected() {
    let root = workdir();
    let image = root.join("still.jpg");
    let movie = root.join("paired.mov");
    let output = root.join("output.jpg");
    let jpeg = [
        0xff, 0xd8, 0xff, 0xe0, 0, 4, 1, 2, 0xff, 0xda, 0, 2, 0xff, 0xd9,
    ];
    let mov = b"\0\0\0\x08wide\0\0\0\x0cmdatDATA\0\0\0\x20moovMETA";
    std::fs::write(&image, jpeg).unwrap();
    std::fs::write(&movie, mov).unwrap();
    assert!(write_jpeg_motion_with_burst_and_video_mime(
        &image,
        &movie,
        &output,
        None,
        None,
        "video/quicktime",
    )
    .is_err());
    assert!(!output.exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn real_jpeg_live_photo_when_available() {
    let Ok(dir) = std::env::var("BACKUPDUCK_LIVE_PHOTO_FIXTURE") else {
        return;
    };
    let root = std::path::PathBuf::from(dir);
    let still = root.join("still.jpg");
    if !still.exists() {
        return;
    }
    let video = root.join("paired.mov");
    let output = root.join("rust-jpeg-mov.jpg");
    let _ = std::fs::remove_file(&output);
    write_jpeg_motion_with_burst_and_video_mime(
        &still,
        &video,
        &output,
        None,
        None,
        "video/quicktime",
    )
    .unwrap();
    let result = std::fs::read(&output).unwrap();
    let original_still = std::fs::read(&still).unwrap();
    let original_video = std::fs::read(&video).unwrap();
    let (old_header, old_entries) = read_mpf(&original_still);
    let (new_header, new_entries) = read_mpf(&result);
    let old_auxiliary = old_header + old_entries[1].1 as usize;
    let new_auxiliary = new_header + new_entries[1].1 as usize;
    assert_eq!(old_entries[1].0, new_entries[1].0);
    assert_eq!(new_entries[0].0 as usize, new_auxiliary);
    assert_eq!(
        &result[new_auxiliary..new_auxiliary + old_entries[1].0 as usize],
        &original_still[old_auxiliary..old_auxiliary + old_entries[1].0 as usize]
    );
    let old_scan = scan_offset(&original_still);
    let new_scan = scan_offset(&result);
    assert_eq!(
        &result[new_scan..new_scan + original_still.len() - old_scan],
        &original_still[old_scan..]
    );
    assert!(result.ends_with(&original_video));
    assert!(!result.windows(4).any(|window| window == b"SEFH"));
    assert!(String::from_utf8_lossy(&result).contains("Item:Mime=\"video/quicktime\""));
    assert!(String::from_utf8_lossy(&result).contains(&format!(
        "Item:Semantic=\"GainMap\" Item:Length=\"{}\"",
        old_entries[1].0
    )));
}

const XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const GAIN_MAP: [u8; 14] = [
    0xff, 0xd8, 0xff, 0xe0, 0, 4, 9, 9, 0xff, 0xda, 0, 2, 0xff, 0xd9,
];

fn workdir() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "backupduck-motion-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0xff, marker];
    out.extend(((payload.len() + 2) as u16).to_be_bytes());
    out.extend(payload);
    out
}

/// The XMP packet Android's Ultra HDR encoder writes for `Bitmap.compress(JPEG)`.
fn gain_map_xmp(length: usize) -> Vec<u8> {
    let mut packet = XMP.to_vec();
    packet.extend(
        format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core 5.1.2">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description
        xmlns:Container="http://ns.google.com/photos/1.0/container/"
        xmlns:Item="http://ns.google.com/photos/1.0/container/item/"
        xmlns:hdrgm="http://ns.adobe.com/hdr-gain-map/1.0/"
        hdrgm:Version="1.0">
      <Container:Directory>
        <rdf:Seq>
          <rdf:li rdf:parseType="Resource">
            <Container:Item Item:Semantic="Primary" Item:Mime="image/jpeg"/>
          </rdf:li>
          <rdf:li rdf:parseType="Resource">
            <Container:Item Item:Semantic="GainMap" Item:Mime="image/jpeg" Item:Length="{length}"/>
          </rdf:li>
        </rdf:Seq>
      </Container:Directory>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>"#
        )
        .as_bytes(),
    );
    segment(0xe1, &packet)
}

/// Big-endian MPF APP2 with a primary entry and a gain map entry.
fn mpf(primary_size: u32, gain_offset: u32, gain_size: u32) -> Vec<u8> {
    let mut p = b"MPF\0MM\0*".to_vec();
    p.extend(8u32.to_be_bytes()); // IFD
    p.extend(1u16.to_be_bytes());
    p.extend(0xb002u16.to_be_bytes());
    p.extend(7u16.to_be_bytes());
    p.extend(32u32.to_be_bytes());
    p.extend(26u32.to_be_bytes()); // entries after IFD (8 + 2 + 12 + 4)
    p.extend(0u32.to_be_bytes()); // next IFD
    for (attr, size, offset) in [
        (0x0003_0000u32, primary_size, 0u32),
        (0, gain_size, gain_offset),
    ] {
        p.extend(attr.to_be_bytes());
        p.extend(size.to_be_bytes());
        p.extend(offset.to_be_bytes());
        p.extend([0u8; 4]);
    }
    segment(0xe2, &p)
}

/// An Ultra HDR JPEG as Android produces it. With `exif`, an EXIF segment is
/// inserted after encoding the way androidx ExifInterface does: the MP header and
/// gain map move together (offsets stay exact) but the primary size entry is stale.
fn ultra_hdr(xmp_after_mpf: bool, declared_gain: usize, exif: bool) -> Vec<u8> {
    let xmp = gain_map_xmp(declared_gain);
    let placeholder = mpf(0, 0, 0);
    let scan = [0xff, 0xda, 0, 2, 0xff, 0xd9];
    let primary_len = 2 + xmp.len() + placeholder.len() + scan.len();
    let mpf_at = 2 + if xmp_after_mpf { 0 } else { xmp.len() };
    let header = mpf_at + 8;
    let mut out = vec![0xff, 0xd8];
    let table = mpf(
        primary_len as u32,
        (primary_len - header) as u32,
        GAIN_MAP.len() as u32,
    );
    if xmp_after_mpf {
        out.extend(&table);
        out.extend(&xmp);
    } else {
        out.extend(&xmp);
        out.extend(&table);
    }
    out.extend(scan);
    assert_eq!(out.len(), primary_len);
    out.extend(GAIN_MAP);
    if exif {
        let mut segment = segment(0xe1, b"Exif\0\0MM\0*\0\0\0\x08\0\0");
        segment.extend(out.split_off(2));
        out.extend(segment);
    }
    out
}

fn ordinary_mpf_jpeg() -> Vec<u8> {
    let mut packet = XMP.to_vec();
    packet.extend(br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:test="urn:test" test:value="kept"/></rdf:RDF></x:xmpmeta>"#);
    packet.push(0);
    let xmp = segment(0xe1, &packet);
    let placeholder = mpf(0, 0, 0);
    let scan = [0xff, 0xda, 0, 2, 1, 2, 3, 0xff, 0xd9];
    let primary_len = 2 + xmp.len() + placeholder.len() + scan.len();
    let header = 2 + xmp.len() + 8;
    let mut source = vec![0xff, 0xd8];
    source.extend(xmp);
    source.extend(mpf(
        primary_len as u32,
        (primary_len - header) as u32,
        GAIN_MAP.len() as u32,
    ));
    source.extend(scan);
    source.extend(GAIN_MAP);
    source
}

/// Returns (MP header position, [(size, offset)]) of the only MPF segment before SOS.
fn read_mpf(bytes: &[u8]) -> (usize, Vec<(u32, u32)>) {
    let mut at = 2;
    loop {
        assert_eq!(bytes[at], 0xff);
        let marker = bytes[at + 1];
        assert_ne!(marker, 0xda, "no MPF segment");
        let len = u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]) as usize;
        if marker == 0xe2 && bytes[at + 4..].starts_with(b"MPF\0") {
            let header = at + 8;
            let be = |i: usize| u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap());
            let ifd = header + be(header + 4) as usize;
            let fields = u16::from_be_bytes(bytes[ifd..ifd + 2].try_into().unwrap()) as usize;
            let field = (0..fields)
                .map(|i| ifd + 2 + i * 12)
                .find(|at| u16::from_be_bytes(bytes[*at..*at + 2].try_into().unwrap()) == 0xb002)
                .unwrap();
            let entries = header + be(field + 8) as usize;
            let count = be(field + 4) as usize / 16;
            return (
                header,
                (0..count)
                    .map(|j| (be(entries + j * 16 + 4), be(entries + j * 16 + 8)))
                    .collect(),
            );
        }
        at += 2 + len;
    }
}

fn scan_offset(bytes: &[u8]) -> usize {
    let mut at = 2;
    loop {
        assert_eq!(bytes[at], 0xff);
        if bytes[at + 1] == 0xda {
            return at;
        }
        let len = u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]) as usize;
        at += 2 + len;
    }
}

#[test]
fn ultra_hdr_gain_map_survives_motion_packaging() {
    for (xmp_after_mpf, exif) in [(false, false), (true, false), (false, true), (true, true)] {
        let root = workdir();
        let video = b"\0\0\0\x14ftypisom\0\0\0\0isom";
        let (image, movie, out) = (
            root.join("still.jpg"),
            root.join("video.mp4"),
            root.join("output.jpg"),
        );
        let source = ultra_hdr(xmp_after_mpf, GAIN_MAP.len(), exif);
        std::fs::write(&image, &source).unwrap();
        std::fs::write(&movie, video).unwrap();
        write_jpeg_motion(&image, &movie, &out, None).unwrap();
        let bytes = std::fs::read(&out).unwrap();

        assert!(bytes.ends_with(video));
        let gain_at = bytes.len() - video.len() - GAIN_MAP.len();
        assert_eq!(&bytes[gain_at..bytes.len() - video.len()], GAIN_MAP);
        assert_eq!(&bytes[gain_at - 2..gain_at], &[0xff, 0xd9]);
        let packets = bytes.windows(XMP.len()).filter(|w| *w == XMP).count();
        assert_eq!(packets, 1, "exactly one XMP packet");

        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains(r#"hdrgm:Version="1.0""#));
        assert!(text.contains(r#"GCamera:MotionPhoto="1""#));
        let primary = text.find(r#"Item:Semantic="Primary""#).unwrap();
        let gain = text
            .find(&format!(
                r#"Item:Semantic="GainMap" Item:Length="{}""#,
                GAIN_MAP.len()
            ))
            .unwrap();
        let motion = text
            .find(&format!(
                r#"Item:Semantic="MotionPhoto" Item:Length="{}""#,
                video.len()
            ))
            .unwrap();
        assert!(primary < gain && gain < motion);

        let (header, entries) = read_mpf(&bytes);
        assert_eq!(
            entries[0],
            (gain_at as u32, 0),
            "primary size follows the new packet"
        );
        assert_eq!(entries[1].0 as usize, GAIN_MAP.len());
        assert_eq!(
            header + entries[1].1 as usize,
            gain_at,
            "gain map offset still resolves"
        );
        assert_eq!(std::fs::read(&image).unwrap(), source);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn ordinary_xmp_is_merged_without_touching_image_or_video() {
    let root = workdir();
    let video = b"\0\0\0\x14ftypqt  \0\0\0\0qt  ";
    let movie = root.join("paired.mov");
    std::fs::write(&movie, video).unwrap();
    let mut plain = vec![0xff, 0xd8];
    let exif = segment(0xe1, b"Exif\0\0untouched EXIF");
    let icc = segment(0xe2, b"ICC_PROFILE\0untouched ICC");
    plain.extend(&exif);
    plain.extend(&icc);
    let mut packet = XMP.to_vec();
    let original_xml = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description xmlns:test="urn:test" test:value="kept"/></r:RDF></x:xmpmeta>"#;
    packet.extend(original_xml);
    packet.extend([0, 0]);
    plain.extend(segment(0xe1, &packet));
    let scan = [0xff, 0xda, 0, 2, 1, 2, 3, 0xff, 0xd9];
    plain.extend(scan);
    let image = root.join("plain.jpg");
    let out = root.join("motion.jpg");
    std::fs::write(&image, &plain).unwrap();
    write_jpeg_motion_with_burst_and_video_mime(
        &image,
        &movie,
        &out,
        None,
        None,
        "video/quicktime",
    )
    .unwrap();
    let result = std::fs::read(&out).unwrap();
    assert_eq!(result.windows(XMP.len()).filter(|w| *w == XMP).count(), 1);
    assert!(result.windows(exif.len()).any(|w| w == exif));
    assert!(result.windows(icc.len()).any(|w| w == icc));
    assert!(result.windows(scan.len()).any(|w| w == scan));
    assert!(String::from_utf8_lossy(&result).contains(r#"test:value="kept"/>"#));
    assert!(String::from_utf8_lossy(&result).contains("test:value=\"kept\""));
    assert!(String::from_utf8_lossy(&result).contains("GCamera:MotionPhoto=\"1\""));
    assert!(result.ends_with(video));
    assert!(!result.windows(4).any(|w| w == b"SEFH"));
    assert_eq!(std::fs::read(image).unwrap(), plain);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ordinary_xmp_with_mpf_keeps_auxiliary_jpeg_and_rebases_offsets() {
    let root = workdir();
    let image = root.join("still.jpg");
    let movie = root.join("paired.mov");
    let output = root.join("output.jpg");
    let source = ordinary_mpf_jpeg();
    let video = b"\0\0\0\x14ftypqt  \0\0\0\0qt  ";
    std::fs::write(&image, &source).unwrap();
    std::fs::write(&movie, video).unwrap();
    write_jpeg_motion_with_burst_and_video_mime(
        &image,
        &movie,
        &output,
        None,
        None,
        "video/quicktime",
    )
    .unwrap();
    let result = std::fs::read(output).unwrap();
    let (header, entries) = read_mpf(&result);
    let auxiliary = header + entries[1].1 as usize;
    assert_eq!(entries[0].0 as usize, auxiliary);
    assert_eq!(entries[1].0 as usize, GAIN_MAP.len());
    assert_eq!(&result[auxiliary..auxiliary + GAIN_MAP.len()], GAIN_MAP);
    assert!(result.ends_with(video));
    assert!(!result.windows(4).any(|w| w == b"SEFH"));
    assert!(String::from_utf8_lossy(&result).contains("test:value=\"kept\""));
    assert!(String::from_utf8_lossy(&result).contains("GCamera:MotionPhoto=\"1\""));
    assert!(String::from_utf8_lossy(&result).contains(&format!(
        "Item:Semantic=\"GainMap\" Item:Length=\"{}\"",
        GAIN_MAP.len()
    )));
    assert_eq!(std::fs::read(image).unwrap(), source);
    std::fs::remove_dir_all(root).unwrap();
}

/// Big-endian MPF APP2 with a primary entry followed by `secondary` entries.
fn mpf_entries(primary_size: u32, secondary: &[(u32, u32)]) -> Vec<u8> {
    let count = 1 + secondary.len() as u32;
    let mut p = b"MPF\0MM\0*".to_vec();
    p.extend(8u32.to_be_bytes()); // IFD
    p.extend(1u16.to_be_bytes());
    p.extend(0xb002u16.to_be_bytes());
    p.extend(7u16.to_be_bytes());
    p.extend((16 * count).to_be_bytes());
    p.extend(26u32.to_be_bytes()); // entries after IFD (8 + 2 + 12 + 4)
    p.extend(0u32.to_be_bytes()); // next IFD
    for (attr, size, offset) in std::iter::once((0x0003_0000u32, primary_size, 0u32))
        .chain(secondary.iter().map(|(size, offset)| (0, *size, *offset)))
    {
        p.extend(attr.to_be_bytes());
        p.extend(size.to_be_bytes());
        p.extend(offset.to_be_bytes());
        p.extend([0u8; 4]);
    }
    segment(0xe2, &p)
}

/// An iPhone 16 style JPEG: ordinary Apple XMP, a gain map and further MPF
/// images (segmentation mattes) appended back to back. `gap` inserts a stray
/// byte between two secondary images.
fn apple_multi_mpf_jpeg(gap: bool) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut packet = XMP.to_vec();
    packet.extend(br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:test="urn:test" test:value="kept"/></rdf:RDF></x:xmpmeta>"#);
    packet.push(0);
    let xmp = segment(0xe1, &packet);
    let images: Vec<Vec<u8>> = (0..3u8)
        .map(|i| vec![0xff, 0xd8, 0xff, 0xda, 0, 2, i, i, 0xff, 0xd9])
        .collect();
    let placeholder = mpf_entries(0, &[(0, 0); 3]);
    let scan = [0xff, 0xda, 0, 2, 1, 2, 3, 0xff, 0xd9];
    let primary_len = 2 + xmp.len() + placeholder.len() + scan.len();
    let header = 2 + xmp.len() + 8;
    let mut offset = primary_len - header;
    let mut secondary = Vec::new();
    for (index, image) in images.iter().enumerate() {
        secondary.push((image.len() as u32, offset as u32));
        offset += image.len() + usize::from(gap && index == 0);
    }
    let mut source = vec![0xff, 0xd8];
    source.extend(xmp);
    source.extend(mpf_entries(primary_len as u32, &secondary));
    source.extend(scan);
    for (index, image) in images.iter().enumerate() {
        source.extend(image);
        if gap && index == 0 {
            source.push(0);
        }
    }
    (source, images)
}

#[test]
fn apple_jpeg_with_several_mpf_images_keeps_them_all() {
    let root = workdir();
    let image = root.join("still.jpg");
    let movie = root.join("paired.mov");
    let output = root.join("output.jpg");
    let (source, images) = apple_multi_mpf_jpeg(false);
    let video = b"\0\0\0\x14ftypqt  \0\0\0\0qt  ";
    std::fs::write(&image, &source).unwrap();
    std::fs::write(&movie, video).unwrap();
    write_jpeg_motion_with_burst_and_video_mime(
        &image,
        &movie,
        &output,
        None,
        None,
        "video/quicktime",
    )
    .unwrap();
    let result = std::fs::read(&output).unwrap();
    let (header, entries) = read_mpf(&result);
    assert_eq!(entries.len(), 4);
    let auxiliary = header + entries[1].1 as usize;
    assert_eq!(entries[0].0 as usize, auxiliary);
    for (entry, original) in entries[1..].iter().zip(&images) {
        let start = header + entry.1 as usize;
        assert_eq!(&result[start..start + entry.0 as usize], &original[..]);
    }
    let total: usize = images.iter().map(Vec::len).sum();
    assert!(result.ends_with(video));
    assert_eq!(result.len() - video.len(), auxiliary + total);
    let text = String::from_utf8_lossy(&result);
    assert!(text.contains("test:value=\"kept\""));
    assert!(text.contains(&format!(
        "Item:Semantic=\"GainMap\" Item:Length=\"{total}\""
    )));

    // Secondary images that are not back to back do not describe the file.
    std::fs::remove_file(&output).unwrap();
    std::fs::write(&image, apple_multi_mpf_jpeg(true).0).unwrap();
    let error = write_jpeg_motion_with_burst_and_video_mime(
        &image,
        &movie,
        &output,
        None,
        None,
        "video/quicktime",
    )
    .unwrap_err();
    assert!(
        matches!(error, backupduck_core::Error::Unsupported(reason) if reason == "Ultra HDR JPEG layout")
    );
    assert!(!output.exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn conflicting_or_inconsistent_xmp_is_still_refused() {
    let root = workdir();
    let video = b"\0\0\0\x14ftypisom\0\0\0\0isom";
    let movie = root.join("video.mp4");
    std::fs::write(&movie, video).unwrap();
    let mut conflict = vec![0xff, 0xd8];
    let mut packet = XMP.to_vec();
    packet.extend(br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:cam="http://ns.google.com/photos/1.0/camera/" cam:MotionPhoto="1"/></rdf:RDF></x:xmpmeta>"#);
    conflict.extend(segment(0xe1, &packet));
    conflict.extend([0xff, 0xda, 0, 2, 0xff, 0xd9]);
    for (name, source) in [
        ("conflict.jpg", conflict),
        ("mismatch.jpg", ultra_hdr(false, GAIN_MAP.len() + 1, true)),
    ] {
        let image = root.join(name);
        let out = root.join(format!("{name}.out"));
        std::fs::write(&image, source).unwrap();
        assert!(
            write_jpeg_motion(&image, &movie, &out, None).is_err(),
            "{name}"
        );
        assert!(!out.exists() && !out.with_extension("motion.partial").exists());
    }
    std::fs::remove_dir_all(root).unwrap();
}
