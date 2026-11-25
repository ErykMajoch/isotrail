use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use clap::Args;
use isotrail_bundle::Route;
use isotrail_geo::{
    Cache, CachedFetcher, FetchError, Fetcher, Fetchers, Grid, Heightmap, HttpFetcher, Landcover,
    LandcoverMap, MAX_TIMED_SPAN_DAYS, OVERPASS_URL, Prepared, Progress, RequestError, RetryPolicy,
    SceneBundle, SceneError, TERRARIUM_URL, Warning, WeatherError, origin,
};
pub use isotrail_options::data::{DataArgs, TimesArg, distance_text, parse_exaggeration};
use isotrail_track::Track;
use png::ColorType;
use tracing::warn;

use crate::output;
use crate::report::{self, Report, ReportArgs, Written, say, shown};
use crate::season;
use crate::usage;
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
    #[arg(short, long, value_name = "BUNDLE", help_heading = crate::OUTPUT)]
    output: PathBuf,

    /// Also write images of the prepared terrain to this folder
    #[arg(long, value_name = "DIR", help_heading = crate::OUTPUT)]
    debug_dir: Option<PathBuf>,

    #[command(flatten)]
    report: ReportArgs,

    /// How many times taller than life to draw the terrain, from 0.1 to 100 [default: automatic]
    #[arg(long, value_name = "X", value_parser = parse_exaggeration, help_heading = crate::LOOK)]
    exaggeration: Option<f64>,

    /// Title to save instead of the hike's name
    #[arg(long, value_name = "TEXT", help_heading = crate::TEXT)]
    title: Option<String>,

    #[command(flatten)]
    data: DataArgs,

    /// Leave the GPX file's description out of the scene file
    #[arg(long, help_heading = crate::DATA)]
    no_desc: bool,

    /// Read settings from the [bundle] and [render] tables of this TOML file
    #[arg(long, value_name = "FILE", help_heading = crate::GENERAL)]
    config: Option<PathBuf>,
}

/// Fetches through the shared cache, or the one in `cache_dir`
pub fn fetcher(
    offline: bool,
    cache_dir: Option<&Path>,
) -> anyhow::Result<CachedFetcher<HttpFetcher>> {
    let cache = Cache::new(crate::cache::dir(cache_dir)?);
    Ok(CachedFetcher::new(HttpFetcher::new()?, cache).offline(offline))
}

/// Says that data is being downloaded before the run's first request that
/// the cache can't answer
struct Announced<F> {
    inner: F,
    line: String,
}

impl<F: Fetcher> Fetcher for Announced<F> {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        report::downloading(&self.line);
        self.inner.get(url)
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
        report::downloading(&self.line);
        self.inner.post(url, body)
    }

    fn forget(&self, url: &str, body: Option<&[u8]>) -> Result<(), FetchError> {
        self.inner.forget(url, body)
    }
}

/// `http` behind the cache, announcing its first download
fn cached(
    data: &DataArgs,
    http: HttpFetcher,
) -> anyhow::Result<CachedFetcher<Announced<HttpFetcher>>> {
    let cache = Cache::new(crate::cache::dir(data.cache_dir.get())?);
    let announced = Announced {
        inner: http,
        line: report::download_line(
            !data.no_map_data,
            !data.no_weather && data.times != TimesArg::None,
        ),
    };
    Ok(CachedFetcher::new(announced, cache).offline(data.offline))
}

/// The next step when --offline finds nothing cached; a server other than
/// the usual one is named, without any key in its address
fn not_cached(what: &str, url: &str, usual: &str, more: &str) -> anyhow::Error {
    let from = if url == usual {
        String::new()
    } else {
        format!(" from {}", origin(url))
    };
    NotCached(format!(
        "the {what}{from} for this area isn't in the cache yet; run once without --offline \
         to download it{more}"
    ))
    .into()
}

/// Data that --offline found missing from the cache
#[derive(Debug)]
pub struct NotCached(String);

impl std::fmt::Display for NotCached {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotCached {}

/// Whether a download failed only because it wasn't cached and --offline is on
fn is_offline(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut cause = Some(error);
    while let Some(error) = cause {
        if matches!(error.downcast_ref(), Some(FetchError::Offline { .. })) {
            return true;
        }
        cause = error.source();
    }
    false
}

/// Prints the pipeline's warnings as the command line words them
pub struct Warnings;

impl Progress for Warnings {
    fn warn(&mut self, warning: Warning) {
        match warning {
            Warning::MapDataSkipped => warn!(
                "--no-map-data: building the diorama from the elevation alone, without lakes, woods or paths"
            ),
            Warning::TimesLeftOut => warn!(
                "--times none: leaving out the date and times, with the light, weather and season that follow from them"
            ),
            Warning::NoWeatherRequest(RequestError::Untimed) => {
                warn!("the track has no times, so the weather is left out");
            }
            Warning::NoWeatherRequest(RequestError::TooLong { days }) => warn!(
                "the hike spans {days} days, more than {MAX_TIMED_SPAN_DAYS}, so the weather is left out"
            ),
            Warning::Weather(WeatherError::Offline { .. }) => warn!(
                "the weather for this hike isn't in the cache, so it is left out; run once without --offline to add it"
            ),
            Warning::Weather(error) => warn!(
                "can't get the weather, so it is left out: {}",
                crate::describe(&error.into())
            ),
            Warning::DaysBefore(WeatherError::Offline { .. }) => warn!(
                "the weather before the hike isn't in the cache, so lakes are left open; run once without --offline to add it"
            ),
            Warning::DaysBefore(error) => warn!(
                "can't get the weather before the hike, so lakes are left open: {}",
                crate::describe(&error.into())
            ),
        }
    }
}

/// Fetches terrain, map data and weather around the track read from `file`
/// and packs them into a bundle
pub fn prepare(track: &Track, file: &Path, data: &DataArgs) -> anyhow::Result<Prepared> {
    let options = data.options(crate::servers::key());
    let fetcher = cached(data, HttpFetcher::new()?)?;
    let map_fetcher = cached(data, HttpFetcher::new()?.with_retry(RetryPolicy::once()))?;
    let fetchers = Fetchers {
        data: &fetcher,
        map_data: &map_fetcher,
        waits: true,
    };
    let title = track.title(file);
    isotrail_geo::prepare(
        track,
        &title,
        &options,
        fetchers,
        weather::now(),
        &mut Warnings,
    )
    .map_err(|error| match error {
        SceneError::TrimmedAway { length_m } => {
            let cuts: Vec<String> = [(options.trim_start_m, "start"), (options.trim_end_m, "end")]
                .into_iter()
                .filter(|&(metres, _)| metres > 0.0)
                .map(|(metres, from)| format!("{} from the {from}", distance_text(metres)))
                .collect();
            usage(format!(
                "trimming {} leaves none of the route, which is {} long; trim less",
                cuts.join(" and "),
                distance_text(length_m)
            ))
        }
        SceneError::NoGround => usage(format!(
            "the route stays on one spot, so --padding {} leaves no ground to draw; \
             give it some metres, such as --padding 15,500",
            data.padding_text()
        )),
        SceneError::Footprint(error) | SceneError::Scene(error) => error.into(),
        SceneError::Elevation(error) => {
            if is_offline(&error) {
                not_cached("elevation data", &data.terrain_url, TERRARIUM_URL, "")
            } else {
                anyhow::Error::new(error)
            }
        }
        SceneError::MapData(error) => {
            if error.is_offline() {
                not_cached(
                    "map data",
                    &data.overpass_url,
                    OVERPASS_URL,
                    ", or add --no-map-data",
                )
            } else {
                anyhow::Error::new(error).context(MAP_DATA_WAYS_OUT)
            }
        }
        SceneError::Invalid(reason) => anyhow::anyhow!(
            "the scene made from {} can't be drawn: {reason}",
            file.display()
        ),
        SceneError::Bundle(error) => error.into(),
    })
}

pub fn run(args: &BundleArgs) -> anyhow::Result<()> {
    crate::check_input(&args.file)?;
    let mut report = Report::new(&args.report);
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
    let inputs: Vec<&Path> = [Some(args.file.as_path()), args.config.as_deref()]
        .into_iter()
        .flatten()
        .collect();
    output::plan(
        [args.output.as_path()]
            .into_iter()
            .chain(debug.iter().map(PathBuf::as_path)),
        &inputs,
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
    if let Some(title) = &args.title {
        bundle.meta.title.clone_from(title);
    }
    if args.no_desc {
        bundle.meta.desc = None;
    }
    let bytes = bundle.to_bytes()?;
    let output = &args.output;
    output::write(output, |file| Ok(file.write_all(&bytes)?))?;
    let mut written = vec![(output.clone(), Some(bytes.len()))];
    if let Some(dir) = &args.debug_dir {
        let images = write_debug_images(dir, &grid, &heightmap, &landcover, &bundle)?;
        written.extend(images.into_iter().map(|path| (path, None)));
    }

    say!(
        "Heightmap {} x {} cells of {:.1}m from {tiles} terrain tile{} at zoom {}",
        grid.cols,
        grid.rows,
        grid.cell_size_m,
        if tiles == 1 { "" } else { "s" },
        grid.dem_zoom()
    );
    say!(
        "Elevation {min:.0} – {max:.0}m, drawn {:.1}x taller",
        bundle.grid.exaggeration
    );
    match features {
        Some(features) => say!(
            "Map data: {} areas and {} lines from OpenStreetMap",
            features.areas.len(),
            features.lines.len()
        ),
        None => say!("Map data: none (--no-map-data)"),
    }
    let shares: Vec<String> = landcover
        .shares()
        .into_iter()
        .filter(|&(_, share)| share >= MIN_REPORTED_SHARE)
        .map(|(class, share)| format!("{} {:.0}%", class.name(), share * 100.0))
        .collect();
    say!("Landcover: {}", shares.join(", "));
    let lakes = landcover.water.iter().filter(|body| !body.sea).count();
    let lakes = format!("{lakes} {}", if lakes == 1 { "lake" } else { "lakes" });
    match landcover.water.iter().any(|body| body.sea) {
        false => say!("Water: {lakes} levelled"),
        true if lakes.starts_with("0 ") => say!("Water: the sea, at sea level"),
        true => say!("Water: the sea and {lakes} levelled"),
    }
    let stats = &bundle.meta.stats;
    let weather = bundle.weather.as_ref().and_then(|weather| {
        let summary = weather::Summary::new(weather, stats.start?, stats.end?)?;
        Some(summary.line(weather.dataset))
    });
    say!("Weather: {}", weather.as_deref().unwrap_or("none"));
    say!("Season: {}", season::describe(&bundle.season));
    let pieces = bundle.route.breaks.len() + 1;
    say!(
        "Route: {} points in {pieces} piece{}",
        bundle.route.points.len(),
        if pieces == 1 { "" } else { "s" }
    );
    for (path, size) in written {
        match size {
            Some(bytes) => say!("Wrote {} ({}KB)", shown(&path), bytes.div_ceil(1024)),
            None => say!("Wrote {}", shown(&path)),
        }
        report.add(Written::file(&path));
    }
    report.finish(None)
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
