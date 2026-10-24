use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::Args;
use isotrail_bundle::SceneBundle;
use isotrail_export::{DEFAULT_SCALE, render_still, upscale, write_png};
use isotrail_render::{Aspect, DEFAULT_YAW_OFFSET_DEG, Gpu, SmoothMesher};

use crate::bundle::DataArgs;

#[derive(Debug, Args)]
pub struct RenderArgs {
    /// GPX file or scene bundle (.itb) to render
    file: PathBuf,

    /// Where to write the image (.png); defaults to the input's name in the
    /// current directory
    #[arg(short, long, value_name = "IMAGE")]
    output: Option<PathBuf>,

    #[command(flatten)]
    data: DataArgs,
}

pub fn run(args: &RenderArgs) -> anyhow::Result<()> {
    let output = output_path(&args.file, args.output.as_deref())?;
    let bundle = read_scene(&args.file, &args.data)?;

    let gpu = Gpu::headless()?;
    let aspect = Aspect::Story;
    let framing = aspect
        .framing(DEFAULT_SCALE)
        .context("the pixel scale doesn't divide the image size")?;
    let frame = render_still(
        &gpu,
        &bundle,
        &SmoothMesher,
        &framing,
        DEFAULT_YAW_OFFSET_DEG,
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

fn read_scene(path: &Path, data: &DataArgs) -> anyhow::Result<SceneBundle> {
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
