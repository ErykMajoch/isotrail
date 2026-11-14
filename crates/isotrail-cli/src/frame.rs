use std::ops::RangeInclusive;

use clap::{Args, ValueEnum};
use isotrail_render::{
    Aim, Aspect, Canvas, Framing, HEADING_RANGE, Insets, PITCH_DEG, PITCH_RANGE, TURN_RANGE,
    ZOOM_RANGE, parse_count, parse_within,
};

use crate::usage;

pub(crate) const SCALE_RANGE: RangeInclusive<u32> = 1..=16;
const SIDE_RANGE: RangeInclusive<u32> = 16..=8192;
const MAX_SAFE_SHARE: f32 = 45.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum AspectArg {
    Story,
    Video,
    Portrait,
    Feed,
    Square,
    Wide,
}

impl From<AspectArg> for Aspect {
    fn from(aspect: AspectArg) -> Self {
        match aspect {
            AspectArg::Story => Aspect::Story,
            AspectArg::Video => Aspect::Video,
            AspectArg::Portrait => Aspect::Portrait,
            AspectArg::Feed => Aspect::Feed,
            AspectArg::Square => Aspect::Square,
            AspectArg::Wide => Aspect::Wide,
        }
    }
}

/// The image's size and safe area, and where the camera looks from
#[derive(Debug, Args)]
pub struct FrameArgs {
    /// Any sizes instead of aspects, such as 1200x900,600x450, each side from 16 to 8192
    #[arg(long, value_name = "WxH", value_parser = parse_size, value_delimiter = ',',
          conflicts_with = "aspect", help_heading = crate::IMAGE)]
    pub size: Vec<(u32, u32)>,

    /// Margins kept clear, in percent: top,right,bottom,left or one number, each from 0 to 45
    #[arg(long, value_name = "T,R,B,L", value_parser = parse_safe_area, allow_hyphen_values = true,
          help_heading = crate::IMAGE)]
    pub safe_area: Option<Insets>,

    /// How far the camera looks down, in degrees from 10 to 80
    #[arg(long, value_name = "DEG", default_value_t = PITCH_DEG, value_parser = parse_pitch,
          help_heading = crate::CAMERA)]
    pub pitch: f32,

    /// Turns the camera anticlockwise round the slab, in degrees from -360 to 360
    #[arg(long, value_name = "DEG", default_value_t = 0.0, value_parser = parse_turn,
          help_heading = crate::CAMERA)]
    pub turn: f32,

    /// Compass bearing the camera looks towards, from 0 to 360 [default: along the hike]
    #[arg(long, value_name = "DEG", value_parser = parse_heading, help_heading = crate::CAMERA)]
    pub heading: Option<f32>,

    /// The diorama's size, from 0.5 to 2 times what fits; above 1 crops it
    #[arg(long, value_name = "X", default_value_t = 1.0, value_parser = parse_zoom,
          help_heading = crate::CAMERA)]
    pub zoom: f32,
}

/// An aspect, or the size given instead
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Shape {
    Aspect(Aspect),
    Size(u32, u32),
}

impl Shape {
    /// Output size in pixels
    pub fn size(&self) -> (u32, u32) {
        match *self {
            Shape::Aspect(aspect) => aspect.output_size(),
            Shape::Size(width, height) => (width, height),
        }
    }

    /// The aspect's name, or the size
    pub fn label(&self) -> String {
        match self {
            Shape::Aspect(aspect) => aspect.name().to_owned(),
            Shape::Size(width, height) => format!("{width} x {height}"),
        }
    }

    /// The aspect's name, or the size as `640x480`, to go in a file name
    pub fn file_label(&self) -> String {
        match self {
            Shape::Aspect(aspect) => aspect.name().to_owned(),
            Shape::Size(width, height) => format!("{width}x{height}"),
        }
    }
}

impl FrameArgs {
    pub fn aim(&self) -> Aim {
        Aim {
            heading_deg: self.heading.map(f64::from),
            turn_deg: self.turn,
            pitch_deg: self.pitch,
            zoom: self.zoom,
        }
    }

    /// The given sizes, or each of `aspects`
    pub fn shapes(&self, aspects: &[Aspect]) -> Vec<Shape> {
        if self.size.is_empty() {
            aspects
                .iter()
                .map(|&aspect| Shape::Aspect(aspect))
                .collect()
        } else {
            self.size
                .iter()
                .map(|&(width, height)| Shape::Size(width, height))
                .collect()
        }
    }

    /// The shapes of `--aspect` or `--size`, refusing one listed twice
    pub fn listed_shapes(&self, aspect: &[AspectArg]) -> anyhow::Result<Vec<Shape>> {
        let aspects: Vec<Aspect> = aspect.iter().map(|&a| a.into()).collect();
        for (i, aspect) in aspects.iter().enumerate() {
            if aspects[..i].contains(aspect) {
                return Err(usage(format!("--aspect lists {} twice", aspect.name())));
            }
        }
        for (i, (width, height)) in self.size.iter().enumerate() {
            if self.size[..i].contains(&(*width, *height)) {
                return Err(usage(format!("--size lists {width}x{height} twice")));
            }
        }
        Ok(self.shapes(&aspects))
    }

    pub fn canvas(&self, shape: Shape) -> Canvas {
        let canvas = match shape {
            Shape::Aspect(aspect) => aspect.canvas(),
            Shape::Size(width, height) => Canvas::new(width, height),
        };
        Canvas {
            safe: self.safe_area.unwrap_or(canvas.safe),
            ..canvas
        }
    }

    /// The internal frame of each shape at `scale`, or a usage error naming
    /// the first that `scale` doesn't divide
    pub fn framings(&self, shapes: &[Shape], scale: u32) -> anyhow::Result<Vec<Framing>> {
        shapes
            .iter()
            .map(|&shape| {
                let canvas = self.canvas(shape);
                canvas.framing(scale).ok_or_else(|| {
                    let name = match shape {
                        Shape::Aspect(aspect) => Some(aspect.name()),
                        Shape::Size(..) => None,
                    };
                    usage(scale_mismatch(scale, canvas.width, canvas.height, name))
                })
            })
            .collect()
    }
}

/// Why `scale` can't make a `width` x `height` image, and the scales that can
pub(crate) fn scale_mismatch(scale: u32, width: u32, height: u32, aspect: Option<&str>) -> String {
    let fits: Vec<String> = SCALE_RANGE
        .filter(|s| width.is_multiple_of(*s) && height.is_multiple_of(*s))
        .map(|s| s.to_string())
        .collect();
    let image = match aspect {
        Some(name) => format!("the {name} image ({width} x {height})"),
        None => format!("{width} x {height}"),
    };
    let size_hint = match aspect {
        Some(_) => String::new(),
        None => format!(", or a size that is a multiple of {scale}"),
    };
    match fits.split_last() {
        Some((last, [])) => {
            format!("--scale {scale} doesn't divide {image}; use --scale {last}{size_hint}")
        }
        Some((last, rest)) => format!(
            "--scale {scale} doesn't divide {image}; use --scale {} or {last}{size_hint}",
            rest.join(", ")
        ),
        None => format!("--scale {scale} doesn't divide {image}{size_hint}"),
    }
}

pub(crate) fn parse_scale(value: &str) -> Result<u32, String> {
    parse_count(value, &SCALE_RANGE)
}

fn parse_size(value: &str) -> Result<(u32, u32), String> {
    let error = || "expected WIDTHxHEIGHT in pixels, such as 1200x900".to_owned();
    let (width, height) = value.trim().split_once(['x', 'X', '×']).ok_or_else(error)?;
    let side = |text: &str| -> Result<u32, String> {
        let side: u32 = text.trim().parse().map_err(|_| error())?;
        if SIDE_RANGE.contains(&side) {
            Ok(side)
        } else {
            Err(format!(
                "expected each side from {} to {} pixels",
                SIDE_RANGE.start(),
                SIDE_RANGE.end()
            ))
        }
    };
    Ok((side(width)?, side(height)?))
}

fn parse_safe_area(value: &str) -> Result<Insets, String> {
    let shares = value
        .split(',')
        .map(|part| {
            let percent: f32 = part
                .trim()
                .parse()
                .ok()
                .filter(|percent| (0.0..=MAX_SAFE_SHARE).contains(percent))
                .ok_or_else(|| {
                    format!("expected each margin from 0 to {MAX_SAFE_SHARE} percent")
                })?;
            Ok(percent / 100.0)
        })
        .collect::<Result<Vec<f32>, String>>()?;
    match shares[..] {
        [all] => Ok(Insets::even(all)),
        [top, right, bottom, left] => Ok(Insets {
            top,
            bottom,
            left,
            right,
        }),
        _ => Err("expected one margin, or four: TOP,RIGHT,BOTTOM,LEFT".into()),
    }
}

fn parse_pitch(value: &str) -> Result<f32, String> {
    parse_within(value, &PITCH_RANGE)
}

fn parse_turn(value: &str) -> Result<f32, String> {
    parse_within(value, &TURN_RANGE)
}

fn parse_heading(value: &str) -> Result<f32, String> {
    parse_within(value, &HEADING_RANGE)
}

fn parse_zoom(value: &str) -> Result<f32, String> {
    parse_within(value, &ZOOM_RANGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_width_by_height() {
        assert_eq!(parse_size("1200x900"), Ok((1200, 900)));
        assert_eq!(parse_size(" 1080 × 1440 "), Ok((1080, 1440)));
        assert_eq!(parse_size("16X8192"), Ok((16, 8192)));
        for bad in [
            "1200",
            "1200x",
            "x800",
            "15x800",
            "1200x9000",
            "-5x10",
            "axb",
        ] {
            assert!(parse_size(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn safe_areas_are_one_or_four_percentages() {
        assert_eq!(parse_safe_area("10"), Ok(Insets::even(0.1)));
        assert_eq!(
            parse_safe_area("14, 6, 35, 4"),
            Ok(Insets {
                top: 0.14,
                right: 0.06,
                bottom: 0.35,
                left: 0.04,
            })
        );
        for bad in ["", "10,10", "10,10,10", "50", "-1", "a,b,c,d", "1,2,3,4,5"] {
            assert!(parse_safe_area(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn scales_are_whole_numbers_up_to_sixteen() {
        for scale in [1, 4, 7, 16] {
            assert_eq!(parse_scale(&scale.to_string()), Ok(scale));
        }
        for bad in ["0", "17", "2.5", "x", "-1"] {
            assert_eq!(
                parse_scale(bad),
                Err("expected a whole number from 1 to 16".to_owned()),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_scale_that_doesnt_divide_names_those_that_do() {
        assert_eq!(
            scale_mismatch(3, 1200, 800, None),
            "--scale 3 doesn't divide 1200 x 800; use --scale 1, 2, 4, 5, 8, 10 or 16, \
             or a size that is a multiple of 3"
        );
        assert_eq!(
            scale_mismatch(7, 1080, 1920, Some("story")),
            "--scale 7 doesn't divide the story image (1080 x 1920); use --scale 1, 2, 3, 4, 5, \
             6, 8, 10, 12 or 15"
        );
        assert_eq!(
            scale_mismatch(2, 17, 17, None),
            "--scale 2 doesn't divide 17 x 17; use --scale 1, or a size that is a multiple of 2"
        );
    }

    #[test]
    fn camera_angles_are_checked() {
        assert_eq!(parse_pitch("45"), Ok(45.0));
        assert!(parse_pitch("5").is_err() && parse_pitch("85").is_err());
        assert_eq!(parse_turn("-90"), Ok(-90.0));
        assert!(parse_turn("400").is_err());
        assert!(parse_heading("361").is_err() && parse_heading("-1").is_err());
        assert!(parse_zoom("0.4").is_err() && parse_zoom("2.5").is_err());
    }
}
