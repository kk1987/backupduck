//! Create a date-enriched delivery copy without modifying the original image.
fn main() -> backupduck_core::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err(backupduck_core::Error::Invalid(
            "usage: photo-date SOURCE OUTPUT 'YYYY:MM:DD HH:MM:SS' (UTC)".into(),
        ));
    }
    backupduck_pixel::write_photo_date(args[1].as_ref(), args[2].as_ref(), &args[3], 0)?;
    Ok(())
}
