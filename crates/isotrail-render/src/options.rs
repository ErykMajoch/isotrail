use std::fmt;
use std::ops::RangeInclusive;
use std::str::FromStr;

use crate::{
    Background, ForcedWeather, GodRays, HiddenRoute, Layout, Markers, Mesher, Shaped, SmoothMesher,
    Terrain, VoxelMesher, WeatherEffects,
};

pub const OCCLUSION_RANGE: RangeInclusive<f32> = 0.0..=1.0;
pub const SOFT_SHADOWS_RANGE: RangeInclusive<f32> = 0.0..=4.0;
pub const BLOCK_HEIGHT_RANGE: RangeInclusive<f32> = 0.25..=4.0;
pub const PITCH_RANGE: RangeInclusive<f32> = 10.0..=80.0;
pub const TURN_RANGE: RangeInclusive<f32> = -360.0..=360.0;
pub const HEADING_RANGE: RangeInclusive<f32> = 0.0..=360.0;
pub const ZOOM_RANGE: RangeInclusive<f32> = 0.5..=2.0;
pub const PROP_DENSITY_RANGE: RangeInclusive<f32> = 0.0..=2.0;
pub const BASE_BLOCKS_RANGE: RangeInclusive<u32> = 1..=24;
pub const OUTLINES_RANGE: RangeInclusive<f32> = 0.0..=2.0;
pub const ROUTE_WIDTH_RANGE: RangeInclusive<u32> = 1..=4;
pub const SUN_AZIMUTH_RANGE: RangeInclusive<f32> = 0.0..=360.0;
pub const SUN_ELEVATION_RANGE: RangeInclusive<f32> = -90.0..=90.0;

/// A whole number within `range`, or why not
pub fn parse_count(value: &str, range: &RangeInclusive<u32>) -> Result<u32, String> {
    let number: u32 = value
        .trim()
        .parse()
        .map_err(|_| "expected a whole number".to_owned())?;
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

/// `on` or `off`
pub fn parse_switch(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err("expected on or off".to_owned()),
    }
}

pub fn switch_name(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// A hand-set sun as `AZIMUTH,ELEVATION` in degrees
pub fn parse_sun(value: &str) -> Result<[f64; 2], String> {
    let (azimuth, elevation) = value
        .split_once(',')
        .ok_or_else(|| "expected AZIMUTH,ELEVATION in degrees, such as 200,25".to_owned())?;
    let azimuth = parse_within(azimuth, &SUN_AZIMUTH_RANGE).map_err(|e| format!("azimuth {e}"))?;
    let elevation =
        parse_within(elevation, &SUN_ELEVATION_RANGE).map_err(|e| format!("elevation {e}"))?;
    Ok([azimuth.into(), elevation.into()])
}

/// A colour such as `#d64834`
pub fn parse_colour(value: &str) -> Result<[u8; 3], String> {
    match value.parse::<Background>() {
        Ok(Background::Colour(rgb)) => Ok(rgb),
        _ => Err("expected a colour such as #d64834".to_owned()),
    }
}

pub fn colour_name([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Names from `names`, comma-separated and each at most once, or `none`
fn parse_names<'a>(value: &str, names: &[&'a str]) -> Result<Vec<&'a str>, String> {
    let value = value.trim().to_ascii_lowercase();
    if value == "none" {
        return Ok(Vec::new());
    }
    let mut chosen = Vec::new();
    for part in value.split(',').map(str::trim) {
        let Some(&name) = names.iter().find(|&&name| name == part) else {
            return Err(format!("expected {} or none", names.join(", ")));
        };
        if chosen.contains(&name) {
            return Err(format!("{name} is named twice"));
        }
        chosen.push(name);
    }
    Ok(chosen)
}

fn write_names(f: &mut fmt::Formatter<'_>, names: &[(&str, bool)]) -> fmt::Result {
    let shown: Vec<&str> = names
        .iter()
        .filter(|(_, on)| *on)
        .map(|(name, _)| *name)
        .collect();
    if shown.is_empty() {
        f.write_str("none")
    } else {
        f.write_str(&shown.join(","))
    }
}

const EFFECTS: [&str; 5] = ["clouds", "shade", "rain", "snow", "haze"];

impl FromStr for WeatherEffects {
    type Err = String;

    fn from_str(value: &str) -> Result<WeatherEffects, String> {
        let chosen = parse_names(value, &EFFECTS)?;
        let has = |name| chosen.contains(&name);
        Ok(WeatherEffects {
            clouds: has("clouds"),
            shade: has("shade"),
            rain: has("rain"),
            snow: has("snow"),
            haze: has("haze"),
        })
    }
}

impl fmt::Display for WeatherEffects {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_names(
            f,
            &[
                ("clouds", self.clouds),
                ("shade", self.shade),
                ("rain", self.rain),
                ("snow", self.snow),
                ("haze", self.haze),
            ],
        )
    }
}

const MARKERS: [&str; 3] = ["start", "finish", "head"];

impl FromStr for Markers {
    type Err = String;

    fn from_str(value: &str) -> Result<Markers, String> {
        let chosen = parse_names(value, &MARKERS)?;
        Ok(Markers {
            start: chosen.contains(&"start"),
            finish: chosen.contains(&"finish"),
            head: chosen.contains(&"head"),
        })
    }
}

impl fmt::Display for Markers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_names(
            f,
            &[
                ("start", self.start),
                ("finish", self.finish),
                ("head", self.head),
            ],
        )
    }
}

impl ForcedWeather {
    pub fn name(self) -> &'static str {
        match self {
            ForcedWeather::Clear => "clear",
            ForcedWeather::Cloudy => "cloudy",
            ForcedWeather::Overcast => "overcast",
            ForcedWeather::Rain => "rain",
            ForcedWeather::Snow => "snow",
            ForcedWeather::Fog => "fog",
        }
    }
}

impl FromStr for ForcedWeather {
    type Err = String;

    fn from_str(value: &str) -> Result<ForcedWeather, String> {
        let value = value.trim().to_ascii_lowercase();
        ForcedWeather::ALL
            .into_iter()
            .find(|sky| sky.name() == value)
            .ok_or_else(|| "expected clear, cloudy, overcast, rain, snow or fog".to_owned())
    }
}

impl fmt::Display for ForcedWeather {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for HiddenRoute {
    type Err = String;

    fn from_str(value: &str) -> Result<HiddenRoute, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dashed" => Ok(HiddenRoute::Dashed),
            "dim" => Ok(HiddenRoute::Dim),
            "solid" => Ok(HiddenRoute::Solid),
            "off" => Ok(HiddenRoute::Off),
            _ => Err("expected dashed, dim, solid or off".to_owned()),
        }
    }
}

impl fmt::Display for HiddenRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HiddenRoute::Dashed => "dashed",
            HiddenRoute::Dim => "dim",
            HiddenRoute::Solid => "solid",
            HiddenRoute::Off => "off",
        })
    }
}

impl FromStr for Layout {
    type Err = String;

    fn from_str(value: &str) -> Result<Layout, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Layout::Auto),
            "stacked" => Ok(Layout::Stacked),
            "column" => Ok(Layout::Column),
            _ => Err("expected auto, stacked or column".to_owned()),
        }
    }
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Layout::Auto => "auto",
            Layout::Stacked => "stacked",
            Layout::Column => "column",
        })
    }
}

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
    /// The mesher, with `block_height` for voxels (ignored when smooth),
    /// showing the slab as `terrain` has it
    pub fn mesher(self, block_height: Option<f32>, terrain: Terrain) -> Box<dyn Mesher> {
        match self {
            MesherKind::Voxel => Box::new(Shaped {
                mesher: VoxelMesher {
                    block_height: block_height.unwrap_or(VoxelMesher::default().block_height),
                },
                terrain,
            }),
            MesherKind::Smooth => Box::new(Shaped {
                mesher: SmoothMesher,
                terrain,
            }),
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

    #[test]
    fn switches_counts_and_suns_are_checked() {
        assert_eq!(parse_switch(" ON "), Ok(true));
        assert_eq!(parse_switch("off"), Ok(false));
        assert!(parse_switch("yes").is_err());
        assert_eq!(parse_count("6", &BASE_BLOCKS_RANGE), Ok(6));
        assert_eq!(
            parse_count("25", &BASE_BLOCKS_RANGE),
            Err("must be between 1 and 24".to_owned())
        );
        assert!(parse_count("2.5", &ROUTE_WIDTH_RANGE).is_err());
        assert_eq!(parse_sun("200, -10"), Ok([200.0, -10.0]));
        assert!(parse_sun("200").unwrap_err().contains("AZIMUTH,ELEVATION"));
        assert!(parse_sun("400,10").unwrap_err().starts_with("azimuth"));
        assert!(parse_sun("10,91").unwrap_err().starts_with("elevation"));
        assert_eq!(parse_colour("#D64834"), Ok([214, 72, 52]));
        assert!(parse_colour("sky").is_err());
        assert_eq!(colour_name([214, 72, 52]), "#d64834");
    }

    #[test]
    fn lists_name_each_part_once_or_none() {
        assert_eq!(
            "clouds,shade,rain,snow,haze".parse(),
            Ok(WeatherEffects::ALL)
        );
        assert_eq!("none".parse(), Ok(WeatherEffects::NONE));
        let some: WeatherEffects = "rain, clouds".parse().unwrap();
        assert_eq!(some.to_string(), "clouds,rain");
        assert!(
            "rain,rain"
                .parse::<WeatherEffects>()
                .unwrap_err()
                .contains("twice")
        );
        assert!("wind".parse::<WeatherEffects>().is_err());
        assert_eq!(WeatherEffects::NONE.to_string(), "none");
        assert_eq!("start,finish,head".parse(), Ok(Markers::ALL));
        assert_eq!("None".parse(), Ok(Markers::NONE));
        assert_eq!("head".parse::<Markers>().unwrap().to_string(), "head");
        for sky in ForcedWeather::ALL {
            assert_eq!(sky.to_string().parse(), Ok(sky));
        }
        assert!("sunny".parse::<ForcedWeather>().is_err());
        for text in ["dashed", "dim", "solid", "off"] {
            assert_eq!(text.parse::<HiddenRoute>().unwrap().to_string(), text);
        }
        for text in ["auto", "stacked", "column"] {
            assert_eq!(text.parse::<Layout>().unwrap().to_string(), text);
        }
    }
}
