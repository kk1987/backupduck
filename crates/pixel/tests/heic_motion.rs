use photobridge_pixel::{
    write_heic_motion_with_burst, write_heic_motion_with_burst_and_video_mime,
};
use std::{fs, path::PathBuf};

fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend(kind);
    out.extend(body);
    out
}

#[test]
fn heic_and_mov_payloads_are_preserved() {
    let root = std::env::temp_dir().join(format!("photobridge-motion-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let still = root.join("still.heic");
    let video = root.join("paired.mov");
    let output = root.join("result.heic");
    let ftyp = atom(b"ftyp", b"heic\0\0\0\0mif1heic");
    let pitm = atom(b"pitm", &[0, 0, 0, 0, 0, 1]);
    let infe = atom(b"infe", b"\x02\0\0\x01\0\x01\0\0hvc1\0");
    let mut iinf_body = vec![0, 0, 0, 0, 0, 1];
    iinf_body.extend(infe);
    let iinf = atom(b"iinf", &iinf_body);
    let iref = atom(b"iref", &[0, 0, 0, 0]);
    let mut iloc_body = vec![1, 0, 0, 0, 0x44, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1];
    iloc_body.extend([0, 0, 0, 0, 0, 0, 0, 5]);
    let mut meta_body = vec![0, 0, 0, 0];
    meta_body.extend(pitm);
    meta_body.extend(iinf);
    meta_body.extend(iref);
    meta_body.extend(atom(b"iloc", &iloc_body));
    let mut meta = atom(b"meta", &meta_body);
    let image_offset = (ftyp.len() + meta.len() + 8) as u32;
    let iloc_start = meta.windows(4).position(|w| w == b"iloc").unwrap() - 4;
    meta[iloc_start + 24..iloc_start + 28].copy_from_slice(&image_offset.to_be_bytes());
    let pixels = b"abcde";
    let mut image = ftyp;
    image.extend(meta);
    image.extend(atom(b"mdat", pixels));
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
    let Ok(dir) = std::env::var("PHOTOBRIDGE_LIVE_PHOTO_FIXTURE") else {
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

// Independent reader: follow the first primary cdsc reference, as Android's
// ItemTable does, rather than searching the entire file for a MotionPhoto string.
fn primary_xmp(data: &[u8]) -> (Vec<u8>, usize) {
    fn boxes(data: &[u8], mut at: usize) -> Vec<(usize, usize, &[u8])> {
        let mut result = Vec::new();
        while at < data.len() {
            let size = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
            result.push((at, size, &data[at + 4..at + 8]));
            at += size;
        }
        result
    }
    let u16_at = |b: &[u8], at| u16::from_be_bytes(b[at..at + 2].try_into().unwrap());
    let u32_at = |b: &[u8], at| u32::from_be_bytes(b[at..at + 4].try_into().unwrap());
    let (at, size, _) = boxes(data, 0).into_iter().find(|b| b.2 == b"meta").unwrap();
    let meta = &data[at..at + size];
    let children = boxes(meta, 12);
    let child = |kind: &[u8]| {
        let &(at, size, _) = children.iter().find(|b| b.2 == kind).unwrap();
        &meta[at..at + size]
    };
    let primary = u16_at(child(b"pitm"), 12);
    let iinf = child(b"iinf");
    let ids: Vec<_> = boxes(iinf, 14)
        .into_iter()
        .filter_map(|(at, size, _)| {
            let e = &iinf[at..at + size];
            (e[16..].starts_with(b"mime\0application/rdf+xml\0")).then(|| u16_at(e, 12))
        })
        .collect();
    let iref = child(b"iref");
    let refs: Vec<_> = boxes(iref, 12)
        .into_iter()
        .filter_map(|(at, size, kind)| {
            let e = &iref[at..at + size];
            (kind == b"cdsc"
                && ids.contains(&u16_at(e, 8))
                && e[12..].chunks_exact(2).any(|b| u16_at(b, 0) == primary))
            .then(|| u16_at(e, 8))
        })
        .collect();
    let iloc = child(b"iloc");
    let mut at = 16;
    for _ in 0..u16_at(iloc, 14) {
        let id = u16_at(iloc, at);
        let count = u16_at(iloc, at + 6);
        if id == refs[0] {
            assert_eq!(count, 1);
            let offset = u32_at(iloc, at + 8) as usize;
            let length = u32_at(iloc, at + 12) as usize;
            return (data[offset..offset + length].to_vec(), refs.len());
        }
        at += 8 + count as usize * 8;
    }
    panic!("missing primary XMP extent")
}

#[test]
fn existing_primary_and_auxiliary_xmp_remain_readable() {
    let root = std::env::temp_dir().join(format!("photobridge-motion-xmp-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let ftyp = atom(b"ftyp", b"heic\0\0\0\0mif1heic");
    let primary = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description xmlns:apple="urn:apple:fixture" apple:Color="HDR"/></r:RDF></x:xmpmeta>"#;
    let auxiliary = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description xmlns:depth="urn:depth:fixture" depth:Value="preserved"/></r:RDF></x:xmpmeta>"#;
    let mut iinf = vec![0, 0, 0, 0, 0, 4];
    for (id, kind) in [
        (1u16, b"hvc1\0".as_slice()),
        (2, b"hvc1\0"),
        (3, b"mime\0application/rdf+xml\0"),
        (4, b"mime\0application/rdf+xml\0"),
    ] {
        let mut e = vec![2, 0, 0, 1];
        e.extend(id.to_be_bytes());
        e.extend([0, 0]);
        e.extend(kind);
        iinf.extend(atom(b"infe", &e));
    }
    let mut iref = vec![0, 0, 0, 0];
    for (id, target) in [(3u16, 1u16), (4, 2)] {
        let mut r = id.to_be_bytes().to_vec();
        r.extend(1u16.to_be_bytes());
        r.extend(target.to_be_bytes());
        iref.extend(atom(b"cdsc", &r));
    }
    let lengths = [5usize, 5, primary.len(), auxiliary.len()];
    let make_meta = |payload_offset: usize| {
        let mut iloc = vec![1, 0, 0, 0, 0x44, 0, 0, 4];
        let mut offset = payload_offset;
        for (index, length) in lengths.iter().enumerate() {
            iloc.extend((index as u16 + 1).to_be_bytes());
            iloc.extend([0, 0, 0, 0, 0, 1]);
            iloc.extend((offset as u32).to_be_bytes());
            iloc.extend((*length as u32).to_be_bytes());
            offset += length;
        }
        let mut meta = vec![0, 0, 0, 0];
        meta.extend(atom(b"pitm", &[0, 0, 0, 0, 0, 1]));
        meta.extend(atom(b"iinf", &iinf));
        meta.extend(atom(b"iref", &iref));
        meta.extend(atom(b"iloc", &iloc));
        atom(b"meta", &meta)
    };
    let meta = make_meta(ftyp.len() + make_meta(0).len() + 8);
    let mut payload = b"abcdefghij".to_vec();
    payload.extend(primary);
    payload.extend(auxiliary);
    let mut original = ftyp;
    original.extend(meta);
    original.extend(atom(b"mdat", &payload));
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
