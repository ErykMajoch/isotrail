use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use isotrail_export::{render_still, upscale, write_png};
use isotrail_render::{
    Aspect, DEFAULT_YAW_OFFSET_DEG, Frame, Gpu, Look, Mesher, Palette, SmoothMesher, VoxelMesher,
};
use tracing::info;

use crate::bundle::DataArgs;

const SCALES: [u32; 3] = [3, 5, 6];
const BACKGROUND: [u8; 4] = [28, 28, 32, 255];
const INK: [u8; 4] = [232, 232, 232, 255];
const GUTTER: u32 = 24;
/// Sheet pixels per font pixel
const LETTER_PX: u32 = 8;
const GLYPH_WIDTH: u32 = 3;
const GLYPH_HEIGHT: u32 = 5;
const BAND: u32 = GLYPH_HEIGHT * LETTER_PX + 24;

#[derive(Debug, Args)]
pub struct StyleLabArgs {
    /// GPX file or scene bundle (.itb) to render
    file: PathBuf,

    /// Directory to write the contact sheet and the individual images to
    #[arg(short, long, value_name = "DIR")]
    output: PathBuf,

    #[command(flatten)]
    data: DataArgs,
}

pub fn run(args: &StyleLabArgs) -> anyhow::Result<()> {
    let bundle = crate::render::read_scene(&args.file, &args.data)?;
    let dir = &args.output;
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;

    let meshers: [(&str, Box<dyn Mesher>); 2] = [
        ("voxel", Box::new(VoxelMesher::default())),
        ("smooth", Box::new(SmoothMesher)),
    ];
    let palettes: Vec<&str> = Palette::builtin_names().collect();
    let aspect = Aspect::Story;
    let (width, height) = aspect.output_size();
    let columns = meshers.len() * SCALES.len();
    let mut sheet = Sheet::new(columns, palettes.len(), width, height);

    let gpu = Gpu::headless()?;
    let mut index = 0;
    for palette in palettes {
        let look = Look {
            palette: Palette::builtin(palette),
            ..Look::default()
        };
        for (mesher_name, mesher) in &meshers {
            for scale in SCALES {
                info!(mesher = mesher_name, scale, palette, "rendering");
                let framing = aspect
                    .framing(scale)
                    .context("the pixel scale doesn't divide the image size")?;
                let frame = render_still(
                    &gpu,
                    &bundle,
                    mesher.as_ref(),
                    &framing,
                    DEFAULT_YAW_OFFSET_DEG,
                    &look,
                )?;
                let frame = upscale(&frame, scale)?;
                let path = dir.join(format!("{mesher_name}-{scale}-{palette}.png"));
                write_png(&path, &frame)?;
                println!("Wrote {}", path.display());
                sheet.place(
                    index,
                    &format!("{mesher_name} / scale {scale} / {palette}"),
                    &frame,
                );
                index += 1;
            }
        }
    }

    let path = dir.join("contact-sheet.png");
    write_png(&path, &sheet.into_frame())?;
    println!("Wrote {}", path.display());
    Ok(())
}

/// Equal-sized images in a grid, each under a label band
struct Sheet {
    columns: usize,
    tile_width: u32,
    tile_height: u32,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Sheet {
    fn new(columns: usize, rows: usize, tile_width: u32, tile_height: u32) -> Sheet {
        let width = GUTTER + columns as u32 * (tile_width + GUTTER);
        let height = GUTTER + rows as u32 * (BAND + tile_height + GUTTER);
        Sheet {
            columns,
            tile_width,
            tile_height,
            width,
            height,
            rgba: BACKGROUND.repeat(width as usize * height as usize),
        }
    }

    /// Puts `frame` in the `index`th cell, counting along rows
    fn place(&mut self, index: usize, label: &str, frame: &Frame) {
        assert_eq!(
            (frame.width(), frame.height()),
            (self.tile_width, self.tile_height),
            "every tile has the same size"
        );
        let x = GUTTER + (index % self.columns) as u32 * (self.tile_width + GUTTER);
        let y = GUTTER + (index / self.columns) as u32 * (BAND + self.tile_height + GUTTER);
        self.label(x, y + (BAND - GLYPH_HEIGHT * LETTER_PX) / 2, label);

        let row_bytes = self.tile_width as usize * 4;
        for (i, row) in frame.rgba().chunks_exact(row_bytes).enumerate() {
            let start = self.offset(x, y + BAND + i as u32);
            self.rgba[start..start + row_bytes].copy_from_slice(row);
        }
    }

    /// Writes `text` from (x, y), cut off at the tile's right edge
    fn label(&mut self, x: u32, y: u32, text: &str) {
        let right = x + self.tile_width;
        for (i, c) in text.chars().enumerate() {
            let bits = glyph(c);
            let left = x + i as u32 * (GLYPH_WIDTH + 1) * LETTER_PX;
            for gy in 0..GLYPH_HEIGHT {
                for gx in 0..GLYPH_WIDTH {
                    let bit = (GLYPH_HEIGHT - 1 - gy) * GLYPH_WIDTH + (GLYPH_WIDTH - 1 - gx);
                    if bits >> bit & 1 == 0 {
                        continue;
                    }
                    for py in 0..LETTER_PX {
                        for px in 0..LETTER_PX {
                            let sx = left + gx * LETTER_PX + px;
                            if sx < right {
                                let start = self.offset(sx, y + gy * LETTER_PX + py);
                                self.rgba[start..start + 4].copy_from_slice(&INK);
                            }
                        }
                    }
                }
            }
        }
    }

    fn offset(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }

    fn into_frame(self) -> Frame {
        Frame::new(self.width, self.height, self.rgba)
    }
}

/// A 3 × 5 letter, its rows top to bottom in 3-bit groups; a solid block for
/// characters it doesn't know
#[rustfmt::skip]
fn glyph(c: char) -> u16 {
    match c.to_ascii_uppercase() {
        'A' => 0b010_101_111_101_101,
        'B' => 0b110_101_110_101_110,
        'C' => 0b011_100_100_100_011,
        'D' => 0b110_101_101_101_110,
        'E' => 0b111_100_110_100_111,
        'F' => 0b111_100_110_100_100,
        'G' => 0b011_100_101_101_011,
        'H' => 0b101_101_111_101_101,
        'I' => 0b111_010_010_010_111,
        'J' => 0b001_001_001_101_010,
        'K' => 0b101_101_110_101_101,
        'L' => 0b100_100_100_100_111,
        'M' => 0b101_111_111_101_101,
        'N' => 0b110_101_101_101_101,
        'O' => 0b010_101_101_101_010,
        'P' => 0b110_101_110_100_100,
        'Q' => 0b010_101_101_110_011,
        'R' => 0b110_101_110_101_101,
        'S' => 0b011_100_010_001_110,
        'T' => 0b111_010_010_010_010,
        'U' => 0b101_101_101_101_111,
        'V' => 0b101_101_101_101_010,
        'W' => 0b101_101_111_111_101,
        'X' => 0b101_101_010_101_101,
        'Y' => 0b101_101_010_010_010,
        'Z' => 0b111_001_010_100_111,
        '0' => 0b111_101_101_101_111,
        '1' => 0b010_110_010_010_111,
        '2' => 0b110_001_010_100_111,
        '3' => 0b110_001_010_001_110,
        '4' => 0b101_101_111_001_001,
        '5' => 0b111_100_110_001_110,
        '6' => 0b011_100_111_101_111,
        '7' => 0b111_001_010_010_010,
        '8' => 0b111_101_111_101_111,
        '9' => 0b111_101_111_001_110,
        '-' => 0b000_000_111_000_000,
        '.' => 0b000_000_000_000_010,
        '/' => 0b001_001_010_100_100,
        ' ' => 0,
        _ => 0b111_111_111_111_111,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILE: (u32, u32) = (40, 20);

    fn tile(seed: u8) -> Frame {
        let (w, h) = TILE;
        let rgba = (0..w * h)
            .flat_map(|i| [seed, (i % 251) as u8, (i / 251) as u8, 255])
            .collect();
        Frame::new(w, h, rgba)
    }

    fn lit(frame: &Frame, x0: u32, y0: u32, w: u32, h: u32) -> usize {
        (y0..y0 + h)
            .flat_map(|y| (x0..x0 + w).map(move |x| (x, y)))
            .filter(|&(x, y)| frame.pixel(x, y) == INK)
            .count()
    }

    #[test]
    fn sheets_have_gutters_and_label_bands() {
        let sheet = Sheet::new(3, 2, TILE.0, TILE.1).into_frame();
        assert_eq!(sheet.width(), GUTTER + 3 * (TILE.0 + GUTTER));
        assert_eq!(sheet.height(), GUTTER + 2 * (BAND + TILE.1 + GUTTER));
        assert_eq!(sheet.pixel(0, 0), BACKGROUND);
    }

    #[test]
    fn tiles_are_copied_unchanged_along_rows() {
        let mut sheet = Sheet::new(2, 2, TILE.0, TILE.1);
        let source = tile(7);
        sheet.place(3, "", &source);
        let sheet = sheet.into_frame();
        let x0 = GUTTER + (TILE.0 + GUTTER);
        let y0 = GUTTER + (BAND + TILE.1 + GUTTER) + BAND;
        for y in 0..TILE.1 {
            for x in 0..TILE.0 {
                assert_eq!(
                    sheet.pixel(x0 + x, y0 + y),
                    source.pixel(x, y),
                    "({x}, {y})"
                );
            }
        }
        assert_eq!(sheet.pixel(x0 - 1, y0), BACKGROUND);
        assert_eq!(sheet.pixel(x0, y0 - 1), BACKGROUND);
    }

    #[test]
    fn labels_sit_in_the_band_above_their_tile() {
        let mut sheet = Sheet::new(2, 1, TILE.0, TILE.1);
        sheet.place(0, "a", &tile(1));
        sheet.place(1, "", &tile(2));
        let sheet = sheet.into_frame();
        let full = sheet.width();
        // An A is 10 font pixels out of 15
        assert_eq!(
            lit(&sheet, 0, GUTTER, full, BAND),
            10 * (LETTER_PX * LETTER_PX) as usize
        );
        assert_eq!(lit(&sheet, 0, 0, full, GUTTER), 0);
    }

    #[test]
    fn labels_stop_at_the_tile_edge() {
        let mut sheet = Sheet::new(2, 1, TILE.0, TILE.1);
        sheet.place(0, "88888888", &tile(1));
        let sheet = sheet.into_frame();
        assert!(lit(&sheet, GUTTER, GUTTER, TILE.0, BAND) > 0);
        assert_eq!(
            lit(
                &sheet,
                GUTTER + TILE.0,
                0,
                sheet.width() - GUTTER - TILE.0,
                BAND + GUTTER
            ),
            0
        );
    }

    #[test]
    fn unknown_characters_are_solid_blocks() {
        assert_eq!(glyph('é'), 0x7fff);
        assert_eq!(glyph('a'), glyph('A'));
        assert_eq!(glyph(' '), 0);
        for c in "abcdefghijklmnopqrstuvwxyz0123456789-./".chars() {
            assert_ne!(glyph(c), 0x7fff, "{c}");
            assert!(glyph(c) < 0x8000, "{c}");
        }
    }
}
