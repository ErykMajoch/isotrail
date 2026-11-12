use std::path::Path;

use chrono::NaiveTime;
use clap::{Args, ValueEnum};
use isotrail_render::{
    BLOCK_HEIGHT_RANGE, Background, DEFAULT_OCCLUSION, DEFAULT_PALETTE, DEFAULT_SOFT_SHADOWS,
    GodRays, Look, Mesher, MesherKind, OCCLUSION_RANGE, Palette, SOFT_SHADOWS_RANGE, Terrain,
    Theme, VoxelMesher, parse_within,
};
use tracing::warn;

use crate::config::Typed;
use crate::usage;

/// How the diorama looks, for `render` and `embed`
#[derive(Debug, Args)]
#[command(next_help_heading = crate::LOOK)]
pub struct LookArgs {
    /// Around the diorama: the sky, transparent, or a colour such as #1e2a33
    #[arg(long, value_name = "sky|transparent|#RRGGBB", default_value = "sky",
          value_parser = parse_background, help_heading = crate::IMAGE)]
    pub background: Background,

    /// Stepped blocks (voxel) or a smooth low-poly surface
    #[arg(long, value_enum, value_name = "voxel|smooth", default_value_t = MesherArg::Voxel,
          hide_possible_values = true)]
    mesher: MesherArg,

    /// Height of a voxel block as a share of its width, from 0.25 to 4 [default: 1]
    #[arg(long, value_name = "F", value_parser = parse_block_height)]
    block_height: Option<f32>,

    /// Colours: a built-in palette, a Lospec .hex file, or none for unquantised colours
    #[arg(long, value_name = "NAME|FILE", default_value = DEFAULT_PALETTE)]
    pub palette: String,

    /// Light the scene as at this local time on the day of the hike
    #[arg(long, value_name = "HH:MM", value_parser = crate::sun::parse_time)]
    pub time: Option<NaiveTime>,

    /// How much hollows and valley floors darken, from 0 (none) to 1
    #[arg(long, value_name = "X", default_value_t = DEFAULT_OCCLUSION, value_parser = parse_share)]
    occlusion: f32,

    /// How wide shadow edges soften, in grid cells, from 0 (hard) to 4
    #[arg(long, value_name = "CELLS", default_value_t = DEFAULT_SOFT_SHADOWS, value_parser = parse_softening)]
    soft_shadows: f32,

    /// Sunbeams through a partly cloudy sky (auto), whenever the sun is up (on), or never
    #[arg(long, value_enum, value_name = "auto|on|off", default_value_t = GodRaysArg::Auto,
          hide_possible_values = true)]
    god_rays: GodRaysArg,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum GodRaysArg {
    Auto,
    On,
    Off,
}

impl From<GodRaysArg> for GodRays {
    fn from(arg: GodRaysArg) -> GodRays {
        match arg {
            GodRaysArg::Auto => GodRays::Weather,
            GodRaysArg::On => GodRays::On,
            GodRaysArg::Off => GodRays::Off,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum MesherArg {
    Voxel,
    Smooth,
}

impl From<MesherArg> for MesherKind {
    fn from(arg: MesherArg) -> MesherKind {
        match arg {
            MesherArg::Voxel => MesherKind::Voxel,
            MesherArg::Smooth => MesherKind::Smooth,
        }
    }
}

impl LookArgs {
    /// The look these flags ask for, over `theme`'s
    pub fn look(&self, theme: &Theme) -> anyhow::Result<Look> {
        Ok(Look {
            palette: read_palette(&self.palette)?,
            background: self.background,
            occlusion: self.occlusion,
            soft_shadows: self.soft_shadows,
            god_rays: self.god_rays.into(),
            ..theme.look()
        })
    }

    pub fn mesher(&self, typed: &Typed, terrain: Terrain) -> Box<dyn Mesher> {
        let kind = MesherKind::from(self.mesher);
        if kind == MesherKind::Smooth && typed.has("block_height") {
            warn!("--block-height only applies to --mesher voxel, so it does nothing here");
        }
        kind.mesher(self.block_height, terrain)
    }

    /// The .hex file `--palette` names, rather than a built-in palette or none
    pub fn palette_file(&self) -> Option<&Path> {
        palette_file(&self.palette)
    }

    /// The options that differ from the default, as the embed's attribute
    /// names and values; `palette_url` stands in for a palette file
    pub fn attributes(&self, palette_url: Option<&str>) -> Vec<(&'static str, String)> {
        let mut attributes = Vec::new();
        let palette = match (self.palette_file(), palette_url) {
            (Some(_), Some(url)) => Some(url.to_owned()),
            _ if self.palette.eq_ignore_ascii_case(DEFAULT_PALETTE) => None,
            _ => Some(self.palette.to_ascii_lowercase()),
        };
        attributes.extend(palette.map(|palette| ("palette", palette)));
        let mesher = MesherKind::from(self.mesher);
        if mesher != MesherKind::default() {
            attributes.push(("mesher", mesher.to_string()));
        }
        let default_height = VoxelMesher::default().block_height;
        if let Some(height) = self
            .block_height
            .filter(|&height| mesher == MesherKind::Voxel && height != default_height)
        {
            attributes.push(("block-height", height.to_string()));
        }
        if self.occlusion != DEFAULT_OCCLUSION {
            attributes.push(("occlusion", self.occlusion.to_string()));
        }
        if self.soft_shadows != DEFAULT_SOFT_SHADOWS {
            attributes.push(("soft-shadows", self.soft_shadows.to_string()));
        }
        let god_rays = GodRays::from(self.god_rays);
        if god_rays != GodRays::default() {
            attributes.push(("god-rays", god_rays.to_string()));
        }
        if self.background != Background::default() {
            attributes.push(("background", self.background.to_string()));
        }
        if let Some(time) = self.time {
            attributes.push(("time", time.format("%H:%M").to_string()));
        }
        attributes
    }
}

/// The path in a `--palette` value that isn't a built-in palette or none
pub fn palette_file(value: &str) -> Option<&Path> {
    let builtin = Palette::builtin_names().any(|name| name.eq_ignore_ascii_case(value));
    (!builtin && !value.eq_ignore_ascii_case("none")).then(|| Path::new(value))
}

pub fn read_palette(value: &str) -> anyhow::Result<Option<Palette>> {
    let Some(path) = palette_file(value) else {
        return Ok(Palette::builtin(value));
    };
    let is_hex = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("hex"));
    if !is_hex && !path.is_file() {
        let names: Vec<&str> = Palette::builtin_names().collect();
        return Err(usage(format!(
            "unknown palette {value:?}: expected {}, none or a .hex file",
            names.join(", ")
        )));
    }
    let bytes =
        crate::read_named(path, "the palette ").map_err(|error| usage(error.to_string()))?;
    let text = String::from_utf8_lossy(&bytes);
    let palette = Palette::parse_hex(&text).map_err(|error| {
        usage(format!(
            "can't read the palette {}: {error}",
            path.display()
        ))
    })?;
    Ok(Some(palette))
}

fn parse_background(value: &str) -> Result<Background, String> {
    value.parse()
}

fn parse_share(value: &str) -> Result<f32, String> {
    parse_within(value, &OCCLUSION_RANGE)
}

fn parse_softening(value: &str) -> Result<f32, String> {
    parse_within(value, &SOFT_SHADOWS_RANGE)
}

fn parse_block_height(value: &str) -> Result<f32, String> {
    parse_within(value, &BLOCK_HEIGHT_RANGE)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    #[command(allow_negative_numbers = true)]
    struct Cli {
        #[command(flatten)]
        look: LookArgs,
    }

    fn attributes(line: &[&str], palette_url: Option<&str>) -> Vec<(&'static str, String)> {
        let args = ["isotrail"].iter().chain(line);
        Cli::try_parse_from(args)
            .unwrap()
            .look
            .attributes(palette_url)
    }

    #[test]
    fn only_choices_that_differ_from_the_default_become_attributes() {
        assert!(attributes(&[], None).is_empty());
        assert!(attributes(&["--palette", "Apollo", "--occlusion", "1"], None).is_empty());
        let all = attributes(
            &[
                "--palette",
                "AAP-64",
                "--block-height",
                "2",
                "--occlusion",
                "0.5",
                "--soft-shadows",
                "0",
                "--god-rays",
                "off",
                "--background",
                "#1E2A33",
                "--time",
                "07:05",
            ],
            None,
        );
        assert_eq!(
            all,
            [
                ("palette", "aap-64".to_owned()),
                ("block-height", "2".to_owned()),
                ("occlusion", "0.5".to_owned()),
                ("soft-shadows", "0".to_owned()),
                ("god-rays", "off".to_owned()),
                ("background", "#1e2a33".to_owned()),
                ("time", "07:05".to_owned()),
            ]
        );
        // A block height means nothing to the smooth mesher
        let smooth = attributes(&["--mesher", "smooth", "--block-height", "2"], None);
        assert_eq!(smooth, [("mesher", "smooth".to_owned())]);
        let file = attributes(&["--palette", "my/warm.hex"], Some("warm.hex"));
        assert_eq!(file, [("palette", "warm.hex".to_owned())]);
        let none = attributes(&["--palette", "none"], None);
        assert_eq!(none, [("palette", "none".to_owned())]);
    }

    #[test]
    fn palette_files_are_neither_built_in_nor_none() {
        assert_eq!(palette_file("Resurrect-64"), None);
        assert_eq!(palette_file("NONE"), None);
        assert_eq!(palette_file("warm.hex"), Some(Path::new("warm.hex")));
        let message = read_palette("warm").unwrap_err().to_string();
        assert!(message.contains("unknown palette"), "{message}");
    }
}
