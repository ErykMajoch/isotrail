use std::fmt::Write as _;
use std::io::Write as _;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Args;
use isotrail_bundle::Route;
use isotrail_geo::{
    Cache, CachedFetcher, DEFAULT_DETAIL, DemSource, FeatureSource, Footprint, Grid, Heightmap,
    HttpFetcher, Landcover, LandcoverMap, OSM_ATTRIBUTION, OVERPASS_URL, OsmFeatures,
    OverpassSource, Provenance, RetryPolicy, SceneBundle, TERRARIUM_URL, TerrariumSource,
    line_mask, origin, scene_bundle,
};
use isotrail_track::Track;
use png::ColorType;
use tracing::{info, warn};

use crate::config::Typed;
use crate::output;
use crate::season::{self, SeasonArg};
use crate::weather;

const PREVIEW_SCALE: u32 = 4;
const ROUTE_RGB: [u8; 3] = [230, 32, 48];
const MIN_REPORTED_SHARE: f64 = 0.01;
const DEBUG_IMAGES: [&str; 5] = [
    "heightmap.png",
    "hillshade.png",
    "landcover.png",
    "water.png",
    "paths.png",
];
const DEBUG_LEGEND: &str = "landcover-legend.txt";
const MAP_DATA_WAYS_OUT: &str = concat!(
    "OpenStreetMap's Overpass server is busy or failing. Try again later, use another server ",
    "with --overpass-url, or pass --no-map-data to build the diorama without lakes, woods and paths"
);

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

    /// Build the diorama from the elevation alone, without OpenStreetMap's
    /// lakes, woods and paths, when its server is busy or out of reach
    #[arg(long)]
    no_map_data: bool,

    /// Leave the weather out: don't look it up, or draw what a bundle holds,
    /// along with the snow and ice it brings
    #[arg(long)]
    pub no_weather: bool,

    /// Season to show: the hike's own, from its date, place and weather, or
    /// spring, summer, autumn or winter (snow everywhere and frozen lakes)
    #[arg(long, value_enum, value_name = "SEASON", default_value_t = SeasonArg::Auto)]
    pub season: SeasonArg,
}

impl DataArgs {
    /// Warns about flags typed for fetching data, which a bundle doesn't need
    pub fn warn_unused_with_bundle(&self, typed: &Typed) {
        for (id, flag) in [
            ("detail", "--detail"),
            ("offline", "--offline"),
            ("overpass_url", "--overpass-url"),
            ("no_map_data", "--no-map-data"),
        ] {
            if typed.has(id) {
                warn!(
                    "{flag} does nothing with a scene bundle, which already holds its terrain and map data"
                );
            }
        }
    }
}

/// A scene bundle and what went into it
pub struct Prepared {
    pub bundle: SceneBundle,
    grid: Grid,
    heightmap: Heightmap,
    landcover: LandcoverMap,
    /// None with --no-map-data
    features: Option<OsmFeatures>,
    tiles: usize,
    elevation_range: (f32, f32),
}

/// Fetches through the shared cache
pub fn fetcher(offline: bool) -> anyhow::Result<CachedFetcher<HttpFetcher>> {
    Ok(CachedFetcher::new(HttpFetcher::new()?, Cache::new(crate::cache::dir()?)).offline(offline))
}

/// Map features from Overpass, which retries by itself, or none with
/// --no-map-data
fn map_data(grid: &Grid, data: &DataArgs) -> anyhow::Result<Option<OsmFeatures>> {
    if data.no_map_data {
        warn!(
            "--no-map-data: building the diorama from the elevation alone, without lakes, woods or paths"
        );
        return Ok(None);
    }
    let http = HttpFetcher::new()?.with_retry(RetryPolicy::once());
    let fetcher = CachedFetcher::new(http, Cache::new(crate::cache::dir()?)).offline(data.offline);
    let features = OverpassSource::new(&fetcher)
        .endpoint(&data.overpass_url)
        .features(grid)
        .map_err(|err| {
            if err.is_offline() {
                anyhow::Error::new(err)
            } else {
                anyhow::Error::new(err).context(MAP_DATA_WAYS_OUT)
            }
        })?;
    for (tag, count) in features.counts() {
        info!("{count:>6}  {tag}");
    }
    Ok(Some(features))
}

/// Fetches terrain, map data and weather around the track read from `file`
/// and packs them into a bundle
pub fn prepare(track: &Track, file: &Path, data: &DataArgs) -> anyhow::Result<Prepared> {
    let footprint = Footprint::from_track(track)?;
    let detail = NonZeroU32::new(data.detail).context("--detail must be positive")?;
    let grid = Grid::new(&footprint, detail);

    let fetcher = fetcher(data.offline)?;
    let tiles = TerrariumSource::<HttpFetcher>::tiles(&grid).len();
    let mut heightmap = TerrariumSource::new(&fetcher).heightmap(&grid)?;
    let features = map_data(&grid, data)?;
    let elevation_range = heightmap.range();
    let landcover = LandcoverMap::classify(
        &grid,
        features.as_ref().unwrap_or(&OsmFeatures::default()),
        &mut heightmap,
    );
    for (bit, cells) in landcover.line_counts() {
        let name = match bit {
            line_mask::PATH => "path",
            line_mask::ROAD => "road",
            _ => "stream",
        };
        info!("{cells:>6}  cells with a {name}");
    }

    let weather = if data.no_weather {
        None
    } else {
        weather::look_up(track, &fetcher)
    };

    let provenance = Provenance {
        generator: format!("isotrail {}", env!("CARGO_PKG_VERSION")),
        elevation: format!("{TERRARIUM_URL} at zoom {}", grid.dem_zoom()),
        map_data: match features {
            Some(_) => format!("OpenStreetMap via {}", origin(&data.overpass_url)),
            None => "none".to_owned(),
        },
        weather: weather.as_ref().map(|(_, provenance)| provenance.clone()),
    };
    let mut bundle = scene_bundle(
        track,
        &crate::title(track, file),
        &grid,
        &heightmap,
        &landcover,
        provenance,
    )?;
    if features.is_none() {
        bundle
            .meta
            .attribution
            .retain(|credit| credit != OSM_ATTRIBUTION);
    }
    bundle.weather = weather.map(|(weather, _)| weather);
    season::estimate(&mut bundle, track, &fetcher);
    season::apply(&mut bundle, data.season);
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
    let debug: Vec<PathBuf> = args
        .debug_dir
        .iter()
        .flat_map(|dir| {
            DEBUG_IMAGES
                .iter()
                .chain([&DEBUG_LEGEND])
                .map(|name| dir.join(name))
        })
        .collect();
    if let Some(dir) = &args.debug_dir {
        output::folder(dir)?;
    }
    output::plan(
        [args.output.as_path()]
            .into_iter()
            .chain(debug.iter().map(PathBuf::as_path)),
        &[&args.file],
    )?;
    let track = crate::read_track(&args.file)?;
    let Prepared {
        mut bundle,
        grid,
        heightmap,
        landcover,
        features,
        tiles,
        elevation_range: (min, max),
    } = prepare(&track, &args.file, &args.data)?;
    if let Some(exaggeration) = args.exaggeration {
        bundle.grid.exaggeration = exaggeration;
    }
    let bytes = bundle.to_bytes();
    let output = &args.output;
    output::write(output, |file| Ok(file.write_all(&bytes)?))?;
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
    match features {
        Some(features) => println!(
            "Map data: {} areas and {} lines from OpenStreetMap",
            features.areas.len(),
            features.lines.len()
        ),
        None => println!("Map data: none (--no-map-data)"),
    }
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
    let stats = &bundle.meta.stats;
    let weather = bundle.weather.as_ref().and_then(|weather| {
        let summary = weather::Summary::new(weather, stats.start?, stats.end?)?;
        Some(summary.line(weather.dataset))
    });
    println!("Weather: {}", weather.as_deref().unwrap_or("none"));
    println!("Season: {}", season::describe(&bundle.season));
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
    let (cols, rows) = (grid.cols, grid.rows);
    let (width, height) = (cols * PREVIEW_SCALE, rows * PREVIEW_SCALE);
    let shade = heightmap.hillshade(grid.cell_size_m, bundle.grid.exaggeration);
    let mut hillshade = enlarge(&grey_to_rgb(&shade), cols, 3);
    draw_route(&mut hillshade, &bundle.route, width, height);
    let mut landcover_rgb = enlarge(&landcover.legend_image(), cols, 3);
    draw_route(&mut landcover_rgb, &bundle.route, width, height);

    let images = [
        (
            DEBUG_IMAGES[0],
            cols,
            rows,
            ColorType::Grayscale,
            heightmap.greyscale(),
        ),
        (DEBUG_IMAGES[1], width, height, ColorType::Rgb, hillshade),
        (
            DEBUG_IMAGES[2],
            width,
            height,
            ColorType::Rgb,
            landcover_rgb,
        ),
        (
            DEBUG_IMAGES[3],
            width,
            height,
            ColorType::Grayscale,
            enlarge(&landcover.water_mask(), cols, 1),
        ),
        (
            DEBUG_IMAGES[4],
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
    let legend = dir.join(DEBUG_LEGEND);
    let text = legend_text(landcover);
    output::write(&legend, |file| Ok(file.write_all(text.as_bytes())?))?;
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
    output::write(path, |file| {
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(data)?;
        writer.finish()?;
        Ok(())
    })
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
