//! Synthetic HEIF builders and an independent item-table reader shared by the
//! HEIC integration tests.
#![allow(dead_code)]

pub fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend(kind);
    out.extend(body);
    out
}

/// One hvc1 primary item whose single extent is `pixels`; no XMP.
pub fn single_item_heic(pixels: &[u8]) -> Vec<u8> {
    let ftyp = atom(b"ftyp", b"heic\0\0\0\0mif1heic");
    let pitm = atom(b"pitm", &[0, 0, 0, 0, 0, 1]);
    let infe = atom(b"infe", b"\x02\0\0\x01\0\x01\0\0hvc1\0");
    let mut iinf_body = vec![0, 0, 0, 0, 0, 1];
    iinf_body.extend(infe);
    let iinf = atom(b"iinf", &iinf_body);
    let iref = atom(b"iref", &[0, 0, 0, 0]);
    let mut iloc_body = vec![1, 0, 0, 0, 0x44, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1];
    iloc_body.extend([0, 0, 0, 0]);
    iloc_body.extend((pixels.len() as u32).to_be_bytes());
    let mut meta_body = vec![0, 0, 0, 0];
    meta_body.extend(pitm);
    meta_body.extend(iinf);
    meta_body.extend(iref);
    meta_body.extend(atom(b"iloc", &iloc_body));
    let mut meta = atom(b"meta", &meta_body);
    let image_offset = (ftyp.len() + meta.len() + 8) as u32;
    let iloc_start = meta.windows(4).position(|w| w == b"iloc").unwrap() - 4;
    meta[iloc_start + 24..iloc_start + 28].copy_from_slice(&image_offset.to_be_bytes());
    let mut image = ftyp;
    image.extend(meta);
    image.extend(atom(b"mdat", pixels));
    image
}

/// Items 1 and 2 are hvc1 images; 3 is the primary's XMP and 4 the XMP of
/// item 2. Returns the file and its mdat payload.
pub fn heic_with_xmp(primary: &[u8], auxiliary: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let ftyp = atom(b"ftyp", b"heic\0\0\0\0mif1heic");
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
    (original, payload)
}

/// Top-level boxes as (offset, size, type), honouring 64-bit sizes.
pub fn boxes(data: &[u8], mut at: usize, end: usize) -> Vec<(usize, usize, [u8; 4])> {
    let mut result = Vec::new();
    while at < end {
        let mut size = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        if size == 1 {
            size = u64::from_be_bytes(data[at + 8..at + 16].try_into().unwrap()) as usize;
        }
        result.push((at, size, data[at + 4..at + 8].try_into().unwrap()));
        at += size;
    }
    result
}

fn uint(data: &[u8], at: usize, width: usize) -> usize {
    data[at..at + width]
        .iter()
        .fold(0, |n, b| n << 8 | *b as usize)
}

/// The parts of a HEIF item table the tests check.
pub struct Items {
    pub primary: usize,
    /// Item IDs whose infe declares `application/rdf+xml`.
    pub xmp: Vec<usize>,
    /// cdsc references as (from, to).
    pub cdsc: Vec<(usize, usize)>,
    /// Single-extent locations as (id, offset, length).
    pub locations: Vec<(usize, usize, usize)>,
}

impl Items {
    pub fn location(&self, id: usize) -> (usize, usize) {
        let (_, offset, length) = *self.locations.iter().find(|l| l.0 == id).unwrap();
        (offset, length)
    }
    /// XMP items referenced from the primary image, in iref order.
    pub fn primary_xmp(&self) -> Vec<usize> {
        self.cdsc
            .iter()
            .filter(|(from, to)| *to == self.primary && self.xmp.contains(from))
            .map(|(from, _)| *from)
            .collect()
    }
}

pub fn items(data: &[u8]) -> Items {
    let (at, size, _) = boxes(data, 0, data.len())
        .into_iter()
        .find(|b| &b.2 == b"meta")
        .unwrap();
    let meta = &data[at..at + size];
    let children = boxes(meta, 12, meta.len());
    let child = |kind: &[u8; 4]| {
        children
            .iter()
            .find(|b| &b.2 == kind)
            .map(|&(at, size, _)| &meta[at..at + size])
    };
    let primary = uint(child(b"pitm").unwrap(), 12, 2);
    let iinf = child(b"iinf").unwrap();
    let xmp = boxes(iinf, 14, iinf.len())
        .into_iter()
        .filter_map(|(at, size, _)| {
            let e = &iinf[at..at + size];
            (e[16..].starts_with(b"mime\0application/rdf+xml\0")).then(|| uint(e, 12, 2))
        })
        .collect();
    let mut cdsc = Vec::new();
    if let Some(iref) = child(b"iref") {
        let width = if iref[8] == 0 { 2 } else { 4 };
        for (at, size, kind) in boxes(iref, 12, iref.len()) {
            let e = &iref[at..at + size];
            if &kind == b"cdsc" {
                let from = uint(e, 8, width);
                for i in 0..uint(e, 8 + width, 2) {
                    cdsc.push((from, uint(e, 10 + width + i * width, width)));
                }
            }
        }
    }
    let iloc = child(b"iloc").unwrap();
    let (offset_size, length_size) = ((iloc[12] >> 4) as usize, (iloc[12] & 15) as usize);
    let version = iloc[8];
    let mut at = 16;
    let mut locations = Vec::new();
    for _ in 0..uint(iloc, 14, 2) {
        let id = uint(iloc, at, 2);
        at += if version == 1 { 6 } else { 4 };
        let count = uint(iloc, at, 2);
        at += 2;
        for _ in 0..count {
            if count == 1 {
                locations.push((
                    id,
                    uint(iloc, at, offset_size),
                    uint(iloc, at + offset_size, length_size),
                ));
            }
            at += offset_size + length_size;
        }
    }
    Items {
        primary,
        xmp,
        cdsc,
        locations,
    }
}

// Independent reader: follow the first primary cdsc reference, as Android's
// ItemTable does, rather than searching the entire file for a MotionPhoto string.
pub fn primary_xmp(data: &[u8]) -> (Vec<u8>, usize) {
    let items = items(data);
    let refs = items.primary_xmp();
    let (offset, length) = items.location(refs[0]);
    (data[offset..offset + length].to_vec(), refs.len())
}
