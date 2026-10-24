use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::BufWriter;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Args;
use isotrail_bundle::Route;
use isotrail_geo::{
    Cache, CachedFetcher, DEFAULT_DETAIL, DemSource, FeatureSource, Footprint, Grid, Heightmap,
    HttpFetcher, Landcover, LandcoverMap, OVERPASS_URL, OsmFeatures, OverpassSource, Provenance,
    SceneBundle, TERRARIUM_URL, TerrariumSource, line_mask, scene_bundle,
};
use png::ColorType;
use tracing::info;

const PREVIEW_SCALE: u32 = 4;
const ROUTE_RGB: [u8; 3] = [230, 32, 48];
const MIN_REPORTED_SHARE: f64 = 0.01;

#[derive(Debug, Args)]
pub struct BundleArgs {
    /// GPX file to read
    file: PathBuf,

    /// Where to write the scene bundle (.itb)
    #[arg(short, long, value_name = "BUNDLE")]
    output: PathBuf,

    /// Also write intermediate images (heightmap, hillshade, landcover, water
    /// and path maps) to this directory
    #[arg(long, value_name = "DIR")]
    debug_dir: Option<PathBuf>,

    /// How many times taller than life to draw the terrain; chosen from the
    /// slab's size and relief by default
    #[arg(long, value_name = "X", value_parser = parse_exaggeration)]
    exaggeration: Option<f64>,

    #[command(flatten)]
    data: DataArgs,
}

pub fn parse_exaggeration(value: &str) -> Result<f64, String> {
    let exaggeration: f64 = value.parse().map_err(|e| format!("{e}"))?;
    if (0.1..=100.0).contains(&exaggeration) {
        Ok(exaggeration)
    } else {
        Err("must be between 0.1 and 100".into())
    }
}

/// How terrain and map data are fetched for a GPX file
#[derive(Debug, Args)]
pub struct DataArgs {
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

/// A scene bundle and what went into it
pub struct Prepared {
    pub bundle: SceneBundle,
    grid: Grid,
    heightmap: Heightmap,
    landcover: LandcoverMap,
    features: OsmFeatures,
    tiles: usize,
    elevation_range: (f32, f32),
}

/// Fetches terrain and map data around a GPX route and packs them into a bundle
pub fn prepare(file: &Path, data: &DataArgs) -> anyhow::Result<Prepared> {
    let track = crate::read_track(file)?;
    let footprint = Footprint::from_track(&track)?;
    let detail = NonZeroU32::new(data.detail).context("--detail must be positive")?;
    let grid = Grid::new(&footprint, detail);

    let fetcher = CachedFetcher::new(HttpFetcher::new()?, Cache::new(crate::cache::dir()?))
        .offline(data.offline);
    let tiles = TerrariumSource::<HttpFetcher>::tiles(&grid).len();
    let mut heightmap = TerrariumSource::new(&fetcher).heightmap(&grid)?;
    let features = OverpassSource::new(&fetcher)
        .endpoint(&data.overpass_url)
        .features(&grid)?;
    for (tag, count) in features.counts() {
        info!("{count:>6}  {tag}");
    }
    let elevation_range = heightmap.range();
    let landcover = LandcoverMap::classify(&grid, &features, &mut heightmap);
    for (bit, cells) in landcover.line_counts() {
        let name = match bit {
            line_mask::PATH => "path",
            line_mask::ROAD => "road",
            _ => "stream",
        };
        info!("{cells:>6}  cells with a {name}");
    }

    let provenance = Provenance {
        generator: format!("isotrail {}", env!("CARGO_PKG_VERSION")),
        elevation: format!("{TERRARIUM_URL} at zoom {}", grid.dem_zoom()),
        map_data: format!("OpenStreetMap via {}", data.overpass_url),
    };
    let bundle = scene_bundle(
        &track,
        &crate::title(&track, file),
        &grid,
        &heightmap,
        &landcover,
        provenance,
    )?;
    Ok(Prepared {
        bundle,
        grid,
        heightmap,
        landcover,
        features,
        tiles,
        elevation_range,
    })
}

pub fn run(args: &BundleArgs) -> anyhow::Result<()> {
    let Prepared {
        mut bundle,
        grid,
        heightmap,
        landcover,
        features,
        tiles,
        elevation_range: (min, max),
    } = prepare(&args.file, &args.data)?;
    if let Some(exaggeration) = args.exaggeration {
        bundle.grid.exaggeration = exaggeration;
    }
    let bytes = bundle.to_bytes();
    let output = &args.output;
    fs::write(output, &bytes).with_context(|| format!("could not write {}", output.display()))?;
    let mut written = vec![(output.clone(), Some(bytes.len()))];
    if let Some(dir) = &args.debug_dir {
        let images = write_debug_images(dir, &grid, &heightmap, &landcover, &bundle)?;
        written.extend(images.into_iter().map(|path| (path, None)));
    }

    println!(
        "Heightmap {} × {} cells of {:.1} m from {tiles} terrain tile{} at zoom {}",
        grid.cols,
        grid.rows,
        grid.cell_size_m,
        if tiles == 1 { "" } else { "s" },
        grid.dem_zoom()
    );
    println!(
        "Elevation {min:.0} – {max:.0} m, drawn {:.1}× taller",
        bundle.grid.exaggeration
    );
    println!(
        "Map data: {} areas and {} lines from OpenStreetMap",
        features.areas.len(),
        features.lines.len()
    );
    let shares: Vec<String> = landcover
        .shares()
        .into_iter()
        .filter(|&(_, share)| share >= MIN_REPORTED_SHARE)
        .map(|(class, share)| format!("{} {:.0}%", class.name(), share * 100.0))
        .collect();
    println!("Landcover: {}", shares.join(", "));
    let bodies = landcover.water.len();
    println!(
        "Water: {bodies} {} levelled",
        if bodies == 1 { "lake" } else { "lakes" }
    );
    let pieces = bundle.route.breaks.len() + 1;
    println!(
        "Route: {} points in {pieces} piece{}",
        bundle.route.points.len(),
        if pieces == 1 { "" } else { "s" }
    );
    for (path, size) in written {
        match size {
            Some(bytes) => println!("Wrote {} ({} KB)", path.display(), bytes.div_ceil(1024)),
            None => println!("Wrote {}", path.display()),
        }
    }
    Ok(())
}

fn write_debug_images(
    dir: &Path,
    grid: &Grid,
    heightmap: &Heightmap,
    landcover: &LandcoverMap,
    bundle: &SceneBundle,
) -> anyhow::Result<Vec<PathBuf>> {
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let (cols, rows) = (grid.cols, grid.rows);
    let (width, height) = (cols * PREVIEW_SCALE, rows * PREVIEW_SCALE);
    let shade = heightmap.hillshade(grid.cell_size_m, bundle.grid.exaggeration);
    let mut hillshade = enlarge(&grey_to_rgb(&shade), cols, 3);
    draw_route(&mut hillshade, &bundle.route, width, height);
    let mut landcover_rgb = enlarge(&landcover.legend_image(), cols, 3);
    draw_route(&mut landcover_rgb, &bundle.route, width, height);

    let images = [
        (
            "heightmap.png",
            cols,
            rows,
            ColorType::Grayscale,
            heightmap.greyscale(),
        ),
        ("hillshade.png", width, height, ColorType::Rgb, hillshade),
        (
            "landcover.png",
            width,
            height,
            ColorType::Rgb,
            landcover_rgb,
        ),
        (
            "water.png",
            width,
            height,
            ColorType::Grayscale,
            enlarge(&landcover.water_mask(), cols, 1),
        ),
        (
            "paths.png",
            width,
            height,
            ColorType::Rgb,
            enlarge(&landcover.line_image(), cols, 3),
        ),
    ];
    let mut written = Vec::new();
    for (name, w, h, color, data) in images {
        let path = dir.join(name);
        write_png(&path, w, h, color, &data)?;
        written.push(path);
    }
    let legend = dir.join("landcover-legend.txt");
    fs::write(&legend, legend_text(landcover))
        .with_context(|| format!("could not write {}", legend.display()))?;
    written.push(legend);
    Ok(written)
}

fn legend_text(landcover: &LandcoverMap) -> String {
    let mut text = String::new();
    let shares = landcover.shares();
    for class in Landcover::ALL {
        let [r, g, b] = class.legend_rgb();
        let share = shares
            .iter()
            .find(|(c, _)| *c == class)
            .map_or(0.0, |(_, s)| *s);
        let _ = writeln!(
            text,
            "#{r:02x}{g:02x}{b:02x}  {:>5.1}%  {}",
            share * 100.0,
            class.name()
        );
    }
    text
}

fn grey_to_rgb(grey: &[u8]) -> Vec<u8> {
    grey.iter().flat_map(|&g| [g; 3]).collect()
}

/// Each cell becomes a PREVIEW_SCALE square of pixels
fn enlarge(cells: &[u8], cols: u32, channels: usize) -> Vec<u8> {
    let (cols, scale) = (cols as usize, PREVIEW_SCALE as usize);
    let mut out = Vec::with_capacity(cells.len() * scale * scale);
    for row in cells.chunks_exact(cols * channels) {
        for _ in 0..scale {
            for cell in row.chunks_exact(channels) {
                for _ in 0..scale {
                    out.extend_from_slice(cell);
                }
            }
        }
    }
    out
}

/// Draws the bundle's route in red over an enlarged RGB image of the grid,
/// leaving its breaks open
fn draw_route(rgb: &mut [u8], route: &Route, width: u32, height: u32) {
    let mut plot = |x: i64, y: i64| {
        if (0..i64::from(width)).contains(&x) && (0..i64::from(height)).contains(&y) {
            let i = ((y * i64::from(width) + x) * 3) as usize;
            rgb[i..i + 3].copy_from_slice(&ROUTE_RGB);
        }
    };
    let scale = f64::from(PREVIEW_SCALE);
    let pixels: Vec<(f64, f64)> = route
        .points
        .iter()
        .map(|p| (f64::from(p.col) * scale, f64::from(p.row) * scale))
        .collect();
    for (i, pair) in pixels.windows(2).enumerate() {
        if route.breaks.contains(&(i as u32 + 1)) {
            continue;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enlarging_repeats_cells_into_squares() {
        assert_eq!(PREVIEW_SCALE, 4);
        let big = enlarge(&[1, 2, 3, 4, 5, 6], 3, 1);
        assert_eq!(big.len(), 6 * 16);
        let first_row = [1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3];
        for row in 0..4 {
            assert_eq!(big[row * 12..(row + 1) * 12], first_row);
        }
        assert_eq!(big[4 * 12], 4);
        assert_eq!(big[big.len() - 1], 6);
        assert_eq!(enlarge(&[9, 8, 7], 1, 3), [9, 8, 7].repeat(16));
    }
}
