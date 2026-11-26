//! Renders a bundle's default animation at scale 1 and writes some of its
//! frames as PNGs, `frame-<n>.png`, plus `still.png`, to compare with the
//! browser's
//!
//! `cargo run -p isotrail-export --example clip_frames -- hike.itb 640x360 out/frames 0,150,299`

use std::path::PathBuf;

use anyhow::Context;
use isotrail_bundle::SceneBundle;
use isotrail_export::{ExportError, render_clip, render_still, write_png};
use isotrail_render::{Aim, Canvas, Gpu, Look, Motion, SunState, VoxelMesher, When};

fn main() -> anyhow::Result<()> {
    let usage = "usage: clip_frames <bundle.itb> <WxH> <folder> <frame,frame,...>";
    let mut args = std::env::args().skip(1);
    let bundle = SceneBundle::from_bytes(&std::fs::read(args.next().context(usage)?)?)?;
    let size = args.next().context(usage)?;
    let (width, height) = size.split_once('x').context(usage)?;
    let framing = Canvas::new(width.parse()?, height.parse()?)
        .framing(1)
        .context(usage)?;
    let folder = PathBuf::from(args.next().context(usage)?);
    let wanted = args
        .next()
        .context(usage)?
        .split(',')
        .map(str::parse)
        .collect::<Result<Vec<usize>, _>>()?;
    std::fs::create_dir_all(&folder)?;

    let gpu = Gpu::headless()?;
    let (mesher, look, motion) = (VoxelMesher::default(), Look::default(), Motion::default());
    let sun = SunState::seen(&bundle, When::Start, &motion.conditions);
    let still = render_still(
        &gpu,
        &bundle,
        &mesher,
        &framing,
        &Aim::default(),
        &sun,
        &look,
    )?;
    write_png(&folder.join("still.png"), &still)?;
    let mut index = 0;
    render_clip(
        &gpu,
        &bundle,
        &mesher,
        &framing,
        &look,
        &motion,
        |_, frame| -> Result<(), ExportError> {
            if wanted.contains(&index) {
                write_png(&folder.join(format!("frame-{index}.png")), frame)?;
            }
            index += 1;
            Ok(())
        },
    )?;
    println!("wrote {index} frames' worth to {}", folder.display());
    Ok(())
}
