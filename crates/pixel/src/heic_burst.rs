//! Lossless HEIC annotation for derived burst delivery copies. Only the
//! primary image's XMP item changes; no Motion Photo video or footer is added.
use crate::{
    burst::burst_attributes,
    heic_motion::{rewrite_heic_primary_xmp, MergePolicy},
};
use backupduck_core::{BurstMetadata, Error, Result};
use std::{fs, fs::OpenOptions, io::Write, path::Path};

pub fn write_heic_burst(source: &Path, output: &Path, burst: &BurstMetadata) -> Result<()> {
    burst.validate()?;
    if source == output || (output.exists() && source.canonicalize()? == output.canonicalize()?) {
        return Err(Error::Invalid("burst delivery destination".into()));
    }
    if source.metadata()?.len() > 128 << 20 {
        return Err(Error::Unsupported("HEIC burst size".into()));
    }
    let still = fs::read(source)?;
    let description = format!(
        r#"<rdf:Description rdf:about="" xmlns:GCamera="http://ns.google.com/photos/1.0/camera/"{}/>"#,
        burst_attributes(burst)
    );
    let image = rewrite_heic_primary_xmp(&still, &description, MergePolicy::Burst(burst))?;
    let partial = output.with_extension("burst.partial");
    // Never remove another operation's partial on a create_new failure.
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    let result = (|| -> Result<()> {
        out.write_all(&image)?;
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
