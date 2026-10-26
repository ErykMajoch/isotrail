/// One GIF frame as it shows on screen, after the frames before it
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GifFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub delay_cs: u16,
}

/// Decodes every frame of a GIF and lays each over what the ones before it
/// left, as a browser shows them
///
/// # Panics
///
/// If `bytes` isn't a valid GIF
pub fn decode_gif(bytes: &[u8]) -> Vec<GifFrame> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(bytes).expect("a GIF header");
    let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
    let mut canvas = vec![0u8; width as usize * height as usize * 4];
    let mut frames = Vec::new();
    while let Some(frame) = decoder.read_next_frame().expect("a GIF frame") {
        let (left, top) = (usize::from(frame.left), usize::from(frame.top));
        let rect_width = usize::from(frame.width);
        let pixel = |x: usize, y: usize| ((top + y) * width as usize + left + x) * 4;
        for (i, rgba) in frame.buffer.as_chunks::<4>().0.iter().enumerate() {
            if rgba[3] > 0 {
                let at = pixel(i % rect_width, i / rect_width);
                canvas[at..at + 4].copy_from_slice(rgba);
            }
        }
        frames.push(GifFrame {
            width,
            height,
            rgba: canvas.clone(),
            delay_cs: frame.delay,
        });
        if frame.dispose == gif::DisposalMethod::Background {
            for y in 0..usize::from(frame.height) {
                let at = pixel(0, y);
                canvas[at..at + rect_width * 4].fill(0);
            }
        }
    }
    frames
}
