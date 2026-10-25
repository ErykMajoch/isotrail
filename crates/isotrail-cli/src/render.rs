use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::{Args, ValueEnum};
use isotrail_bundle::SceneBundle;
use isotrail_export::{DEFAULT_SCALE, render_still, upscale, write_png};
use isotrail_render::{
    Aspect, DEFAULT_PALETTE, DEFAULT_YAW_OFFSET_DEG, Gpu, Look, Mesher, Palette, SmoothMesher,
    VoxelMesher,
};
use tracing::info;

use crate::bundle::DataArgs;

#[derive(Debug, Args)]
pub struct RenderArgs {
    /// GPX file or scene bundle (.itb) to render
    file: PathBuf,

    /// Where to write the image (.png); defaults to the input's name in the
    /// current directory
    #[arg(short, long, value_name = "IMAGE")]
    output: Option<PathBuf>,

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
    data: DataArgs,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum MesherArg {
    Voxel,
    Smooth,
}

impl RenderArgs {
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

fn parse_block_height(value: &str) -> Result<f32, String> {
    let height: f32 = value.parse().map_err(|e| format!("{e}"))?;
    if (0.25..=4.0).contains(&height) {
        Ok(height)
    } else {
        Err("must be between 0.25 and 4".into())
    }
}

pub fn run(args: &RenderArgs) -> anyhow::Result<()> {
    let output = output_path(&args.file, args.output.as_deref())?;
    let mesher = args.mesher()?;
    let look = Look {
        palette: read_palette(&args.palette)?,
        ..Look::default()
    };
    let mut bundle = read_scene(&args.file, &args.data)?;
    if let Some(exaggeration) = args.exaggeration {
        bundle.grid.exaggeration = exaggeration;
    }
    info!(exaggeration = bundle.grid.exaggeration, "terrain");

    let gpu = Gpu::headless()?;
    let aspect = Aspect::Story;
    let framing = aspect
        .framing(DEFAULT_SCALE)
        .context("the pixel scale doesn't divide the image size")?;
    let frame = render_still(
        &gpu,
        &bundle,
        mesher.as_ref(),
        &framing,
        DEFAULT_YAW_OFFSET_DEG,
        &look,
    )?;
    let frame = upscale(&frame, DEFAULT_SCALE)?;
    write_png(&output, &frame)?;

    println!(
        "Rendered {} × {} from {} × {} pixels",
        frame.width(),
        frame.height(),
        framing.width,
        framing.height
    );
    println!("Wrote {}", output.display());
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

fn output_path(input: &Path, output: Option<&Path>) -> anyhow::Result<PathBuf> {
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
    if !output
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
    {
        bail!(
            "cannot write {}: only .png output is supported so far",
            output.display()
        );
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_defaults_to_the_input_name_here() {
        let path = output_path(Path::new("hikes/Derwent Edge.gpx"), None).unwrap();
        assert_eq!(path, Path::new("Derwent Edge.png"));
        let path = output_path(Path::new("ridge.v2.itb"), None).unwrap();
        assert_eq!(path, Path::new("ridge.v2.png"));
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
