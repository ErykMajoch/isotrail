use std::fmt;
use std::ops::RangeInclusive;
use std::str::FromStr;

use crate::{Background, GodRays, Mesher, SmoothMesher, VoxelMesher};

pub const OCCLUSION_RANGE: RangeInclusive<f32> = 0.0..=1.0;
pub const SOFT_SHADOWS_RANGE: RangeInclusive<f32> = 0.0..=4.0;
pub const BLOCK_HEIGHT_RANGE: RangeInclusive<f32> = 0.25..=4.0;

/// A number within `range`, or why not
pub fn parse_within(value: &str, range: &RangeInclusive<f32>) -> Result<f32, String> {
    let number: f32 = value.trim().parse().map_err(|e| format!("{e}"))?;
    if range.contains(&number) {
        Ok(number)
    } else {
        Err(format!(
            "must be between {} and {}",
            range.start(),
            range.end()
        ))
    }
}

impl FromStr for Background {
    type Err = String;

    fn from_str(value: &str) -> Result<Background, String> {
        let error = || "expected sky, transparent or a colour such as #1e2a33".to_owned();
        match value.trim().to_ascii_lowercase().as_str() {
            "sky" => Ok(Background::Sky),
            "transparent" => Ok(Background::Transparent),
            hex => {
                let digits = hex.strip_prefix('#').ok_or_else(error)?;
                if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(error());
                }
                let channel =
                    |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).map_err(|_| error());
                Ok(Background::Colour([channel(0)?, channel(2)?, channel(4)?]))
            }
        }
    }
}

impl fmt::Display for Background {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Background::Sky => f.write_str("sky"),
            Background::Transparent => f.write_str("transparent"),
            Background::Colour([r, g, b]) => write!(f, "#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

impl FromStr for GodRays {
    type Err = String;

    fn from_str(value: &str) -> Result<GodRays, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(GodRays::Weather),
            "on" => Ok(GodRays::On),
            "off" => Ok(GodRays::Off),
            _ => Err("expected auto, on or off".to_owned()),
        }
    }
}

impl fmt::Display for GodRays {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            GodRays::Weather => "auto",
            GodRays::On => "on",
            GodRays::Off => "off",
        })
    }
}

/// The meshers by name
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MesherKind {
    #[default]
    Voxel,
    Smooth,
}

impl MesherKind {
    /// The mesher, with `block_height` for voxels (ignored when smooth)
    pub fn mesher(self, block_height: Option<f32>) -> Box<dyn Mesher> {
        match self {
            MesherKind::Voxel => Box::new(VoxelMesher {
                block_height: block_height.unwrap_or(VoxelMesher::default().block_height),
            }),
            MesherKind::Smooth => Box::new(SmoothMesher),
        }
    }
}

impl FromStr for MesherKind {
    type Err = String;

    fn from_str(value: &str) -> Result<MesherKind, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "voxel" => Ok(MesherKind::Voxel),
            "smooth" => Ok(MesherKind::Smooth),
            _ => Err("expected voxel or smooth".to_owned()),
        }
    }
}

impl fmt::Display for MesherKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MesherKind::Voxel => "voxel",
            MesherKind::Smooth => "smooth",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_checked_against_their_range() {
        assert_eq!(parse_within("0.5", &OCCLUSION_RANGE), Ok(0.5));
        assert_eq!(parse_within(" 4 ", &SOFT_SHADOWS_RANGE), Ok(4.0));
        assert_eq!(
            parse_within("0.2", &BLOCK_HEIGHT_RANGE),
            Err("must be between 0.25 and 4".to_owned())
        );
        assert!(parse_within("x", &OCCLUSION_RANGE).is_err());
        assert!(parse_within("NaN", &OCCLUSION_RANGE).is_err());
    }

    #[test]
    fn backgrounds_read_and_write_back() {
        assert_eq!("Sky".parse(), Ok(Background::Sky));
        assert_eq!("transparent".parse(), Ok(Background::Transparent));
        assert_eq!("#1E2a33".parse(), Ok(Background::Colour([30, 42, 51])));
        for bad in ["1e2a33", "#1e2a3", "#1e2a3g", "#1e2a334", "blue", "#ééé"] {
            assert!(bad.parse::<Background>().is_err(), "{bad}");
        }
        for text in ["sky", "transparent", "#1e2a33"] {
            assert_eq!(text.parse::<Background>().unwrap().to_string(), text);
        }
    }

    #[test]
    fn god_rays_and_meshers_go_by_name() {
        for text in ["auto", "on", "off"] {
            assert_eq!(text.parse::<GodRays>().unwrap().to_string(), text);
        }
        assert_eq!("auto".parse(), Ok(GodRays::Weather));
        assert!("sometimes".parse::<GodRays>().is_err());
        for text in ["voxel", "smooth"] {
            assert_eq!(text.parse::<MesherKind>().unwrap().to_string(), text);
        }
        assert!("blocky".parse::<MesherKind>().is_err());
    }
}
