//! Renders the GPU test scene to a PNG and prints which adapter drew it
//!
//! `cargo run -p isotrail-export --example test_scene -- out/test-scene.png`

use std::path::PathBuf;

use anyhow::Context;
use isotrail_export::{upscale, write_png};
use isotrail_render::{Gpu, OffscreenTarget, draw_test_scene};

fn main() -> anyhow::Result<()> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .context("usage: test_scene <output.png>")?,
    );
    let gpu = Gpu::headless()?;
    let info = gpu.adapter_info();
    println!(
        "adapter: {} ({}, {:?})",
        info.name, info.backend, info.device_type
    );

    let target = OffscreenTarget::new(&gpu, 90, 60);
    draw_test_scene(&gpu, &target);
    let frame = upscale(&target.read_back(&gpu)?, 3)?;
    write_png(&path, &frame)?;
    println!("wrote {}", path.display());
    Ok(())
}
