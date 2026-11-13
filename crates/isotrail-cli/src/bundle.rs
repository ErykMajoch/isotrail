use std::borrow::Cow;
use std::fmt::Write as _;
use std::io::Write as _;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Args, ValueEnum};
use isotrail_bundle::{BundleError, Route};
use isotrail_geo::{
    Cache, CachedFetcher, DEFAULT_DETAIL, DemSource, FeatureSource, FetchError, Fetcher, Footprint,
    FootprintOptions, GeoError, Grid, Heightmap, HttpFetcher, Landcover, LandcoverMap,
    MIN_PADDING_M, OSM_ATTRIBUTION, OVERPASS_URL, Orientation, OsmFeatures, OverpassSource,
    PADDING_FRACTION, Provenance, RetryPolicy, SceneBundle, TERRARIUM_URL, TerrariumSource,
    line_mask, origin, redact, retime, scene_bundle, untime,
};
use isotrail_render::parse_count;
use isotrail_track::Track;
use png::ColorType;
use tracing::{info, warn};

use crate::config::Typed;
use crate::output;
use crate::report::{self, Report, ReportArgs, Written, say, shown};
use crate::season::{self, SeasonArg};
use crate::servers::{CacheDirArg, WeatherUrlArg, parse_template};
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

pub fn parse_exaggeration(value: &str) -> Result<f64, String> {
    value
        .trim()
        .parse()
        .ok()
        .filter(|exaggeration| (0.1..=100.0).contains(exaggeration))
        .ok_or_else(|| "expected a number from 0.1 to 100".to_owned())
}

fn parse_detail(value: &str) -> Result<u32, String> {
    parse_count(value, &(8..=isotrail_bundle::MAX_GRID_SIDE))
}

/// How terrain and map data are fetched for a GPX file
#[derive(Debug, Args)]
#[command(next_help_heading = crate::DATA)]
pub struct DataArgs {
    /// Grid cells along the slab's long side, from 8 to 1024
    #[arg(long, value_name = "N", default_value_t = DEFAULT_DETAIL.get(), value_parser = parse_detail)]
    detail: u32,

    /// Only use elevation, map and weather data already in the cache
    #[arg(long)]
    offline: bool,

    /// Overpass API server for the OpenStreetMap data
    #[arg(long, value_name = "URL", env = "ISOTRAIL_OVERPASS_URL", default_value = OVERPASS_URL)]
    overpass_url: String,

    /// Elevation tiles from a mirror, as an address with {z}, {x} and {y}
    #[arg(long, value_name = "URL", env = "ISOTRAIL_TERRAIN_URL",
          default_value = TERRARIUM_URL, value_parser = parse_template)]
    terrain_url: String,

    #[command(flatten)]
    weather_url: WeatherUrlArg,

    #[command(flatten)]
    cache_dir: CacheDirArg,

    /// Build the diorama from the elevation alone, without lakes, woods and paths
    #[arg(long)]
    no_map_data: bool,

    /// Ground round the route: percent from 0 to 100, metres from 0 to 10000 [default: 15,500]
    #[arg(long, value_name = "PERCENT[,METRES]", value_parser = parse_padding,
          allow_hyphen_values = true)]
    padding: Option<Padding>,

    /// Which way the slab's sides run: along the route, or north-south and east-west
    #[arg(long, value_enum, value_name = "route|north", default_value_t = FootprintArg::Route,
          hide_possible_values = true)]
    footprint: FootprintArg,

    /// Leave out the weather, with the snow and ice it brings
    #[arg(long)]
    pub no_weather: bool,

    /// Season to show: the hike's own (auto), or a season you choose
    #[arg(long, value_enum, value_name = "auto|spring|summer|autumn|winter",
          default_value_t = SeasonArg::Auto, hide_possible_values = true)]
    pub season: SeasonArg,

    /// Times to keep in the scene: as recorded, rounded, or none
    #[arg(long, value_enum, value_name = "recorded|coarse|none",
          default_value_t = TimesArg::Recorded, hide_possible_values = true)]
    pub times: TimesArg,

    /// Cut this far off the start of the route, such as 300m or 1.5km
    #[arg(long, value_name = "DIST", value_parser = parse_distance)]
    trim_start: Option<f64>,

    /// Cut this far off the end of the route, such as 300m or 1.5km
    #[arg(long, value_name = "DIST", value_parser = parse_distance)]
    trim_end: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TimesArg {
    Recorded,
    Coarse,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Padding {
    fraction: f64,
    min_m: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum FootprintArg {
    Route,
    North,
}

fn parse_padding(value: &str) -> Result<Padding, String> {
    let (percent, metres) = match value.split_once(',') {
        Some((percent, metres)) => (percent, Some(metres)),
        None => (value, None),
    };
    let number = |text: &str, max: f64, what: &str| -> Result<f64, String> {
        text.trim()
            .parse()
            .ok()
            .filter(|number| (0.0..=max).contains(number))
            .ok_or_else(|| format!("expected the {what} as a number from 0 to {max}"))
    };
    Ok(Padding {
        fraction: number(percent, 100.0, "percentage")? / 100.0,
        min_m: match metres {
            Some(metres) => number(metres, 10_000.0, "metres")?,
            None => MIN_PADDING_M,
        },
    })
}

/// Metres, as a number alone or with `m`, or kilometres with `km`
fn parse_distance(value: &str) -> Result<f64, String> {
    let text = value.trim().to_ascii_lowercase();
    let (number, metres) = match text.strip_suffix("km") {
        Some(number) => (number, 1000.0),
        None => (text.strip_suffix('m').unwrap_or(&text), 1.0),
    };
    number
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite() && *number >= 0.0)
        .map(|number| number * metres)
        .ok_or_else(|| "expected a distance such as 300, 300m or 1.5km".to_owned())
}

/// A distance as the docs write it: 450m, 2.84km
fn distance_text(metres: f64) -> String {
    if metres < 999.5 {
        format!("{metres:.0}m")
    } else {
        let km = format!("{:.2}", metres / 1000.0);
        format!("{}km", km.trim_end_matches('0').trim_end_matches('.'))
    }
}

impl DataArgs {
    /// The track with --trim-start and --trim-end cut off, or a usage error
    /// naming its length when nothing is left
    fn trim<'a>(&self, track: &'a Track) -> anyhow::Result<Cow<'a, Track>> {
        let (start, end) = (self.trim_start.unwrap_or(0.0), self.trim_end.unwrap_or(0.0));
        if start == 0.0 && end == 0.0 {
            return Ok(Cow::Borrowed(track));
        }
        track.trimmed(start, end).map(Cow::Owned).ok_or_else(|| {
            let cuts: Vec<String> = [(start, "start"), (end, "end")]
                .into_iter()
                .filter(|&(metres, _)| metres > 0.0)
                .map(|(metres, from)| format!("{} from the {from}", distance_text(metres)))
                .collect();
            let length = track.stats().map_or(0.0, |stats| stats.distance_m);
            usage(format!(
                "trimming {} leaves none of the route, which is {} long; trim less",
                cuts.join(" and "),
                distance_text(length)
            ))
        })
    }

    /// The padding as `--padding` takes it, such as 15,500
    fn padding_text(&self) -> String {
        let padding = self.padding.unwrap_or(Padding {
            fraction: PADDING_FRACTION,
            min_m: MIN_PADDING_M,
        });
        let round = |value: f64| (value * 1e6).round() / 1e6;
        format!(
            "{},{}",
            round(padding.fraction * 100.0),
            round(padding.min_m)
        )
    }

    fn footprint(&self) -> FootprintOptions {
        let padding = self.padding.unwrap_or(Padding {
            fraction: PADDING_FRACTION,
            min_m: MIN_PADDING_M,
        });
        FootprintOptions {
            padding_fraction: padding.fraction,
            min_padding_m: padding.min_m,
            orientation: match self.footprint {
                FootprintArg::Route => Orientation::Route,
                FootprintArg::North => Orientation::North,
            },
        }
    }

    /// Warns about flags typed for fetching data, which a bundle doesn't need
    pub fn warn_unused_with_bundle(&self, typed: &Typed) {
        for (id, flag) in [
            ("detail", "--detail"),
            ("offline", "--offline"),
            ("overpass_url", "--overpass-url"),
            ("terrain_url", "--terrain-url"),
            ("weather_url", "--weather-url"),
            ("cache_dir", "--cache-dir"),
            ("no_map_data", "--no-map-data"),
            ("padding", "--padding"),
            ("footprint", "--footprint"),
        ] {
            if typed.has(id) {
                warn!(
                    "{flag} does nothing with a scene bundle, which already holds its terrain and map data"
                );
            }
        }
    }

    /// A scene bundle keeps the times and route it was made with, so a
    /// privacy option for them is refused rather than dropped
    pub fn refuse_with_bundle(&self) -> anyhow::Result<()> {
        for (trim, flag) in [
            (self.trim_start, "--trim-start"),
            (self.trim_end, "--trim-end"),
        ] {
            if trim.is_some_and(|metres| metres > 0.0) {
                return Err(usage(format!(
                    "{flag} can't cut the route a scene bundle holds; give the GPX file instead"
                )));
            }
        }
        let times = match self.times {
            TimesArg::Recorded => return Ok(()),
            TimesArg::Coarse => "coarse",
            TimesArg::None => "none",
        };
        Err(usage(format!(
            "--times {times} can't change the times a scene bundle holds; give the GPX file instead"
        )))
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

impl DataArgs {
    /// `http` behind the cache, announcing its first download
    fn cached(&self, http: HttpFetcher) -> anyhow::Result<CachedFetcher<Announced<HttpFetcher>>> {
        let cache = Cache::new(crate::cache::dir(self.cache_dir.get())?);
        let announced = Announced {
            inner: http,
            line: report::download_line(
                !self.no_map_data,
                !self.no_weather && self.times != TimesArg::None,
            ),
        };
        Ok(CachedFetcher::new(announced, cache).offline(self.offline))
    }
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
    let fetcher = data.cached(HttpFetcher::new()?.with_retry(RetryPolicy::once()))?;
    let features = OverpassSource::new(&fetcher)
        .endpoint(&data.overpass_url)
        .features(grid)
        .map_err(|err| {
            if err.is_offline() {
                not_cached(
                    "map data",
                    &data.overpass_url,
                    OVERPASS_URL,
                    ", or add --no-map-data",
                )
            } else {
                anyhow::Error::new(err).context(MAP_DATA_WAYS_OUT)
            }
        })?;
    for (tag, count) in features.counts() {
        info!("{count:>6}  {tag}");
    }
    Ok(Some(features))
}

/// The next step when --offline finds nothing cached; a server other than
/// the usual one is named, without any key in its address
fn not_cached(what: &str, url: &str, usual: &str, more: &str) -> anyhow::Error {
    let from = if url == usual {
        String::new()
    } else {
        format!(" from {}", origin(url))
    };
    anyhow::anyhow!(
        "the {what}{from} for this area isn't in the cache yet; run once without --offline \
         to download it{more}"
    )
}

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

/// Fetches terrain, map data and weather around the track read from `file`
/// and packs them into a bundle
pub fn prepare(track: &Track, file: &Path, data: &DataArgs) -> anyhow::Result<Prepared> {
    let trimmed = data.trim(track)?;
    let track = trimmed.as_ref();
    let footprint = Footprint::from_track_with(track, &data.footprint()).map_err(|error| {
        if matches!(error, GeoError::NoGround) {
            usage(format!(
                "the route stays on one spot, so --padding {} leaves no ground to draw; \
                 give it some metres, such as --padding 15,500",
                data.padding_text()
            ))
        } else {
            error.into()
        }
    })?;
    let detail = NonZeroU32::new(data.detail).context("--detail must be positive")?;
    let grid = Grid::new(&footprint, detail);

    let fetcher = data.cached(HttpFetcher::new()?)?;
    let tiles = TerrariumSource::<HttpFetcher>::tiles(&grid).len();
    let mut heightmap = TerrariumSource::new(&fetcher)
        .template(&data.terrain_url)
        .heightmap(&grid)
        .map_err(|err| {
            if is_offline(&err) {
                not_cached("elevation data", &data.terrain_url, TERRARIUM_URL, "")
            } else {
                anyhow::Error::new(err)
            }
        })?;
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

    // Weather is asked for the hours the scene will hold
    let timed = match data.times {
        TimesArg::Recorded => Some(Cow::Borrowed(track)),
        TimesArg::Coarse => Some(Cow::Owned(track.coarse_times())),
        TimesArg::None => {
            warn!(
                "--times none: leaving out the date and times, with the light, weather and season that follow from them"
            );
            None
        }
    };
    let server = data.weather_url.server();
    let weather = match &timed {
        Some(timed) if !data.no_weather => weather::look_up(timed, &fetcher, &server),
        _ => None,
    };

    let provenance = Provenance {
        generator: format!("isotrail {}", env!("CARGO_PKG_VERSION")),
        elevation: format!("{} at zoom {}", redact(&data.terrain_url), grid.dem_zoom()),
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
    // Built from the recorded times first, so the seed and the trees it
    // places don't depend on --times
    match &timed {
        Some(Cow::Owned(timed)) => retime(&mut bundle, timed),
        Some(Cow::Borrowed(_)) => {}
        None => untime(&mut bundle),
    }
    bundle.weather = weather.map(|(weather, _)| weather);
    if let Some(timed) = &timed {
        season::estimate(&mut bundle, timed, &fetcher, &server);
    }
    season::apply(&mut bundle, data.season);
    bundle.validate().map_err(|error| match error {
        BundleError::Invalid(reason) => {
            anyhow::anyhow!(
                "the scene made from {} can't be drawn: {reason}",
                file.display()
            )
        }
        error => error.into(),
    })?;
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

    #[test]
    fn trims_are_metres_or_kilometres() {
        for (text, metres) in [
            ("300", 300.0),
            ("300m", 300.0),
            (" 1.5km", 1500.0),
            ("2 KM", 2000.0),
            ("0", 0.0),
        ] {
            assert_eq!(parse_distance(text), Ok(metres), "{text}");
        }
        for bad in ["", "-5", "km", "1.5mi", "nan", "inf", "300 metres"] {
            assert!(parse_distance(bad).is_err(), "{bad}");
        }
        assert_eq!(distance_text(450.2), "450m");
        assert_eq!(distance_text(999.7), "1km");
        assert_eq!(distance_text(2836.0), "2.84km");
        assert_eq!(distance_text(1500.0), "1.5km");
    }

    #[test]
    fn padding_is_a_percentage_and_maybe_metres() {
        assert_eq!(
            parse_padding("10"),
            Ok(Padding {
                fraction: 0.1,
                min_m: MIN_PADDING_M
            })
        );
        assert_eq!(
            parse_padding("0, 250"),
            Ok(Padding {
                fraction: 0.0,
                min_m: 250.0
            })
        );
        for bad in ["", "x", "101", "-1", "10,", "10,20000", "10,5,5"] {
            assert!(parse_padding(bad).is_err(), "{bad}");
        }
    }
}
