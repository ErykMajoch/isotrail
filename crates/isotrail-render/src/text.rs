use std::collections::HashMap;
use std::sync::OnceLock;

use glam::IVec2;

use crate::Frame;

const GRID: &[u8] = include_bytes!("../../../assets/fonts/monogram.png");
const CHARS: &str = include_str!("../../../assets/fonts/monogram.txt");
const CELL_WIDTH: usize = 8;
const CELL_HEIGHT: usize = 13;
/// Glyph column 0 sits one pixel left of the pen, so most glyphs start on it
const INK_OFFSET: i32 = 1;
const ELLIPSIS: char = '…';

/// What surrounds each glyph so text reads over a busy background
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEffect {
    None,
    /// One pixel down and to the right
    Shadow([u8; 3]),
    /// One pixel all round
    Outline([u8; 3]),
}

/// A monospaced bitmap font drawn straight onto the pixel grid, at whole
/// multiples of its size
pub struct PixelFont {
    /// Row bits, bit `x` being column `x`
    glyphs: HashMap<char, [u8; CELL_HEIGHT]>,
}

impl PixelFont {
    /// Pen movement per character at scale 1
    pub const ADVANCE: u32 = 6;
    /// Rows from one line to the next at scale 1
    pub const LINE_HEIGHT: u32 = 12;
    /// Rows above the capitals, where their accents go
    pub const CAP_TOP: u32 = 4;
    /// Rows from the top of a line to the bottom of its descenders
    pub const HEIGHT: u32 = CELL_HEIGHT as u32;

    /// monogram, by Vinícius Menézio (CC0), with ASCII, Latin-1 and
    /// Latin Extended-A
    pub fn monogram() -> &'static PixelFont {
        static FONT: OnceLock<PixelFont> = OnceLock::new();
        FONT.get_or_init(|| PixelFont::from_grid(GRID, CHARS))
    }

    /// `chars` lists the glyphs of each grid row on a line of its own
    fn from_grid(png: &[u8], chars: &str) -> PixelFont {
        let (width, height, rgba) = crate::sprite::decode(png);
        let columns = width as usize / CELL_WIDTH;
        let mut glyphs = HashMap::new();
        for (row, line) in chars.lines().enumerate() {
            for (column, c) in line.chars().enumerate() {
                assert!(column < columns, "{c:?} lies outside the font grid");
                let mut rows = [0u8; CELL_HEIGHT];
                for (y, bits) in rows.iter_mut().enumerate() {
                    let py = row * CELL_HEIGHT + y;
                    assert!(py < height as usize, "{c:?} lies outside the font grid");
                    for x in 0..CELL_WIDTH {
                        let px = column * CELL_WIDTH + x;
                        if rgba[(py * width as usize + px) * 4 + 3] >= 128 {
                            *bits |= 1 << x;
                        }
                    }
                }
                glyphs.insert(c, rows);
            }
        }
        PixelFont { glyphs }
    }

    pub fn has_glyph(&self, c: char) -> bool {
        self.glyphs.contains_key(&c)
    }

    /// The characters that will be drawn: whitespace becomes a space,
    /// invisible characters go, look-alikes stand in for missing glyphs and
    /// anything else becomes `?`
    pub fn normalise(&self, text: &str) -> String {
        text.chars()
            .filter_map(|c| {
                if c.is_whitespace() {
                    return Some(' ');
                }
                if c.is_control() || matches!(c, '\u{ad}' | '\u{200b}'..='\u{200f}' | '\u{feff}') {
                    return None;
                }
                if self.has_glyph(c) {
                    return Some(c);
                }
                Some(match c {
                    '‐' | '‑' | '‒' | '–' | '−' => '-',
                    '‘' | '‚' | '′' => '\'',
                    '“' | '”' | '„' | '″' => '"',
                    'ſ' => 's',
                    '•' => '·',
                    _ => '?',
                })
            })
            .collect()
    }

    /// Width in pixels from the first glyph's ink to the last's
    pub fn width(&self, text: &str, scale: u32) -> u32 {
        let count = self.normalise(text).chars().count() as u32;
        (count * Self::ADVANCE).saturating_sub(1) * scale
    }

    /// Breaks `text` into lines no wider than `max_width` at spaces, or
    /// inside words that are too long alone, and ends the last line with an
    /// ellipsis if more than `max_lines` would be needed
    pub fn wrap(&self, text: &str, scale: u32, max_width: u32, max_lines: usize) -> Vec<String> {
        let per_line = ((max_width / scale + 1) / Self::ADVANCE).max(1) as usize;
        let text = self.normalise(text);
        let mut lines: Vec<String> = Vec::new();
        let mut line = String::new();
        for word in text.split(' ').filter(|w| !w.is_empty()) {
            let mut word: Vec<char> = word.chars().collect();
            let used = line.chars().count();
            if used > 0 && used + 1 + word.len() <= per_line {
                line.push(' ');
                line.extend(&word);
                continue;
            }
            if used > 0 {
                lines.push(std::mem::take(&mut line));
            }
            while word.len() > per_line {
                lines.push(word.drain(..per_line).collect());
            }
            line.extend(word);
        }
        if !line.is_empty() {
            lines.push(line);
        }
        if lines.len() > max_lines && max_lines > 0 {
            let rest = lines.split_off(max_lines - 1).join(" ");
            lines.push(with_ellipsis(&rest, per_line));
        }
        lines
    }

    /// First and last rows of the cell that `text` puts ink on
    pub fn ink_rows(&self, text: &str) -> Option<(u32, u32)> {
        let mut used = 0u16;
        for c in self.normalise(text).chars() {
            for (y, bits) in self.glyph(c).iter().enumerate() {
                if *bits != 0 {
                    used |= 1 << y;
                }
            }
        }
        (used != 0).then(|| (used.trailing_zeros(), 15 - used.leading_zeros()))
    }

    /// Draws one line with its pen starting at `origin`, the top of the
    /// glyph cell
    pub fn draw(
        &self,
        frame: &mut Frame,
        text: &str,
        origin: IVec2,
        scale: u32,
        colour: [u8; 3],
        effect: TextEffect,
    ) {
        let scale = scale.max(1) as i32;
        let mut ink = Vec::new();
        for (i, c) in self.normalise(text).chars().enumerate() {
            let pen = origin.x + i as i32 * Self::ADVANCE as i32 * scale;
            for (y, bits) in self.glyph(c).iter().enumerate() {
                for x in 0..CELL_WIDTH {
                    if bits >> x & 1 == 1 {
                        let px = pen + (x as i32 - INK_OFFSET) * scale;
                        let py = origin.y + y as i32 * scale;
                        ink.push(IVec2::new(px, py));
                    }
                }
            }
        }
        let block = |frame: &mut Frame, at: IVec2, rgb: [u8; 3]| {
            let [r, g, b] = rgb;
            for dy in 0..scale {
                for dx in 0..scale {
                    frame.set_pixel(at.x + dx, at.y + dy, [r, g, b, 255]);
                }
            }
        };
        match effect {
            TextEffect::None => {}
            TextEffect::Shadow(shade) => {
                for &at in &ink {
                    block(frame, at + IVec2::ONE, shade);
                }
            }
            TextEffect::Outline(shade) => {
                for &at in &ink {
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            block(frame, at + IVec2::new(dx, dy), shade);
                        }
                    }
                }
            }
        }
        for &at in &ink {
            block(frame, at, colour);
        }
    }

    fn glyph(&self, c: char) -> &[u8; CELL_HEIGHT] {
        self.glyphs
            .get(&c)
            .or_else(|| self.glyphs.get(&'?'))
            .expect("the font has a question mark")
    }
}

/// `line` cut back far enough for an ellipsis to fit in `per_line`
/// characters, without a dangling space or separator before it
fn with_ellipsis(line: &str, per_line: usize) -> String {
    let mut chars: Vec<char> = line.chars().collect();
    chars.truncate(per_line.saturating_sub(1));
    while chars
        .last()
        .is_some_and(|c| matches!(c, ' ' | '-' | '|' | ',' | '·' | ':' | ';'))
    {
        chars.pop();
    }
    chars.push(ELLIPSIS);
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> &'static PixelFont {
        PixelFont::monogram()
    }

    #[test]
    fn covers_ascii_latin_1_and_latin_extended_a() {
        let missing: String = (0x20..0x7f_u32)
            .chain(0xa1..0x100)
            .chain(0x100..0x180)
            .filter_map(char::from_u32)
            .filter(|&c| c != '\u{ad}' && c != 'ſ' && !font().has_glyph(c))
            .collect();
        assert_eq!(missing, "");
        for c in "łńąśżŁŃĄŚŻćęóźĆĘÓŹ|·…—".chars() {
            assert!(font().has_glyph(c), "{c:?}");
        }
    }

    #[test]
    fn glyphs_have_ink_except_spaces() {
        assert_eq!(font().ink_rows(" "), None);
        // Capitals span rows 4–10, descenders reach 12 and accents sit above
        assert_eq!(font().ink_rows("H"), Some((4, 10)));
        assert_eq!(font().ink_rows("Hg"), Some((4, 12)));
        assert_eq!(font().ink_rows("Ó"), Some((1, 10)));
        for c in font().glyphs.keys().filter(|&&c| c != ' ') {
            assert!(font().ink_rows(&c.to_string()).is_some(), "{c:?}");
        }
    }

    #[test]
    fn normalising_keeps_drawable_text() {
        assert_eq!(
            font().normalise("Giewont\t|\u{a0}Łańcuch"),
            "Giewont | Łańcuch"
        );
        assert_eq!(font().normalise("so\u{ad}ft\u{200b}"), "soft");
        assert_eq!(font().normalise("A–B “C” ſ 山"), "A-B \"C\" s ?");
    }

    #[test]
    fn width_runs_from_first_ink_to_last() {
        assert_eq!(font().width("", 1), 0);
        assert_eq!(font().width("A", 1), 5);
        assert_eq!(font().width("Giewont", 1), 41);
        assert_eq!(font().width("Giewont", 2), 82);
    }

    #[test]
    fn wrapping_breaks_at_spaces_within_the_width() {
        let lines = font().wrap("Przełęcz pod Chłopkiem i Mięguszowiecki", 1, 83, 9);
        assert_eq!(lines, ["Przełęcz pod", "Chłopkiem i", "Mięguszowiecki"]);
        for line in &lines[..2] {
            assert!(font().width(line, 1) <= 83, "{line}");
        }
        assert_eq!(font().wrap("  Two   spaces ", 1, 200, 2), ["Two spaces"]);
        assert!(font().wrap(" ", 1, 200, 2).is_empty());
    }

    #[test]
    fn long_words_break_and_extra_lines_end_in_an_ellipsis() {
        assert_eq!(
            font().wrap("Wierchporoniec", 1, 35, 9),
            ["Wierch", "poroni", "ec"]
        );
        let lines = font().wrap("Dolina Pięciu Stawów Polskich | Kozi Wierch", 2, 166, 2);
        assert_eq!(lines, ["Dolina Pięciu", "Stawów Polski…"]);
        let lines = font().wrap("Over the edge - and back", 1, 95, 1);
        assert_eq!(lines, ["Over the edge…"]);
        assert!(font().width(&lines[0], 1) <= 95);
    }

    #[test]
    fn draws_scaled_glyphs_with_their_effect_underneath() {
        let mut frame = Frame::new(16, 30, vec![0; 16 * 30 * 4]);
        let white = [255, 255, 255];
        let grey = [80, 80, 80];
        font().draw(
            &mut frame,
            "I",
            IVec2::new(2, 1),
            2,
            white,
            TextEffect::Shadow(grey),
        );
        let lit: Vec<(u32, u32)> = (0..30)
            .flat_map(|y| (0..16).map(move |x| (x, y)))
            .filter(|&(x, y)| frame.pixel(x, y) == [255, 255, 255, 255])
            .collect();
        // Capital I is five pixels wide and seven tall, doubled
        let (min_y, max_y) = (lit[0].1, lit.last().unwrap().1);
        assert_eq!((min_y, max_y), (1 + 4 * 2, 1 + 11 * 2 - 1));
        assert_eq!(lit.iter().map(|p| p.0).min(), Some(2));
        assert_eq!(lit.iter().map(|p| p.0).max(), Some(2 + 5 * 2 - 1));
        assert_eq!(frame.pixel(2 + 1, max_y + 1), [80, 80, 80, 255]);
        assert_eq!(frame.pixel(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn outlines_surround_every_pixel() {
        let mut frame = Frame::new(12, 16, vec![0; 12 * 16 * 4]);
        font().draw(
            &mut frame,
            ".",
            IVec2::new(3, 0),
            1,
            [255, 255, 255],
            TextEffect::Outline([1, 2, 3]),
        );
        let ink: Vec<(i32, i32)> = (0..16)
            .flat_map(|y| (0..12).map(move |x| (x, y)))
            .filter(|&(x, y)| frame.pixel(x as u32, y as u32)[0] == 255)
            .collect();
        assert!(!ink.is_empty());
        for (x, y) in ink {
            for (dx, dy) in [(-1, -1), (0, -1), (1, 0), (0, 1), (-1, 1)] {
                let p = frame.pixel((x + dx) as u32, (y + dy) as u32);
                assert!(p == [255, 255, 255, 255] || p == [1, 2, 3, 255]);
            }
        }
    }
}
