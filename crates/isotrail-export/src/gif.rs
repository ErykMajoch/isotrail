use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Write;
use std::num::NonZeroU16;

use gif::{DisposalMethod, Encoder, Repeat};
use isotrail_render::{Background, Frame, Look};

use crate::ExportError;

/// Browsers stretch shorter GIF frames to a tenth of a second
pub const MAX_GIF_FPS: u32 = 50;

/// How many times a GIF plays
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Plays {
    #[default]
    Forever,
    Times(NonZeroU16),
}

/// How a GIF is written: enlarged `scale` times, `fps` frames a second
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GifOptions {
    pub scale: u32,
    pub fps: u32,
    pub plays: Plays,
}

/// Streams frames into a looping GIF, enlarged `scale` times, each frame
/// keeping only what changed since the one before
pub struct GifWriter<W: Write> {
    encoder: Encoder<W>,
    width: u32,
    height: u32,
    scale: u32,
    fps: u32,
    indices: HashMap<[u8; 3], u8>,
    table: Vec<[u8; 3]>,
    transparent: u8,
    /// Frames may have see-through pixels, so each one is written whole and
    /// cleared before the next
    see_through: bool,
    previous: Vec<u8>,
    pending: Option<gif::Frame<'static>>,
    /// Frames pushed so far, and the one the pending frame started at
    pushed: u32,
    pending_from: u32,
}

/// A GIF's colour table and size, checked before anything is written
struct Setup {
    width: u32,
    height: u32,
    options: GifOptions,
    table: Vec<[u8; 3]>,
    see_through: bool,
    out: (u16, u16),
}

impl Setup {
    fn from_look(
        width: u32,
        height: u32,
        look: &Look,
        options: GifOptions,
    ) -> Result<Setup, ExportError> {
        let palette = look.palette.as_ref().ok_or(ExportError::GifPalette)?;
        let mut colours = palette.colours().to_vec();
        if let Background::Colour(colour) = look.background {
            colours.push(colour);
        }
        let see_through = look.background == Background::Transparent;
        Setup::new(width, height, &colours, see_through, options)
    }

    fn new(
        width: u32,
        height: u32,
        colours: &[[u8; 3]],
        see_through: bool,
        options: GifOptions,
    ) -> Result<Setup, ExportError> {
        let GifOptions { scale, fps, .. } = options;
        if scale == 0 {
            return Err(ExportError::ZeroScale);
        }
        if !(1..=MAX_GIF_FPS).contains(&fps) {
            return Err(ExportError::GifFps(fps));
        }
        let mut table: Vec<[u8; 3]> = Vec::new();
        for &colour in colours {
            if !table.contains(&colour) {
                table.push(colour);
            }
        }
        if table.is_empty() || table.len() > 255 {
            return Err(ExportError::GifColours(table.len()));
        }
        let size = |pixels: u32| u16::try_from(pixels * scale).ok().filter(|&s| s > 0);
        let (Some(out_width), Some(out_height)) = (size(width), size(height)) else {
            return Err(ExportError::GifSize {
                width: width * scale,
                height: height * scale,
            });
        };
        Ok(Setup {
            width,
            height,
            options,
            table,
            see_through,
            out: (out_width, out_height),
        })
    }
}

/// Checks that `look` can colour a GIF, before any frame is drawn
pub fn check_gif_look(look: &Look) -> Result<(), ExportError> {
    let options = GifOptions {
        scale: 1,
        fps: 1,
        plays: Plays::Forever,
    };
    Setup::from_look(1, 1, look, options).map(drop)
}

impl<W: Write> GifWriter<W> {
    /// Frames are `width × height` internal pixels, coloured from `look`'s
    /// palette and background
    pub fn new(
        writer: W,
        width: u32,
        height: u32,
        look: &Look,
        options: GifOptions,
    ) -> Result<Self, ExportError> {
        GifWriter::start(writer, Setup::from_look(width, height, look, options)?)
    }

    /// One more table entry stays free for see-through pixels
    pub fn with_colours(
        writer: W,
        width: u32,
        height: u32,
        colours: &[[u8; 3]],
        see_through: bool,
        options: GifOptions,
    ) -> Result<Self, ExportError> {
        let setup = Setup::new(width, height, colours, see_through, options)?;
        GifWriter::start(writer, setup)
    }

    fn start(writer: W, setup: Setup) -> Result<Self, ExportError> {
        let table = setup.table;
        let transparent = table.len() as u8;
        let indices = table
            .iter()
            .enumerate()
            .map(|(i, &c)| (c, i as u8))
            .collect();
        let mut global: Vec<u8> = table.iter().flatten().copied().collect();
        global.extend([0, 0, 0]);
        let (out_width, out_height) = setup.out;
        let mut encoder = Encoder::new(writer, out_width, out_height, &global)?;
        // Without the loop extension a GIF plays once; its count is the
        // repeats after the first play
        match setup.options.plays {
            Plays::Forever => encoder.set_repeat(Repeat::Infinite)?,
            Plays::Times(times) if times.get() > 1 => {
                encoder.set_repeat(Repeat::Finite(times.get() - 1))?;
            }
            Plays::Times(_) => {}
        }
        Ok(GifWriter {
            encoder,
            width: setup.width,
            height: setup.height,
            scale: setup.options.scale,
            fps: setup.options.fps,
            indices,
            table,
            transparent,
            see_through: setup.see_through,
            previous: Vec::new(),
            pending: None,
            pushed: 0,
            pending_from: 0,
        })
    }

    /// Adds the next frame, shown for one frame time, or longer while the
    /// frames after it look the same
    pub fn push(&mut self, frame: &Frame) -> Result<(), ExportError> {
        if (frame.width(), frame.height()) != (self.width, self.height) {
            return Err(ExportError::FrameSize {
                expected: (self.width, self.height),
                actual: (frame.width(), frame.height()),
            });
        }
        let indices: Vec<u8> = frame
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[r, g, b, a]| self.index(r, g, b, a))
            .collect();
        let first = self.previous.is_empty();
        if !first && indices == self.previous {
            self.pushed += 1;
            return Ok(());
        }
        let next = if first || self.see_through {
            self.encode(&indices, None)
        } else {
            self.encode(&indices, Some(&self.previous))
        };
        self.flush(next)?;
        self.previous = indices;
        self.pending_from = self.pushed;
        self.pushed += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Result<W, ExportError> {
        self.flush(None)?;
        Ok(self.encoder.into_inner()?)
    }

    fn index(&mut self, r: u8, g: u8, b: u8, a: u8) -> u8 {
        if self.see_through && a < 128 {
            return self.transparent;
        }
        let rgb = [r, g, b];
        if let Some(&index) = self.indices.get(&rgb) {
            return index;
        }
        let distance = |c: &[u8; 3]| {
            c.iter()
                .zip(rgb)
                .map(|(&a, b)| (i32::from(a) - i32::from(b)).pow(2))
                .sum::<i32>()
        };
        let nearest = (0..self.table.len())
            .min_by_key(|&i| distance(&self.table[i]))
            .unwrap_or(0) as u8;
        self.indices.insert(rgb, nearest);
        nearest
    }

    /// The part of the frame that differs from `previous`, if given, with
    /// the pixels that don't left see-through, enlarged
    fn encode(&self, indices: &[u8], previous: Option<&[u8]>) -> Option<gif::Frame<'static>> {
        let (width, height) = (self.width as usize, self.height as usize);
        let (mut min, mut max) = ((width, height), (0, 0));
        match previous {
            Some(previous) => {
                for (i, (a, b)) in indices.iter().zip(previous).enumerate() {
                    if a != b {
                        let (x, y) = (i % width, i / width);
                        min = (min.0.min(x), min.1.min(y));
                        max = (max.0.max(x + 1), max.1.max(y + 1));
                    }
                }
            }
            None => (min, max) = ((0, 0), (width, height)),
        }
        if min.0 >= max.0 {
            return None;
        }
        let scale = self.scale as usize;
        let row_len = (max.0 - min.0) * scale;
        let mut buffer = Vec::with_capacity(row_len * (max.1 - min.1) * scale);
        for y in min.1..max.1 {
            let start = buffer.len();
            for x in min.0..max.0 {
                let i = y * width + x;
                let index = match previous {
                    Some(previous) if previous[i] == indices[i] => self.transparent,
                    _ => indices[i],
                };
                buffer.extend(std::iter::repeat_n(index, scale));
            }
            for _ in 1..scale {
                buffer.extend_from_within(start..start + row_len);
            }
        }
        let see_through = previous.is_some() || self.see_through;
        Some(gif::Frame {
            left: (min.0 * scale) as u16,
            top: (min.1 * scale) as u16,
            width: row_len as u16,
            height: ((max.1 - min.1) * scale) as u16,
            dispose: if self.see_through {
                DisposalMethod::Background
            } else {
                DisposalMethod::Keep
            },
            transparent: see_through.then_some(self.transparent),
            buffer: Cow::Owned(buffer),
            ..gif::Frame::default()
        })
    }

    /// Writes the pending frame, now that its length is known, and holds `next`
    fn flush(&mut self, next: Option<gif::Frame<'static>>) -> Result<(), ExportError> {
        if let Some(mut frame) = self.pending.take() {
            let cs = |frame: u32| (f64::from(frame) * 100.0 / f64::from(self.fps)).round() as u32;
            let delay = cs(self.pushed) - cs(self.pending_from);
            frame.delay = delay.min(u32::from(u16::MAX)) as u16;
            self.encoder.write_frame(&frame)?;
        }
        self.pending = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use isotrail_render::Palette;
    use isotrail_testkit::{decode_gif, gif_loop_count};

    use super::*;

    const RED: [u8; 4] = [200, 30, 40, 255];
    const BLUE: [u8; 4] = [20, 40, 220, 255];
    const CLEAR: [u8; 4] = [0, 0, 0, 0];

    fn frame(pixels: &[[u8; 4]]) -> Frame {
        Frame::new(2, 2, pixels.concat())
    }

    fn options(scale: u32, fps: u32) -> GifOptions {
        GifOptions {
            scale,
            fps,
            plays: Plays::Forever,
        }
    }

    fn writer(see_through: bool, fps: u32) -> GifWriter<Vec<u8>> {
        let colours = [[200, 30, 40], [20, 40, 220]];
        GifWriter::with_colours(Vec::new(), 2, 2, &colours, see_through, options(3, fps)).unwrap()
    }

    #[test]
    fn gifs_loop_forever_or_play_a_number_of_times() {
        let colours = [[200, 30, 40]];
        let written = |plays| {
            let options = GifOptions {
                plays,
                ..options(1, 10)
            };
            let mut gif =
                GifWriter::with_colours(Vec::new(), 2, 2, &colours, false, options).unwrap();
            gif.push(&frame(&[RED; 4])).unwrap();
            gif.finish().unwrap()
        };
        let times = |n| Plays::Times(NonZeroU16::new(n).unwrap());
        assert_eq!(gif_loop_count(&written(Plays::Forever)), Some(0));
        assert_eq!(gif_loop_count(&written(times(1))), None);
        assert_eq!(gif_loop_count(&written(times(3))), Some(2));
        assert_eq!(decode_gif(&written(times(1))).len(), 1);
    }

    #[test]
    fn frames_round_trip_enlarged() {
        let frames = [
            frame(&[RED, RED, BLUE, RED]),
            frame(&[RED, BLUE, BLUE, RED]),
            frame(&[BLUE, BLUE, BLUE, BLUE]),
        ];
        let mut gif = writer(false, 10);
        for frame in &frames {
            gif.push(frame).unwrap();
        }
        let decoded = decode_gif(&gif.finish().unwrap());
        assert_eq!(decoded.len(), 3);
        for (frame, decoded) in frames.iter().zip(&decoded) {
            assert_eq!((decoded.width, decoded.height), (6, 6));
            assert_eq!(decoded.delay_cs, 10);
            for y in 0..6 {
                for x in 0..6 {
                    let i = (y * 6 + x) * 4;
                    assert_eq!(
                        &decoded.rgba[i..i + 4],
                        frame.pixel(x as u32 / 3, y as u32 / 3)
                    );
                }
            }
        }
    }

    #[test]
    fn only_the_changed_part_is_written() {
        let mut gif = writer(false, 10);
        gif.push(&frame(&[RED, RED, RED, RED])).unwrap();
        let first = gif.pending.clone().unwrap();
        assert_eq!(
            (first.left, first.top, first.width, first.height),
            (0, 0, 6, 6)
        );
        gif.push(&frame(&[RED, RED, RED, BLUE])).unwrap();
        let second = gif.pending.clone().unwrap();
        assert_eq!((second.left, second.top), (3, 3));
        assert_eq!((second.width, second.height), (3, 3));
        assert_eq!(second.transparent, Some(2));
        assert_eq!(second.dispose, DisposalMethod::Keep);
    }

    #[test]
    fn repeated_frames_lengthen_the_one_before() {
        let mut gif = writer(false, 30);
        let still = frame(&[RED, BLUE, RED, BLUE]);
        for _ in 0..4 {
            gif.push(&still).unwrap();
        }
        gif.push(&frame(&[BLUE; 4])).unwrap();
        gif.push(&frame(&[RED; 4])).unwrap();
        let decoded = decode_gif(&gif.finish().unwrap());
        // 4 frames of 3⅓ cs, then two more: 13, 4 and 3 cs add up to 20
        let delays: Vec<u16> = decoded.iter().map(|f| f.delay_cs).collect();
        assert_eq!(delays, [13, 4, 3]);
    }

    #[test]
    fn transparent_frames_are_whole_and_cleared() {
        let mut gif = writer(true, 10);
        gif.push(&frame(&[RED, CLEAR, CLEAR, CLEAR])).unwrap();
        gif.push(&frame(&[CLEAR, CLEAR, CLEAR, BLUE])).unwrap();
        let pending = gif.pending.clone().unwrap();
        assert_eq!((pending.width, pending.height), (6, 6));
        assert_eq!(pending.dispose, DisposalMethod::Background);
        let decoded = decode_gif(&gif.finish().unwrap());
        let last = &decoded[1].rgba;
        assert_eq!(&last[..4], CLEAR);
        assert_eq!(&last[last.len() - 4..], BLUE);
    }

    #[test]
    fn colours_outside_the_table_snap_to_the_nearest() {
        let mut gif = writer(false, 10);
        gif.push(&frame(&[[190, 40, 50, 255], BLUE, BLUE, BLUE]))
            .unwrap();
        let decoded = decode_gif(&gif.finish().unwrap());
        assert_eq!(&decoded[0].rgba[..4], RED);
    }

    #[test]
    fn bad_settings_are_errors() {
        let colours: Vec<[u8; 3]> = (0..=255).map(|i| [i as u8, 0, 1]).collect();
        let error = |colours: &[[u8; 3]], scale, fps| {
            GifWriter::with_colours(Vec::new(), 10, 10, colours, false, options(scale, fps))
                .err()
                .unwrap()
        };
        assert!(matches!(
            error(&colours, 1, 30),
            ExportError::GifColours(256)
        ));
        assert!(matches!(error(&[], 1, 30), ExportError::GifColours(0)));
        assert!(matches!(
            error(&colours[..2], 1, 60),
            ExportError::GifFps(60)
        ));
        assert!(matches!(
            error(&colours[..2], 0, 30),
            ExportError::ZeroScale
        ));
        let huge =
            GifWriter::with_colours(Vec::new(), 7000, 10, &colours[..2], false, options(10, 30));
        assert!(matches!(huge.err().unwrap(), ExportError::GifSize { .. }));
        let look = Look {
            palette: None,
            ..Look::default()
        };
        let none = GifWriter::new(Vec::new(), 10, 10, &look, options(1, 30));
        assert!(matches!(none.err().unwrap(), ExportError::GifPalette));
        assert!(matches!(
            check_gif_look(&look),
            Err(ExportError::GifPalette)
        ));
        assert!(check_gif_look(&Look::default()).is_ok());
        let hex: String = colours
            .iter()
            .map(|[r, g, b]| format!("{r:02x}{g:02x}{b:02x}\n"))
            .collect();
        let look = Look {
            palette: Some(Palette::parse_hex(&hex).unwrap()),
            ..Look::default()
        };
        assert!(matches!(
            check_gif_look(&look),
            Err(ExportError::GifColours(256))
        ));
        let mut gif = writer(false, 10);
        let wrong = gif.push(&Frame::new(1, 1, vec![0; 4]));
        assert!(matches!(wrong, Err(ExportError::FrameSize { .. })));
    }
}
