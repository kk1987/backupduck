//! Add Motion Photo metadata to an ordinary JPEG's existing XMP packet.
//! Keep all unrelated XML and the JPEG image bytes unchanged.
use photobridge_core::{Error, Result};
use quick_xml::{events::Event, name::ResolveResult, reader::NsReader};

const RDF: &[u8] = b"http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const CAMERA: &[u8] = b"http://ns.google.com/photos/1.0/camera/";
const CONTAINER: &[u8] = b"http://ns.google.com/photos/1.0/container/";

fn unsupported() -> Error {
    Error::Unsupported("preexisting XMP packet".into())
}

pub(super) fn merge(packet: &[u8], description: &[u8]) -> Result<Vec<u8>> {
    // Camera XMP commonly has an xpacket trailer followed by a NUL byte.
    let xml_len = packet.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
    let xml = &packet[..xml_len];
    let mut reader = NsReader::from_reader(xml);
    let mut insertion = None;
    let mut rdf_count = 0;
    let mut depth = 0usize;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|_| unsupported())?;
        let end = reader.buffer_position() as usize;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
                if depth > 128 {
                    return Err(unsupported());
                }
                let (ns, name) = reader.resolver().resolve_element(element.name());
                if matches!(ns, ResolveResult::Bound(n) if n.as_ref() == CONTAINER)
                    && name.as_ref() == b"Directory"
                {
                    return Err(unsupported());
                }
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|_| unsupported())?;
                    let (ns, name) = reader.resolver().resolve_attribute(attribute.key);
                    if matches!(ns, ResolveResult::Bound(n) if n.as_ref() == CAMERA)
                        && matches!(
                            name.as_ref(),
                            b"MotionPhoto"
                                | b"MotionPhotoVersion"
                                | b"MicroVideo"
                                | b"MicroVideoOffset"
                        )
                    {
                        return Err(unsupported());
                    }
                }
                let (ns, name) = reader.resolver().resolve_element(element.name());
                if matches!(ns, ResolveResult::Bound(n) if n.as_ref() == CAMERA)
                    && matches!(
                        name.as_ref(),
                        b"MotionPhoto"
                            | b"MotionPhotoVersion"
                            | b"MicroVideo"
                            | b"MicroVideoOffset"
                    )
                {
                    return Err(unsupported());
                }
                if matches!(ns, ResolveResult::Bound(n) if n.as_ref() == RDF)
                    && name.as_ref() == b"RDF"
                {
                    rdf_count += 1;
                    if matches!(event, Event::Empty(_)) {
                        let mut replacement = xml[start..end - 2].to_vec();
                        replacement.push(b'>');
                        replacement.extend(description);
                        replacement.extend(b"</");
                        replacement.extend(element.name().as_ref());
                        replacement.push(b'>');
                        insertion = Some((start, end, replacement));
                    }
                }
            }
            Event::End(element) => {
                depth = depth.checked_sub(1).ok_or_else(unsupported)?;
                let (ns, name) = reader.resolver().resolve_element(element.name());
                if matches!(ns, ResolveResult::Bound(n) if n.as_ref() == RDF)
                    && name.as_ref() == b"RDF"
                {
                    insertion = Some((start, start, description.to_vec()));
                }
            }
            Event::DocType(_) => return Err(unsupported()),
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 || rdf_count != 1 {
        return Err(unsupported());
    }
    let (start, end, replacement) = insertion.ok_or_else(unsupported)?;
    let mut output = Vec::with_capacity(packet.len() + replacement.len());
    output.extend(&xml[..start]);
    output.extend(replacement);
    output.extend(&xml[end..]);
    output.extend(&packet[xml_len..]);
    Ok(output)
}
