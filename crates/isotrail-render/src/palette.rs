use glam::Vec3;

/// The most colours a palette can hold, as the quantise shader's table is fixed
pub const MAX_COLOURS: usize = 256;
/// Used when no palette is asked for, until a style is settled
pub const DEFAULT_PALETTE: &str = "apollo";

const BUILTIN: [(&str, &str); 3] = [
    (
        "apollo",
        include_str!("../../../assets/palettes/apollo.hex"),
    ),
    (
        "resurrect-64",
        include_str!("../../../assets/palettes/resurrect-64.hex"),
    ),
    (
        "aap-64",
        include_str!("../../../assets/palettes/aap-64.hex"),
    ),
];

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PaletteError {
    #[error("line {line}: expected a colour as RRGGBB, found {text:?}")]
    BadLine { line: usize, text: String },

    #[error("the palette has no colours")]
    Empty,

    #[error("the palette has {0} colours, more than the {MAX_COLOURS} allowed")]
    TooMany(usize),
}

/// A fixed set of sRGB colours that frames are quantised to
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    colours: Vec<[u8; 3]>,
}

impl Palette {
    /// Reads a Lospec `.hex` file: one `RRGGBB` per line, `#` optional
    ///
    /// ```
    /// let palette = isotrail_render::Palette::parse_hex("#1a1c2c\n5d275d\n").unwrap();
    /// assert_eq!(palette.colours(), [[0x1a, 0x1c, 0x2c], [0x5d, 0x27, 0x5d]]);
    /// ```
    pub fn parse_hex(text: &str) -> Result<Palette, PaletteError> {
        let mut colours = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let hex = line.trim();
            if hex.is_empty() {
                continue;
            }
            let digits = hex.strip_prefix('#').unwrap_or(hex);
            let bad = || PaletteError::BadLine {
                line: i + 1,
                text: hex.to_owned(),
            };
            if digits.len() != 6 || !digits.is_ascii() {
                return Err(bad());
            }
            let channel =
                |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).map_err(|_| bad());
            colours.push([channel(0)?, channel(2)?, channel(4)?]);
        }
        match colours.len() {
            0 => Err(PaletteError::Empty),
            n if n > MAX_COLOURS => Err(PaletteError::TooMany(n)),
            _ => Ok(Palette { colours }),
        }
    }

    /// One of the palettes shipped with isotrail, by name, ignoring case
    pub fn builtin(name: &str) -> Option<Palette> {
        BUILTIN
            .iter()
            .find(|(builtin, _)| builtin.eq_ignore_ascii_case(name))
            .map(|(_, text)| Palette::parse_hex(text).expect("shipped palettes parse"))
    }

    pub fn builtin_names() -> impl Iterator<Item = &'static str> {
        BUILTIN.iter().map(|(name, _)| *name)
    }

    pub fn colours(&self) -> &[[u8; 3]] {
        &self.colours
    }

    /// The perceptually closest colour, by distance in OKLab
    pub fn nearest(&self, rgb: [u8; 3]) -> [u8; 3] {
        let target = srgb_to_oklab(rgb);
        *self
            .colours
            .iter()
            .min_by(|a, b| {
                let da = srgb_to_oklab(**a).distance_squared(target);
                let db = srgb_to_oklab(**b).distance_squared(target);
                da.total_cmp(&db)
            })
            .expect("palettes are never empty")
    }
}

/// OKLab (L, a, b) of an 8-bit sRGB colour
pub fn srgb_to_oklab(rgb: [u8; 3]) -> Vec3 {
    let linear = Vec3::from(rgb.map(|c| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }));
    let lms = Vec3::new(
        linear.dot(Vec3::new(0.412_221_46, 0.536_332_55, 0.051_445_995)),
        linear.dot(Vec3::new(0.211_903_5, 0.680_699_5, 0.107_396_96)),
        linear.dot(Vec3::new(0.088_302_46, 0.281_718_85, 0.629_978_7)),
    )
    .map(f32::cbrt);
    Vec3::new(
        lms.dot(Vec3::new(0.210_454_26, 0.793_617_8, -0.004_072_047)),
        lms.dot(Vec3::new(1.977_998_5, -2.428_592_2, 0.450_593_7)),
        lms.dot(Vec3::new(0.025_904_037, 0.782_771_77, -0.808_675_77)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_files_allow_hashes_blank_lines_and_crlf() {
        let palette = Palette::parse_hex("#FF0040\r\n\r\n  131313 \r\n").unwrap();
        assert_eq!(palette.colours(), [[255, 0, 64], [19, 19, 19]]);
    }

    #[test]
    fn bad_lines_are_reported_by_number() {
        for (text, line, bad) in [
            ("ffffff\n12345\n", 2, "12345"),
            ("ffffff\n\nggffff\n", 3, "ggffff"),
            ("ff ff ff\n", 1, "ff ff ff"),
            ("#ffffff00\n", 1, "#ffffff00"),
            ("ffé0f\n", 1, "ffé0f"),
        ] {
            assert_eq!(
                Palette::parse_hex(text),
                Err(PaletteError::BadLine {
                    line,
                    text: bad.into()
                }),
                "{text:?}"
            );
        }
    }

    #[test]
    fn palettes_hold_one_to_256_colours() {
        assert_eq!(Palette::parse_hex("\n \n"), Err(PaletteError::Empty));
        let full = "000000\n".repeat(MAX_COLOURS);
        assert_eq!(Palette::parse_hex(&full).unwrap().colours().len(), 256);
        let over = full + "000000\n";
        assert_eq!(Palette::parse_hex(&over), Err(PaletteError::TooMany(257)));
    }

    #[test]
    fn oklab_matches_reference_values() {
        let close = |a: Vec3, b: Vec3| (a - b).abs().max_element() < 1e-3;
        assert!(close(srgb_to_oklab([255, 255, 255]), Vec3::X));
        assert!(close(srgb_to_oklab([0, 0, 0]), Vec3::ZERO));
        let red = srgb_to_oklab([255, 0, 0]);
        assert!(close(red, Vec3::new(0.628, 0.2249, 0.1258)), "{red}");
        let blue = srgb_to_oklab([0, 0, 255]);
        assert!(close(blue, Vec3::new(0.452, -0.0325, -0.3115)), "{blue}");
    }

    #[test]
    fn nearest_is_perceptual() {
        let palette = Palette::parse_hex("000000\nffffff\n0000ff\n").unwrap();
        assert_eq!(palette.nearest([250, 250, 250]), [255, 255, 255]);
        assert_eq!(palette.nearest([0, 0, 0]), [0, 0, 0]);
        // Closer to black than to blue in RGB, but much more blue than black to the eye
        assert_eq!(palette.nearest([30, 30, 120]), [0, 0, 255]);
        assert_eq!(palette.nearest([255, 255, 255]), [255, 255, 255]);
    }

    #[test]
    fn every_builtin_parses() {
        let counts: Vec<(&str, usize)> = Palette::builtin_names()
            .map(|name| (name, Palette::builtin(name).unwrap().colours().len()))
            .collect();
        assert_eq!(
            counts,
            [("apollo", 46), ("resurrect-64", 64), ("aap-64", 64)]
        );
        assert!(Palette::builtin(DEFAULT_PALETTE).is_some());
        assert_eq!(
            Palette::builtin("Resurrect-64"),
            Palette::builtin("resurrect-64")
        );
        assert_eq!(Palette::builtin("nope"), None);
    }
}
