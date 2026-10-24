use isotrail_render::Frame;

use crate::ExportError;

/// Enlarges a frame by an integer factor, each pixel becoming a `scale × scale` square
pub fn upscale(frame: &Frame, scale: u32) -> Result<Frame, ExportError> {
    if scale == 0 {
        return Err(ExportError::ZeroScale);
    }
    if scale == 1 {
        return Ok(frame.clone());
    }
    let scale = scale as usize;
    let row_bytes = frame.width() as usize * 4;
    let mut rgba = Vec::with_capacity(frame.rgba().len() * scale * scale);
    for row in frame.rgba().chunks_exact(row_bytes) {
        let start = rgba.len();
        for pixel in row.as_chunks::<4>().0 {
            for _ in 0..scale {
                rgba.extend_from_slice(pixel);
            }
        }
        for _ in 1..scale {
            rgba.extend_from_within(start..start + row_bytes * scale);
        }
    }
    Ok(Frame::new(
        frame.width() * scale as u32,
        frame.height() * scale as u32,
        rgba,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker() -> Frame {
        #[rustfmt::skip]
        let rgba = vec![
            1, 2, 3, 4,      5, 6, 7, 8,      9, 10, 11, 12,
            13, 14, 15, 16,  17, 18, 19, 20,  21, 22, 23, 24,
        ];
        Frame::new(3, 2, rgba)
    }

    #[test]
    fn every_pixel_becomes_a_square() {
        let source = checker();
        let big = upscale(&source, 3).unwrap();
        assert_eq!((big.width(), big.height()), (9, 6));
        for y in 0..6 {
            for x in 0..9 {
                assert_eq!(big.pixel(x, y), source.pixel(x / 3, y / 3), "({x}, {y})");
            }
        }
    }

    #[test]
    fn scale_one_is_identity() {
        assert_eq!(upscale(&checker(), 1).unwrap(), checker());
    }

    #[test]
    fn scale_zero_is_an_error() {
        assert!(matches!(
            upscale(&checker(), 0),
            Err(ExportError::ZeroScale)
        ));
    }
}
