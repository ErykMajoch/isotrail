use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use isotrail_render::Frame;

use crate::ExportError;

pub fn encode_png(frame: &Frame, writer: impl Write) -> Result<(), ExportError> {
    let mut encoder = png::Encoder::new(writer, frame.width(), frame.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(frame.rgba())?;
    writer.finish()?;
    Ok(())
}

pub fn write_png(path: &Path, frame: &Frame) -> Result<(), ExportError> {
    let write = || -> Result<(), ExportError> {
        let mut file = BufWriter::new(File::create(path)?);
        encode_png(frame, &mut file)?;
        file.flush()?;
        Ok(())
    };
    write().map_err(|source| ExportError::Write {
        path: path.to_owned(),
        source: Box::new(source),
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn png_round_trips_rgba() {
        let rgba: Vec<u8> = (0..2 * 3 * 4).map(|i| i as u8 * 10).collect();
        let frame = Frame::new(2, 3, rgba.clone());
        let mut bytes = Vec::new();
        encode_png(&frame, &mut bytes).unwrap();

        let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
        let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut decoded).unwrap();
        assert_eq!((info.width, info.height), (2, 3));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(&decoded[..info.buffer_size()], rgba.as_slice());
    }
}
