use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::{Args, ValueEnum};
use isotrail_bundle::SceneBundle;
use isotrail_export::{DEFAULT_SCALE, render_still, upscale, write_png};
use isotrail_render::{
    Aspect, Background, DEFAULT_PALETTE, DEFAULT_YAW_OFFSET_DEG, Gpu, Look, Mesher, Overlays,
    Palette, SmoothMesher, VoxelMesher,
};
use tracing::info;

use crate::bundle::DataArgs;
use crate::overlay::OverlayArgs;

#[derive(Debug, Args)]
pub struct RenderArgs {
    /// GPX file or scene bundle (.itb) to render
    file: PathBuf,

    /// Where to write the image (.png); defaults to the input's name in the
    /// current directory. With several aspects, each name gets the aspect
    /// added: hike-story.png, hike-feed.png
    #[arg(short, long, value_name = "IMAGE")]
    output: Option<PathBuf>,

    /// Image shapes to render: story (1080 × 1920), feed (1080 × 1350),
    /// square (1080 × 1080) or wide (1920 × 1080)
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        value_name = "ASPECTS",
        default_values_t = [AspectArg::Story]
    )]
    aspect: Vec<AspectArg>,

    /// Output pixels per drawn pixel: 2, 3, 5, 6 or 10
    #[arg(long, value_name = "N", default_value_t = DEFAULT_SCALE, value_parser = parse_scale)]
    scale: u32,

    /// What fills the space around the diorama: the sky, nothing
    /// (transparent) or a colour such as #1e2a33
    #[arg(long, value_name = "sky|transparent|#RRGGBB", default_value = "sky",
          value_parser = parse_background)]
    background: Background,

    /// Read settings from the [render] table of this TOML file; flags given
    /// here take precedence over it, and it over the user config file
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// How to build the terrain: stepped blocks or a smooth low-poly surface
    #[arg(long, value_enum, default_value_t = MesherArg::Voxel)]
    mesher: MesherArg,

    /// Height of a voxel block as a share of its width (default 1)
    #[arg(long, value_name = "F", value_parser = parse_block_height)]
    block_height: Option<f32>,

    /// How many times taller than life to draw the terrain; chosen from the
    /// slab's size and relief by default
    #[arg(long, value_name = "X", value_parser = crate::bundle::parse_exaggeration)]
    exaggeration: Option<f64>,

    /// Colours to draw with: a built-in palette's name, a Lospec .hex file, or
    /// none to keep the unquantised colours
    #[arg(long, value_name = "NAME|FILE", default_value = DEFAULT_PALETTE)]
    palette: String,

    #[command(flatten)]
    overlay: OverlayArgs,

    #[command(flatten)]
    data: DataArgs,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum MesherArg {
    Voxel,
    Smooth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum AspectArg {
    Story,
    Feed,
    Square,
    Wide,
}

impl From<AspectArg> for Aspect {
    fn from(aspect: AspectArg) -> Self {
        match aspect {
            AspectArg::Story => Aspect::Story,
            AspectArg::Feed => Aspect::Feed,
            AspectArg::Square => Aspect::Square,
            AspectArg::Wide => Aspect::Wide,
        }
    }
}

const SCALES: [u32; 5] = [2, 3, 5, 6, 10];

impl RenderArgs {
    fn aspects(&self) -> anyhow::Result<Vec<Aspect>> {
        let aspects: Vec<Aspect> = self.aspect.iter().map(|&a| a.into()).collect();
        for (i, aspect) in aspects.iter().enumerate() {
            if aspects[..i].contains(aspect) {
                bail!("--aspect lists {} twice", aspect.name());
            }
        }
        Ok(aspects)
    }

    fn mesher(&self) -> anyhow::Result<Box<dyn Mesher>> {
        Ok(match self.mesher {
            MesherArg::Voxel => Box::new(VoxelMesher {
                block_height: self
                    .block_height
                    .unwrap_or(VoxelMesher::default().block_height),
            }),
            MesherArg::Smooth => {
                if self.block_height.is_some() {
                    bail!("--block-height only applies to --mesher voxel");
                }
                Box::new(SmoothMesher)
            }
        })
    }
}

fn read_palette(value: &str) -> anyhow::Result<Option<Palette>> {
    if value.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    if let Some(palette) = Palette::builtin(value) {
        return Ok(Some(palette));
    }
    let path = Path::new(value);
    let is_hex = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("hex"));
    if !is_hex && !path.is_file() {
        let names: Vec<&str> = Palette::builtin_names().collect();
        bail!(
            "unknown palette {value:?}: expected {}, none or a .hex file",
            names.join(", ")
        );
    }
    let bytes = crate::read_file(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let palette = Palette::parse_hex(&text)
        .with_context(|| format!("could not read the palette {}", path.display()))?;
    Ok(Some(palette))
}

fn parse_scale(value: &str) -> Result<u32, String> {
    let scale: u32 = value.parse().map_err(|e| format!("{e}"))?;
    if SCALES.contains(&scale) {
        Ok(scale)
    } else {
        Err("must be 2, 3, 5, 6 or 10, so that it divides every image size".into())
    }
}

fn parse_background(value: &str) -> Result<Background, String> {
    let error = || "expected sky, transparent or a colour such as #1e2a33".to_owned();
    match value.to_ascii_lowercase().as_str() {
        "sky" => Ok(Background::Sky),
        "transparent" => Ok(Background::Transparent),
        hex => {
            let digits = hex.strip_prefix('#').ok_or_else(error)?;
            if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(error());
            }
            let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).map_err(|_| error());
            Ok(Background::Colour([channel(0)?, channel(2)?, channel(4)?]))
        }
    }
}

fn parse_block_height(value: &str) -> Result<f32, String> {
    let height: f32 = value.parse().map_err(|e| format!("{e}"))?;
    if (0.25..=4.0).contains(&height) {
        Ok(height)
    } else {
        Err("must be between 0.25 and 4".into())
    }
}

pub fn run(args: &RenderArgs) -> anyhow::Result<()> {
    let aspects = args.aspects()?;
    let outputs = output_paths(&args.file, args.output.as_deref(), &aspects)?;
    let mesher = args.mesher()?;
    let look = Look {
        palette: read_palette(&args.palette)?,
        background: args.background,
        ..Look::default()
    };
    let mut bundle = read_scene(&args.file, &args.data)?;
    if let Some(exaggeration) = args.exaggeration {
        bundle.grid.exaggeration = exaggeration;
    }
    info!(exaggeration = bundle.grid.exaggeration, "terrain");
    let text = args.overlay.text(&bundle)?;
    let style = args.overlay.style();

    let gpu = Gpu::headless()?;
    for (&aspect, output) in aspects.iter().zip(&outputs) {
        let framing = aspect
            .framing(args.scale)
            .context("the pixel scale doesn't divide the image size")?;
        let overlays = Overlays::layout(&text, &style, &framing);
        let mut frame = render_still(
            &gpu,
            &bundle,
            mesher.as_ref(),
            &overlays.diorama_framing(),
            DEFAULT_YAW_OFFSET_DEG,
            &look,
        )?;
        overlays.draw(&mut frame, look.palette.as_ref(), 1.0);
        let frame = upscale(&frame, args.scale)?;
        write_png(output, &frame)?;
        println!(
            "Rendered {} × {} from {} × {} pixels",
            frame.width(),
            frame.height(),
            framing.width,
            framing.height
        );
        println!("Wrote {}", output.display());
    }
    if let Some(credit) = args.overlay.caption_credit(&bundle) {
        println!("Credit for the caption: {credit}");
    }
    Ok(())
}

pub(crate) fn read_scene(path: &Path, data: &DataArgs) -> anyhow::Result<SceneBundle> {
    let bytes = crate::read_file(path)?;
    if crate::is_bundle_file(path, &bytes) {
        return SceneBundle::from_bytes(&bytes)
            .with_context(|| format!("could not read {}", path.display()));
    }
    Ok(crate::bundle::prepare(path, data)?.bundle)
}

/// One image per aspect, named after the aspect when there are several
fn output_paths(
    input: &Path,
    output: Option<&Path>,
    aspects: &[Aspect],
) -> anyhow::Result<Vec<PathBuf>> {
    let output = match output {
        Some(path) => path.to_owned(),
        None => {
            let mut name = input
                .file_stem()
                .with_context(|| format!("{} has no file name", input.display()))?
                .to_owned();
            name.push(".png");
            PathBuf::from(name)
        }
    };
    let Some(extension) = output
        .extension()
        .filter(|ext| ext.eq_ignore_ascii_case("png"))
    else {
        bail!(
            "cannot write {}: only .png output is supported so far",
            output.display()
        );
    };
    if aspects.len() == 1 {
        return Ok(vec![output]);
    }
    let stem = output
        .file_stem()
        .with_context(|| format!("{} has no file name", output.display()))?;
    Ok(aspects
        .iter()
        .map(|aspect| {
            let mut name = stem.to_owned();
            name.push(format!("-{}.", aspect.name()));
            name.push(extension);
            output.with_file_name(name)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output_path(input: &Path, output: Option<&Path>) -> anyhow::Result<PathBuf> {
        Ok(output_paths(input, output, &[Aspect::Story])?.remove(0))
    }

    #[test]
    fn output_defaults_to_the_input_name_here() {
        let path = output_path(Path::new("hikes/Derwent Edge.gpx"), None).unwrap();
        assert_eq!(path, Path::new("Derwent Edge.png"));
        let path = output_path(Path::new("ridge.v2.itb"), None).unwrap();
        assert_eq!(path, Path::new("ridge.v2.png"));
    }

    #[test]
    fn several_aspects_name_their_images() {
        let aspects = [Aspect::Story, Aspect::Wide];
        let output = Some(Path::new("out/a.v2.PNG"));
        let paths = output_paths(Path::new("hike.gpx"), output, &aspects).unwrap();
        assert_eq!(
            paths,
            [
                Path::new("out/a.v2-story.PNG"),
                Path::new("out/a.v2-wide.PNG")
            ]
        );
        let paths = output_paths(Path::new("hikes/ridge.itb"), None, &aspects).unwrap();
        assert_eq!(
            paths,
            [Path::new("ridge-story.png"), Path::new("ridge-wide.png")]
        );
    }

    #[test]
    fn scales_and_backgrounds_are_checked() {
        for scale in SCALES {
            assert_eq!(parse_scale(&scale.to_string()), Ok(scale));
        }
        for bad in ["4", "0", "x"] {
            assert!(parse_scale(bad).is_err(), "{bad}");
        }
        assert_eq!(parse_background("Sky"), Ok(Background::Sky));
        assert_eq!(parse_background("transparent"), Ok(Background::Transparent));
        assert_eq!(
            parse_background("#1E2a33"),
            Ok(Background::Colour([30, 42, 51]))
        );
        for bad in ["1e2a33", "#1e2a3", "#1e2a3g", "#1e2a334", "blue"] {
            assert!(parse_background(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_png_output_is_accepted() {
        let input = Path::new("hike.gpx");
        assert!(output_path(input, Some(Path::new("out/a.PNG"))).is_ok());
        for bad in ["a.gif", "a.mp4", "a"] {
            let message = output_path(input, Some(Path::new(bad)))
                .unwrap_err()
                .to_string();
            assert!(message.contains("only .png"), "{message}");
        }
    }
}
