use std::ops::RangeInclusive;

use clap::{Args, ValueEnum};
use isotrail_render::{
    Aim, Aspect, Canvas, Framing, HEADING_RANGE, Insets, PITCH_DEG, PITCH_RANGE, TURN_RANGE,
    ZOOM_RANGE, parse_within,
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
    /// Image size in pixels, such as 1200x800, instead of an aspect; each
    /// side from 16 to 8192 and a multiple of --scale
    #[arg(long, value_name = "WxH", value_parser = parse_size, conflicts_with = "aspect")]
    pub size: Option<(u32, u32)>,

    /// Margins the diorama and text keep clear of, in percent of the image:
    /// TOP,RIGHT,BOTTOM,LEFT, or one number for every side; each aspect has
    /// its own by default, and --size keeps 5
    #[arg(long, value_name = "T,R,B,L", value_parser = parse_safe_area)]
    pub safe_area: Option<Insets>,

    /// How far the camera looks down, in degrees from 10 to 80
    #[arg(long, value_name = "DEG", default_value_t = PITCH_DEG, value_parser = parse_pitch)]
    pub pitch: f32,

    /// Moves the camera this many degrees anticlockwise round the slab, as
    /// seen from above, from -360 to 360
    #[arg(
        long,
        value_name = "DEG",
        default_value_t = 0.0,
        allow_negative_numbers = true,
        value_parser = parse_turn
    )]
    pub turn: f32,

    /// Compass bearing the camera looks towards (0 north, 90 east), instead
    /// of along the hike
    #[arg(long, value_name = "DEG", value_parser = parse_heading)]
    pub heading: Option<f32>,

    /// Draws the slab this many times the size that fits the safe area, from
    /// 0.5 to 2; above 1 crops it
    #[arg(long, value_name = "X", default_value_t = 1.0, value_parser = parse_zoom)]
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
            Shape::Size(width, height) => format!("{width} × {height}"),
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

    /// The given size, or each of `aspects`
    pub fn shapes(&self, aspects: &[Aspect]) -> Vec<Shape> {
        match self.size {
            Some((width, height)) => vec![Shape::Size(width, height)],
            None => aspects
                .iter()
                .map(|&aspect| Shape::Aspect(aspect))
                .collect(),
        }
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
                    usage(format!(
                        "--scale {scale} doesn't divide the {} × {} image{}; use a scale that \
                         divides both sides",
                        canvas.width,
                        canvas.height,
                        match shape {
                            Shape::Aspect(aspect) => format!(" ({})", aspect.name()),
                            Shape::Size(..) => String::new(),
                        }
                    ))
                })
            })
            .collect()
    }
}

pub(crate) fn parse_scale(value: &str) -> Result<u32, String> {
    let scale: u32 = value.trim().parse().map_err(|e| format!("{e}"))?;
    if SCALE_RANGE.contains(&scale) {
        Ok(scale)
    } else {
        Err(format!(
            "must be a whole number from {} to {}",
            SCALE_RANGE.start(),
            SCALE_RANGE.end()
        ))
    }
}

fn parse_size(value: &str) -> Result<(u32, u32), String> {
    let error = || "expected WIDTHxHEIGHT in pixels, such as 1200x800".to_owned();
    let (width, height) = value.trim().split_once(['x', 'X', '×']).ok_or_else(error)?;
    let side = |text: &str| -> Result<u32, String> {
        let side: u32 = text.trim().parse().map_err(|_| error())?;
        if SIDE_RANGE.contains(&side) {
            Ok(side)
        } else {
            Err(format!(
                "each side must be from {} to {} pixels",
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
                .map_err(|_| format!("{:?} is not a number", part.trim()))?;
            if (0.0..=MAX_SAFE_SHARE).contains(&percent) {
                Ok(percent / 100.0)
            } else {
                Err(format!("each margin must be from 0 to {MAX_SAFE_SHARE}%"))
            }
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
        assert_eq!(parse_size("1200x800"), Ok((1200, 800)));
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
        for bad in ["0", "17", "2.5", "x"] {
            assert!(parse_scale(bad).is_err(), "{bad}");
        }
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
