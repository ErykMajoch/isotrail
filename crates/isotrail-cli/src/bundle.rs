use std::fs::{self, File};
use std::io::BufWriter;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Args;
use isotrail_geo::{
    Cache, CachedFetcher, DEFAULT_DETAIL, DemSource, FeatureSource, Footprint, Grid, Heightmap,
    HttpFetcher, OVERPASS_URL, OverpassSource, TerrariumSource, vertical_exaggeration,
};
use isotrail_track::Track;
use png::ColorType;
use tracing::info;

const PREVIEW_SCALE: u32 = 4;
const ROUTE_RGB: [u8; 3] = [230, 32, 48];

#[derive(Debug, Args)]
pub struct BundleArgs {
    /// GPX file to read
    file: PathBuf,

    /// Write intermediate images (heightmap.png, hillshade.png) to this directory
    #[arg(long, value_name = "DIR")]
    debug_dir: PathBuf,

    /// Grid cells along the long side of the terrain slab
    #[arg(long, value_name = "N", default_value_t = DEFAULT_DETAIL.get(),
          value_parser = clap::value_parser!(u32).range(8..=1024))]
    detail: u32,

    /// Only use elevation and map data that is already in the cache
    #[arg(long)]
    offline: bool,

    /// Overpass API endpoint for OpenStreetMap data
    #[arg(long, value_name = "URL", env = "ISOTRAIL_OVERPASS_URL", default_value = OVERPASS_URL)]
    overpass_url: String,
}

pub fn run(args: &BundleArgs) -> anyhow::Result<()> {
    let track = crate::read_track(&args.file)?;
    let footprint = Footprint::from_track(&track)?;
    let detail = NonZeroU32::new(args.detail).context("--detail must be positive")?;
    let grid = Grid::new(&footprint, detail);

    let fetcher = CachedFetcher::new(HttpFetcher::new()?, Cache::new(crate::cache::dir()?))
        .offline(args.offline);
    let tiles = TerrariumSource::<HttpFetcher>::tiles(&grid).len();
    let heightmap = TerrariumSource::new(&fetcher).heightmap(&grid)?;
    let (min, max) = heightmap.range();
    let features = OverpassSource::new(&fetcher)
        .endpoint(&args.overpass_url)
        .features(&grid)?;
    for (tag, count) in features.counts() {
        info!("{count:>6}  {tag}");
    }

    let dir = &args.debug_dir;
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let heightmap_png = dir.join("heightmap.png");
    write_png(
        &heightmap_png,
        grid.cols,
        grid.rows,
        ColorType::Grayscale,
        &heightmap.greyscale(),
    )?;
    let hillshade_png = dir.join("hillshade.png");
    let preview = preview(&track, &grid, &heightmap);
    write_png(
        &hillshade_png,
        grid.cols * PREVIEW_SCALE,
        grid.rows * PREVIEW_SCALE,
        ColorType::Rgb,
        &preview,
    )?;

    println!(
        "Heightmap {} × {} cells of {:.1} m from {tiles} terrain tile{} at zoom {}",
        grid.cols,
        grid.rows,
        grid.cell_size_m,
        if tiles == 1 { "" } else { "s" },
        grid.dem_zoom()
    );
    println!("Elevation {min:.0} – {max:.0} m");
    println!(
        "Map data: {} areas and {} lines from OpenStreetMap",
        features.areas.len(),
        features.lines.len()
    );
    println!("Wrote {}", heightmap_png.display());
    println!("Wrote {}", hillshade_png.display());
    Ok(())
}

/// Hillshade at the slab's own exaggeration, enlarged, with the route on top
fn preview(track: &Track, grid: &Grid, heightmap: &Heightmap) -> Vec<u8> {
    let (min, max) = heightmap.range();
    let exaggeration = vertical_exaggeration(grid.footprint.length_m, f64::from(max - min));
    let shade = heightmap.hillshade(grid.cell_size_m, exaggeration);

    let (width, height) = (grid.cols * PREVIEW_SCALE, grid.rows * PREVIEW_SCALE);
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let s = shade[((y / PREVIEW_SCALE) * grid.cols + x / PREVIEW_SCALE) as usize];
            rgb.extend([s; 3]);
        }
    }

    let mut plot = |x: i64, y: i64| {
        if (0..i64::from(width)).contains(&x) && (0..i64::from(height)).contains(&y) {
            let i = ((y * i64::from(width) + x) * 3) as usize;
            rgb[i..i + 3].copy_from_slice(&ROUTE_RGB);
        }
    };
    let scale = f64::from(PREVIEW_SCALE);
    for segment in &track.segments {
        let pixels: Vec<(f64, f64)> = segment
            .points
            .iter()
            .map(|p| {
                let (x, y) = grid.footprint.projection.project(p.lat, p.lon);
                let (col, row) = grid.local_to_cell(x, y);
                (col * scale, row * scale)
            })
            .collect();
        for pair in pixels.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil().max(1.0);
            for step in 0..=steps as i64 {
                let t = step as f64 / steps;
                plot(
                    (a.0 + t * (b.0 - a.0)).floor() as i64,
                    (a.1 + t * (b.1 - a.1)).floor() as i64,
                );
            }
        }
    }
    rgb
}

fn write_png(
    path: &Path,
    width: u32,
    height: u32,
    color: ColorType,
    data: &[u8],
) -> anyhow::Result<()> {
    let write = || -> anyhow::Result<()> {
        let file = BufWriter::new(File::create(path)?);
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(data)?;
        writer.finish()?;
        Ok(())
    };
    write().with_context(|| format!("could not write {}", path.display()))
}
