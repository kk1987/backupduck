//! Lossless HEIC + QuickTime Live Photo packaging for the Pixel receiver.
//! Only the HEIF item tables and XMP are rewritten; image and video payloads
//! are copied byte-for-byte. Unsupported HEIF layouts use the codec fallback.
//! The primary-XMP rewrite is shared with HEIC burst delivery copies.
use crate::burst::burst_attributes;
use backupduck_core::{BurstMetadata, Error, Result};
use quick_xml::{events::Event, name::ResolveResult, reader::NsReader};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

fn unsupported() -> Error {
    Error::Unsupported("HEIC container layout".into())
}
fn number(data: &[u8], at: usize, width: usize) -> Result<u64> {
    if width > 8 {
        return Err(unsupported());
    }
    let bytes = data
        .get(at..at.checked_add(width).ok_or_else(unsupported)?)
        .ok_or_else(unsupported)?;
    Ok(bytes.iter().fold(0u64, |n, b| n << 8 | u64::from(*b)))
}
fn put_number(data: &mut [u8], at: usize, width: usize, value: u64) -> Result<()> {
    if width > 8 || (width < 8 && value >= 1u64 << (width * 8)) {
        return Err(unsupported());
    }
    let target = data
        .get_mut(at..at.checked_add(width).ok_or_else(unsupported)?)
        .ok_or_else(unsupported)?;
    for (i, byte) in target.iter_mut().enumerate() {
        *byte = (value >> ((width - i - 1) * 8)) as u8;
    }
    Ok(())
}
fn boxes(data: &[u8], start: usize, end: usize) -> Result<Vec<(usize, usize, [u8; 4])>> {
    let mut result = Vec::new();
    let mut at = start;
    while at < end {
        let short_size = number(data, at, 4)?;
        let size = usize::try_from(if short_size == 1 {
            number(data, at + 8, 8)?
        } else {
            short_size
        })
        .map_err(|_| unsupported())?;
        let typ: [u8; 4] = data
            .get(at + 4..at + 8)
            .ok_or_else(unsupported)?
            .try_into()
            .unwrap();
        if size < (if short_size == 1 { 16 } else { 8 })
            || at.checked_add(size).filter(|v| *v <= end).is_none()
        {
            return Err(unsupported());
        }
        result.push((at, size, typ));
        at += size;
    }
    Ok(result)
}
fn box_data(typ: &[u8; 4], payload: &[u8]) -> Result<Vec<u8>> {
    let size = u32::try_from(payload.len().checked_add(8).ok_or(Error::Capacity)?)
        .map_err(|_| Error::Capacity)?;
    let mut result = Vec::with_capacity(size as usize);
    result.extend(size.to_be_bytes());
    result.extend(typ);
    result.extend(payload);
    Ok(result)
}
fn extend_count(
    mut value: Vec<u8>,
    count_at: usize,
    width: usize,
    extra: &[u8],
) -> Result<Vec<u8>> {
    let count = number(&value, count_at, width)?;
    put_number(
        &mut value,
        count_at,
        width,
        count.checked_add(1).ok_or(Error::Capacity)?,
    )?;
    value.extend(extra);
    let size = u32::try_from(value.len()).map_err(|_| Error::Capacity)?;
    value[..4].copy_from_slice(&size.to_be_bytes());
    Ok(value)
}

const RDF: &[u8] = b"http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const CAMERA: &[u8] = b"http://ns.google.com/photos/1.0/camera/";
const CONTAINER: &[u8] = b"http://ns.google.com/photos/1.0/container/";

/// What an existing primary XMP packet may already declare.
pub(crate) enum MergePolicy<'a> {
    /// No GCamera or Container attribute at all.
    Motion,
    /// Only this burst's own BurstID/BurstPrimary pair; the merge is then a no-op.
    Burst(&'a BurstMetadata),
}

/// Keep one authoritative XMP item for the primary image. Android readers use
/// the first cdsc XMP reference, so appending another item hides MotionPhoto
/// behind the camera's existing metadata (notably on recent iPhone HEICs).
fn merge_primary_xmp(original: &[u8], description: &str, policy: &MergePolicy) -> Result<Vec<u8>> {
    let mut reader = NsReader::from_reader(original);
    let mut insertion = None;
    let mut depth = 0usize;
    let (mut burst_id, mut burst_primary) = (false, false);
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|_| unsupported())?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                if let MergePolicy::Burst(_) = policy {
                    // Element-form properties would duplicate the attributes added below.
                    let (ns, _) = reader.resolver().resolve_element(e.name());
                    if matches!(ns, ResolveResult::Bound(n) if n.as_ref() == CAMERA || n.as_ref() == CONTAINER)
                    {
                        return Err(unsupported());
                    }
                }
                for attr in e.attributes() {
                    let attr = attr.map_err(|_| unsupported())?;
                    let (ns, name) = reader.resolver().resolve_attribute(attr.key);
                    let ResolveResult::Bound(ns) = ns else {
                        continue;
                    };
                    if ns.as_ref() == CONTAINER {
                        return Err(unsupported());
                    }
                    if ns.as_ref() != CAMERA {
                        continue;
                    }
                    let MergePolicy::Burst(burst) = policy else {
                        return Err(unsupported());
                    };
                    let (seen, expected) = match name.as_ref() {
                        b"BurstID" => (&mut burst_id, burst.group_id.as_bytes()),
                        b"BurstPrimary" => (
                            &mut burst_primary,
                            if burst.primary { b"1".as_slice() } else { b"0" },
                        ),
                        _ => return Err(unsupported()),
                    };
                    if attr.value.as_ref() != expected {
                        return Err(unsupported());
                    }
                    *seen = true;
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                    if depth > 128 {
                        return Err(unsupported());
                    }
                }
            }
            Event::End(e) => {
                let (ns, name) = reader.resolver().resolve_element(e.name());
                if matches!(ns, ResolveResult::Bound(n) if n.as_ref() == RDF)
                    && name.as_ref() == b"RDF"
                    && insertion.replace(start).is_some()
                {
                    return Err(unsupported());
                }
                depth = depth.checked_sub(1).ok_or_else(unsupported)?;
            }
            Event::DocType(_) => return Err(unsupported()),
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 || burst_id != burst_primary {
        return Err(unsupported());
    }
    if burst_id {
        return Ok(original.to_vec());
    }
    let at = insertion.ok_or_else(unsupported)?;
    let mut merged = original[..at].to_vec();
    merged.extend(description.as_bytes());
    merged.extend(&original[at..]);
    Ok(merged)
}

/// Return `still` with `description` in the primary image's XMP item: merged
/// into the existing cdsc item, or added as a new item. Image payloads are
/// copied byte-for-byte; only `iinf`, `iref`, `iloc` and `mdat` sizes change.
/// `description` is an `rdf:Description` element without an `xmlns:rdf`
/// declaration. The result equals `still` when the policy finds nothing to add.
pub(crate) fn rewrite_heic_primary_xmp(
    still: &[u8],
    description: &str,
    policy: MergePolicy,
) -> Result<Vec<u8>> {
    let top = boxes(still, 0, still.len())?;
    if top.len() != 3 || top[0].2 != *b"ftyp" || top[1].2 != *b"meta" || top[2].2 != *b"mdat" {
        return Err(unsupported());
    }
    if !still[8..top[0].1]
        .windows(4)
        .any(|b| matches!(b, b"heic" | b"heix" | b"mif1"))
    {
        return Err(unsupported());
    }
    let meta = &still[top[1].0..top[1].0 + top[1].1];
    let children = boxes(meta, 12, meta.len())?;
    let child = |typ: &[u8; 4]| -> Result<&[u8]> {
        let matches: Vec<_> = children.iter().filter(|c| &c.2 == typ).collect();
        if matches.len() != 1 {
            return Err(unsupported());
        }
        let c = matches[0];
        Ok(&meta[c.0..c.0 + c.1])
    };
    let pitm = child(b"pitm")?;
    if pitm[8] != 0 {
        return Err(unsupported());
    }
    let primary_id = u16::try_from(number(pitm, 12, 2)?).map_err(|_| unsupported())?;
    let iinf = child(b"iinf")?;
    let iloc = child(b"iloc")?;
    if iinf[8] != 0 || !matches!(iloc[8], 0 | 1) {
        return Err(unsupported());
    }
    // A single-image HEIC may have no item references at all.
    let iref_missing = !children.iter().any(|c| &c.2 == b"iref");
    let empty_iref = box_data(b"iref", &[0, 0, 0, 0])?;
    let iref = if iref_missing {
        empty_iref.as_slice()
    } else {
        child(b"iref")?
    };
    // Version 1 (written by little_exif, for one) uses 32-bit item IDs.
    let ref_width = match iref[8] {
        0 => 2,
        1 => 4,
        _ => return Err(unsupported()),
    };
    let offset_size = (iloc[12] >> 4) as usize;
    let length_size = (iloc[12] & 15) as usize;
    let base_size = (iloc[13] >> 4) as usize;
    let index_size = if iloc[8] == 1 {
        (iloc[13] & 15) as usize
    } else {
        0
    };
    if !matches!(offset_size, 4 | 8)
        || !matches!(length_size, 4 | 8)
        || base_size != 0
        || index_size != 0
    {
        return Err(unsupported());
    }
    let count = number(iloc, 14, 2)? as usize;
    let mut at = 16;
    let mut offsets = Vec::new();
    let mut locations = std::collections::BTreeMap::new();
    let mut xmp_ids = std::collections::BTreeSet::new();
    let mut max_id = primary_id;
    let entries = boxes(iinf, 14, iinf.len())?;
    if entries.len() != number(iinf, 12, 2)? as usize {
        return Err(unsupported());
    }
    for (entry_at, _, kind) in entries {
        if kind != *b"infe" || iinf[entry_at + 8] != 2 {
            return Err(unsupported());
        }
        let id = number(iinf, entry_at + 12, 2)? as u16;
        max_id = max_id.max(id);
        let entry_end = entry_at + number(iinf, entry_at, 4)? as usize;
        if iinf[entry_at + 16..entry_at + 20] == *b"mime" {
            let fields = &iinf[entry_at + 20..entry_end];
            if fields.split(|b| *b == 0).nth(1) == Some(b"application/rdf+xml".as_slice()) {
                xmp_ids.insert(id);
            }
        }
    }
    for _ in 0..count {
        let id = number(iloc, at, 2)? as u16;
        max_id = max_id.max(id);
        at += 2;
        let method = if iloc[8] == 1 {
            let v = number(iloc, at, 2)?;
            at += 2;
            v & 15
        } else {
            0
        };
        let reference = number(iloc, at, 2)?;
        at += 2;
        if reference != 0 || method > 1 {
            return Err(unsupported());
        }
        let extent_count = number(iloc, at, 2)? as usize;
        at += 2;
        for _ in 0..extent_count {
            let offset = number(iloc, at, offset_size)?;
            if method == 0 && extent_count == 1 {
                locations.insert(
                    id,
                    (at, offset, number(iloc, at + offset_size, length_size)?),
                );
            }
            if method == 0 {
                offsets.push((at, offset));
            }
            at += offset_size + length_size;
            if at > iloc.len() {
                return Err(unsupported());
            }
        }
    }
    if at != iloc.len() {
        return Err(unsupported());
    }
    let mut existing_xmp = None;
    for (at, size, kind) in boxes(iref, 12, iref.len())? {
        if kind != *b"cdsc" {
            continue;
        }
        let id = number(iref, at + 8, ref_width)?;
        let count = number(iref, at + 8 + ref_width, 2)? as usize;
        if size != 10 + ref_width + count * ref_width {
            return Err(unsupported());
        }
        let Some(id) = u16::try_from(id).ok().filter(|id| xmp_ids.contains(id)) else {
            continue;
        };
        if (0..count).any(|i| {
            number(iref, at + 10 + ref_width + i * ref_width, ref_width).ok()
                == Some(u64::from(primary_id))
        }) && existing_xmp.replace(id).is_some()
        {
            return Err(unsupported());
        }
    }
    let xmp_id = match existing_xmp {
        Some(id) => id,
        None => max_id.checked_add(1).ok_or_else(unsupported)?,
    };
    let xmp = if let Some(id) = existing_xmp {
        let (_, offset, length) = *locations.get(&id).ok_or_else(unsupported)?;
        let start = usize::try_from(offset).map_err(|_| unsupported())?;
        let end = usize::try_from(offset.checked_add(length).ok_or_else(unsupported)?)
            .map_err(|_| unsupported())?;
        let existing = still.get(start..end).ok_or_else(unsupported)?;
        let description = description.replacen(
            "<rdf:Description",
            "<rdf:Description xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"",
            1,
        );
        let merged = merge_primary_xmp(existing, &description, &policy)?;
        if merged == existing {
            return Ok(still.to_vec());
        }
        merged
    } else {
        format!(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">{description}</rdf:RDF></x:xmpmeta>"#).into_bytes()
    };
    let xmp = xmp.as_slice();
    let mut infe = vec![2, 0, 0, 1];
    infe.extend(xmp_id.to_be_bytes());
    infe.extend([0, 0]);
    infe.extend(b"mime\0application/rdf+xml\0");
    let iinf_new = if existing_xmp.is_some() {
        iinf.to_vec()
    } else {
        extend_count(iinf.to_vec(), 12, 2, &box_data(b"infe", &infe)?)?
    };
    let mut cdsc = Vec::new();
    cdsc.extend(&u32::from(xmp_id).to_be_bytes()[4 - ref_width..]);
    cdsc.extend(1u16.to_be_bytes());
    cdsc.extend(&u32::from(primary_id).to_be_bytes()[4 - ref_width..]);
    let mut iref_new = iref.to_vec();
    if existing_xmp.is_none() {
        iref_new.extend(box_data(b"cdsc", &cdsc)?);
    }
    let iref_size = u32::try_from(iref_new.len()).map_err(|_| Error::Capacity)?;
    iref_new[..4].copy_from_slice(&iref_size.to_be_bytes());
    let mut new_extent = Vec::new();
    new_extent.extend(xmp_id.to_be_bytes());
    if iloc[8] == 1 {
        new_extent.extend([0, 0]);
    }
    new_extent.extend([0, 0, 0, 1]);
    new_extent.extend(vec![0; offset_size]);
    new_extent.extend(vec![0; length_size]);
    let mut iloc_new = if existing_xmp.is_some() {
        iloc.to_vec()
    } else {
        extend_count(iloc.to_vec(), 14, 2, &new_extent)?
    };
    let iref_old = if iref_missing { 0 } else { iref.len() };
    let meta_delta =
        iinf_new.len() + iref_new.len() + iloc_new.len() - iinf.len() - iref_old - iloc.len();
    let mdat_header = if number(still, top[2].0, 4)? == 1 {
        16
    } else {
        8
    };
    let old_payload = top[2].0 + mdat_header;
    let new_payload = old_payload.checked_add(meta_delta).ok_or(Error::Capacity)?;
    for (position, old) in offsets {
        if old < old_payload as u64 || old >= still.len() as u64 {
            return Err(unsupported());
        }
        put_number(
            &mut iloc_new,
            position,
            offset_size,
            old + meta_delta as u64 + xmp.len() as u64,
        )?;
    }
    let new_entry_offset = if let Some(id) = existing_xmp {
        locations[&id].0
    } else {
        iloc_new.len() - length_size - offset_size
    };
    put_number(
        &mut iloc_new,
        new_entry_offset,
        offset_size,
        new_payload as u64,
    )?;
    put_number(
        &mut iloc_new,
        new_entry_offset + offset_size,
        length_size,
        xmp.len() as u64,
    )?;
    let mut new_meta = meta[..12].to_vec();
    for c in children {
        match &c.2 {
            b"iinf" => {
                new_meta.extend(&iinf_new);
                if iref_missing {
                    new_meta.extend(&iref_new);
                }
            }
            b"iref" => new_meta.extend(&iref_new),
            b"iloc" => new_meta.extend(&iloc_new),
            _ => new_meta.extend(&meta[c.0..c.0 + c.1]),
        }
    }
    let meta_size = u32::try_from(new_meta.len()).map_err(|_| Error::Capacity)?;
    new_meta[..4].copy_from_slice(&meta_size.to_be_bytes());
    let mut mdat = still[top[2].0..old_payload].to_vec();
    let mdat_size = top[2].1.checked_add(xmp.len()).ok_or(Error::Capacity)?;
    if mdat_header == 16 {
        mdat[8..16].copy_from_slice(&(mdat_size as u64).to_be_bytes());
    } else {
        mdat[..4].copy_from_slice(
            &u32::try_from(mdat_size)
                .map_err(|_| Error::Capacity)?
                .to_be_bytes(),
        );
    }
    let mut image = Vec::with_capacity(top[0].1 + new_meta.len() + mdat_size);
    image.extend(&still[..top[0].1]);
    image.extend(&new_meta);
    image.extend(&mdat);
    image.extend(xmp);
    image.extend(&still[old_payload..]);
    Ok(image)
}

/// Append the unmodified MOV to a HEIC and register a Motion Photo XMP item.
pub fn write_heic_motion_with_burst(
    heic: &Path,
    mov: &Path,
    output: &Path,
    burst: Option<&BurstMetadata>,
) -> Result<()> {
    write_heic_motion_with_burst_and_video_mime(heic, mov, output, burst, "video/quicktime")
}

pub fn write_heic_motion_with_burst_and_video_mime(
    heic: &Path,
    mov: &Path,
    output: &Path,
    burst: Option<&BurstMetadata>,
    video_mime: &str,
) -> Result<()> {
    if !matches!(video_mime, "video/mp4" | "video/quicktime") {
        return Err(Error::Unsupported("motion video MIME".into()));
    }
    if heic == output || mov == output || output.exists() {
        return Err(Error::Invalid("motion delivery destination".into()));
    }
    if let Some(b) = burst {
        b.validate()?;
    }
    let still = fs::read(heic)?;
    if still.len() > 128 << 20 {
        return Err(unsupported());
    }
    if still
        .windows(b"GCamera:MotionPhoto=\"1\"".len())
        .any(|window| window == b"GCamera:MotionPhoto=\"1\"")
    {
        return Err(unsupported());
    }
    let mut video = File::open(mov)?;
    let video_size = video.metadata()?.len();
    if video_size < 16 || video_size > u32::MAX as u64 - 256 {
        return Err(unsupported());
    }
    let mut video_head = [0; 8];
    video.read_exact(&mut video_head)?;
    if &video_head[4..] != b"ftyp" {
        return Err(unsupported());
    }
    let footer_size = samsung_footer(0, 0).len() as u64;
    // mpvd header and SEF footer are included in the MotionPhoto item length,
    // while the primary item declares the eight-byte mpvd header as padding.
    let motion_length = video_size + footer_size;
    let burst_fields = burst.map(burst_attributes).unwrap_or_default();
    let description = format!(
        r#"<rdf:Description rdf:about="" xmlns:GCamera="http://ns.google.com/photos/1.0/camera/" xmlns:Container="http://ns.google.com/photos/1.0/container/" xmlns:Item="http://ns.google.com/photos/1.0/container/item/" GCamera:MotionPhoto="1" GCamera:MotionPhotoVersion="1" GCamera:MotionPhotoPresentationTimestampUs="-1"{burst_fields}><Container:Directory><rdf:Seq><rdf:li rdf:parseType="Resource"><Container:Item Item:Mime="image/heic" Item:Semantic="Primary" Item:Length="0" Item:Padding="8"/></rdf:li><rdf:li rdf:parseType="Resource"><Container:Item Item:Mime="{video_mime}" Item:Semantic="MotionPhoto" Item:Length="{motion_length}" Item:Padding="0"/></rdf:li></rdf:Seq></Container:Directory></rdf:Description>"#
    );
    let image = rewrite_heic_primary_xmp(&still, &description, MergePolicy::Motion)?;
    let image_size = image.len();
    if image_size
        .checked_add(8)
        .filter(|size| *size <= u32::MAX as usize)
        .is_none()
    {
        return Err(unsupported());
    }
    let footer = samsung_footer(image_size, video_size as u32);
    let mpvd_size =
        u32::try_from(8 + video_size as usize + footer.len()).map_err(|_| Error::Capacity)?;
    let partial = output.with_extension("motion.partial");
    let mut out = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&partial)?;
    let result = (|| -> Result<()> {
        out.write_all(&image)?;
        out.write_all(&mpvd_size.to_be_bytes())?;
        out.write_all(b"mpvd")?;
        let mut video = File::open(mov)?;
        std::io::copy(&mut video, &mut out)?;
        out.write_all(&footer)?;
        out.sync_all()?;
        drop(out);
        fs::rename(&partial, output)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

fn samsung_footer(image_size: usize, video_size: u32) -> Vec<u8> {
    let tags: [([u8; 4], &str, Vec<u8>); 2] = [
        ([0, 0, 0x30, 0x0a], "MotionPhoto_Data", {
            let mut v = b"mpv2".to_vec();
            v.extend(((image_size + 8) as u32).to_be_bytes());
            v.extend(video_size.to_be_bytes());
            v
        }),
        ([0, 0, 0x31, 0x0a], "MotionPhoto_Version", b"mpv3".to_vec()),
    ];
    let mut data = Vec::new();
    let mut lengths = Vec::new();
    for (id, name, value) in &tags {
        let start = data.len();
        data.extend(id);
        data.extend((name.len() as u32).to_le_bytes());
        data.extend(name.as_bytes());
        data.extend(value);
        lengths.push((data.len() - start) as u32);
    }
    let mut sefh = b"SEFH".to_vec();
    sefh.extend(107u32.to_le_bytes());
    sefh.extend(2u32.to_le_bytes());
    for (i, (id, _, _)) in tags.iter().enumerate() {
        sefh.extend(id);
        sefh.extend(lengths[..=i].iter().sum::<u32>().to_le_bytes());
        sefh.extend(lengths[i].to_le_bytes());
    }
    let sefh_len = sefh.len() as u32;
    sefh.extend(sefh_len.to_le_bytes());
    sefh.extend(b"SEFT");
    let mut result = Vec::new();
    result.extend(((8 + data.len() + sefh.len()) as u32).to_be_bytes());
    result.extend(b"sefd");
    result.extend(data);
    result.extend(sefh);
    result
}
