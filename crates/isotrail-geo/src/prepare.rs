use std::borrow::Cow;
use std::num::NonZeroU32;

use isotrail_bundle::{BundleError, SceneBundle, Season, Weather, WeatherDataset};
use isotrail_track::Track;
use tracing::info;

use crate::{
    DEFAULT_DETAIL, DemError, DemSource, FeatureSource, Fetcher, Footprint, FootprintOptions,
    GeoError, Grid, Heightmap, LandcoverMap, OSM_ATTRIBUTION, OVERPASS_URL, OpenMeteoSource,
    OsmError, OsmFeatures, OverpassSource, Provenance, Request, RequestError, SeasonKind,
    TERRARIUM_URL, TerrariumSource, WeatherError, WeatherRequest, WeatherServer, WeatherSource,
    forced_season, ice_line, line_mask, origin, redact, retime, scene_bundle, snow_line,
    snow_readings, untime,
};

/// What a scene is made with, besides the track
#[derive(Debug, Clone, PartialEq)]
pub struct SceneOptions {
    /// Grid cells along the slab's long side
    pub detail: NonZeroU32,
    pub footprint: FootprintOptions,
    /// False builds the terrain from the elevation alone
    pub map_data: bool,
    pub weather: bool,
    /// None keeps the hike's own season
    pub season: Option<SeasonKind>,
    pub times: Times,
    pub trim_start_m: f64,
    pub trim_end_m: f64,
    /// Elevation tile address with `{z}`, `{x}` and `{y}`
    pub terrain_url: String,
    pub overpass_url: String,
    pub weather_server: WeatherServer,
}

impl Default for SceneOptions {
    fn default() -> Self {
        SceneOptions {
            detail: DEFAULT_DETAIL,
            footprint: FootprintOptions::default(),
            map_data: true,
            weather: true,
            season: None,
            times: Times::Recorded,
            trim_start_m: 0.0,
            trim_end_m: 0.0,
            terrain_url: TERRARIUM_URL.to_owned(),
            overpass_url: OVERPASS_URL.to_owned(),
            weather_server: WeatherServer::default(),
        }
    }
}

/// The times a scene keeps
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Times {
    #[default]
    Recorded,
    /// The start rounded to the half hour and the length to the quarter hour
    Coarse,
    None,
}

/// Where the data comes from. Overpass retries a busy server by itself, so
/// its fetcher may be one that doesn't
#[derive(Clone, Copy)]
pub struct Fetchers<'a> {
    /// Elevation tiles and weather
    pub data: &'a dyn Fetcher,
    pub map_data: &'a dyn Fetcher,
}

/// A stage of the pipeline, as it starts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Elevation { tiles: usize },
    MapData,
    Weather,
    Season,
}

/// Something left out of the scene, which still gets made
#[derive(Debug)]
pub enum Warning {
    /// Map data was turned off
    MapDataSkipped,
    /// Times were turned off, with the light, weather and season they give
    TimesLeftOut,
    /// The track can't be asked about, so the weather is left out
    NoWeatherRequest(RequestError),
    /// The weather is left out
    Weather(WeatherError),
    /// The weather of the days before is missing, so lakes are left open
    DaysBefore(WeatherError),
}

/// Hears about the pipeline's stages and warnings
pub trait Progress {
    fn step(&mut self, _step: Step) {}
    fn warn(&mut self, warning: Warning);
}

#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    #[error("trimming leaves none of the route, which is {length_m:.0}m long")]
    TrimmedAway { length_m: f64 },

    #[error("the route stays on one spot, so the padding leaves no ground to draw")]
    NoGround,

    #[error(transparent)]
    Footprint(GeoError),

    #[error(transparent)]
    Elevation(DemError),

    #[error(transparent)]
    MapData(OsmError),

    #[error(transparent)]
    Scene(GeoError),

    #[error("the scene can't be drawn: {0}")]
    Invalid(String),

    #[error(transparent)]
    Bundle(BundleError),
}

/// A scene bundle and what went into it
pub struct Prepared {
    pub bundle: SceneBundle,
    pub grid: Grid,
    pub heightmap: Heightmap,
    pub landcover: LandcoverMap,
    /// None without map data
    pub features: Option<OsmFeatures>,
    pub tiles: usize,
    pub elevation_range: (f32, f32),
}

/// Fetches terrain, map data and weather around the track and packs them
/// into a scene bundle. `now` is Unix seconds, which picks the weather's
/// archive or forecast
pub fn prepare(
    track: &Track,
    title: &str,
    options: &SceneOptions,
    fetchers: Fetchers<'_>,
    now: i64,
    progress: &mut dyn Progress,
) -> Result<Prepared, SceneError> {
    let (trimmed, grid) = slab(track, options)?;
    let track = trimmed.as_ref();

    let tiles = TerrariumSource::<&dyn Fetcher>::tiles(&grid).len();
    progress.step(Step::Elevation { tiles });
    let mut heightmap = TerrariumSource::new(fetchers.data)
        .template(&options.terrain_url)
        .heightmap(&grid)
        .map_err(SceneError::Elevation)?;
    let features = if options.map_data {
        progress.step(Step::MapData);
        let features = OverpassSource::new(fetchers.map_data)
            .endpoint(&options.overpass_url)
            .features(&grid)
            .map_err(SceneError::MapData)?;
        for (tag, count) in features.counts() {
            info!("{count:>6}  {tag}");
        }
        Some(features)
    } else {
        progress.warn(Warning::MapDataSkipped);
        None
    };
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
    let timed = timed(track, options.times);
    if timed.is_none() {
        progress.warn(Warning::TimesLeftOut);
    }
    let server = &options.weather_server;
    let weather = match &timed {
        Some(timed) if options.weather => {
            progress.step(Step::Weather);
            look_up_weather(timed, fetchers.data, server, now, progress)
        }
        _ => None,
    };

    let provenance = Provenance {
        generator: format!("isotrail {}", env!("CARGO_PKG_VERSION")),
        elevation: format!(
            "{} at zoom {}",
            redact(&options.terrain_url),
            grid.dem_zoom()
        ),
        map_data: match features {
            Some(_) => format!("OpenStreetMap via {}", origin(&options.overpass_url)),
            None => "none".to_owned(),
        },
        weather: weather.as_ref().map(|(_, provenance)| provenance.clone()),
    };
    let mut bundle = scene_bundle(track, title, &grid, &heightmap, &landcover, provenance)
        .map_err(SceneError::Scene)?;
    if features.is_none() {
        bundle
            .meta
            .attribution
            .retain(|credit| credit != OSM_ATTRIBUTION);
    }
    // Built from the recorded times first, so the seed and the trees it
    // places don't depend on the times kept
    match &timed {
        Some(Cow::Owned(timed)) => retime(&mut bundle, timed),
        Some(Cow::Borrowed(_)) => {}
        None => untime(&mut bundle),
    }
    bundle.weather = weather.map(|(weather, _)| weather);
    if let Some(timed) = &timed
        && bundle.weather.is_some()
    {
        progress.step(Step::Season);
        snow_and_ice(&mut bundle, timed, fetchers.data, server, now, progress);
    }
    if let Some(kind) = options.season {
        bundle.season = forced_season(kind, &bundle);
    }
    bundle.validate().map_err(|error| match error {
        BundleError::Invalid(reason) => SceneError::Invalid(reason),
        error => SceneError::Bundle(error),
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

/// What [`prepare`] fetches first, so it can all be fetched at once
/// beforehand. A run may still ask for more: the forecast where the archive
/// has gaps, or an answer that couldn't be read
pub fn requests(
    track: &Track,
    options: &SceneOptions,
    now: i64,
) -> Result<Vec<Request>, SceneError> {
    let (trimmed, grid) = slab(track, options)?;
    let mut requests: Vec<Request> = TerrariumSource::<&dyn Fetcher>::tiles(&grid)
        .into_iter()
        .map(|tile| {
            Request::get(TerrariumSource::<&dyn Fetcher>::url_from(
                &options.terrain_url,
                tile,
            ))
        })
        .collect();
    if options.map_data {
        requests.push(Request::post(
            options.overpass_url.clone(),
            OverpassSource::<&dyn Fetcher>::request_body(&grid),
        ));
    }
    if options.weather
        && let Some(timed) = timed(&trimmed, options.times)
        && let Ok(request) = WeatherRequest::from_track(&timed, now)
    {
        let urls = options.weather_server.first_urls(&request);
        requests.extend(urls.into_iter().map(Request::get));
    }
    Ok(requests)
}

/// The kept part of the track and the grid around it
fn slab<'a>(
    track: &'a Track,
    options: &SceneOptions,
) -> Result<(Cow<'a, Track>, Grid), SceneError> {
    let trimmed = trim(track, options)?;
    let footprint =
        Footprint::from_track_with(&trimmed, &options.footprint).map_err(|error| match error {
            GeoError::NoGround => SceneError::NoGround,
            error => SceneError::Footprint(error),
        })?;
    let grid = Grid::new(&footprint, options.detail);
    Ok((trimmed, grid))
}

/// The track with the times the scene keeps, if it keeps any
fn timed(track: &Track, times: Times) -> Option<Cow<'_, Track>> {
    match times {
        Times::Recorded => Some(Cow::Borrowed(track)),
        Times::Coarse => Some(Cow::Owned(track.coarse_times())),
        Times::None => None,
    }
}

fn trim<'a>(track: &'a Track, options: &SceneOptions) -> Result<Cow<'a, Track>, SceneError> {
    let (start, end) = (options.trim_start_m, options.trim_end_m);
    if start == 0.0 && end == 0.0 {
        return Ok(Cow::Borrowed(track));
    }
    track
        .trimmed(start, end)
        .map(Cow::Owned)
        .ok_or_else(|| SceneError::TrimmedAway {
            length_m: track.stats().map_or(0.0, |stats| stats.distance_m),
        })
}

/// The weather during the hike and where it came from, or a warning and
/// nothing when it can't be found
pub fn look_up_weather(
    track: &Track,
    fetcher: impl Fetcher,
    server: &WeatherServer,
    now: i64,
    progress: &mut dyn Progress,
) -> Option<(Weather, String)> {
    let request = match WeatherRequest::from_track(track, now) {
        Ok(request) => request,
        Err(error) => {
            progress.warn(Warning::NoWeatherRequest(error));
            return None;
        }
    };
    match OpenMeteoSource::new(fetcher)
        .server(server)
        .weather(&request)
    {
        Ok(weather) => {
            let places = request.locations.len();
            let provenance = format!(
                "Open-Meteo {} at {places} location{}",
                dataset_name(weather.dataset),
                if places == 1 { "" } else { "s" }
            );
            Some((weather, provenance))
        }
        Err(error) => {
            progress.warn(Warning::Weather(error));
            None
        }
    }
}

pub fn dataset_name(dataset: WeatherDataset) -> &'static str {
    match dataset {
        WeatherDataset::Archive => "archive",
        WeatherDataset::Forecast => "forecast",
    }
}

/// Snow from the weather during the hike, and lakes frozen by the frost of
/// the days before; when the days before can't be found, it warns and
/// leaves the lakes open
fn snow_and_ice(
    bundle: &mut SceneBundle,
    track: &Track,
    fetcher: &dyn Fetcher,
    server: &WeatherServer,
    now: i64,
    progress: &mut dyn Progress,
) {
    let Some(weather) = &bundle.weather else {
        return;
    };
    let Ok(request) = WeatherRequest::from_track(track, now) else {
        return;
    };
    let before = OpenMeteoSource::new(fetcher)
        .server(server)
        .days_before(&request)
        .unwrap_or_else(|error| {
            progress.warn(Warning::DaysBefore(error));
            Vec::new()
        });
    let route = &bundle.route.points;
    let walked = route.iter().map(|p| p.draped_m).sum::<f32>() / route.len().max(1) as f32;
    let readings = snow_readings(weather, &request, &before, walked);
    let terrain = &bundle.terrain;
    let top = (0..terrain.heights.len())
        .map(|cell| terrain.height_m(cell))
        .fold(f32::MIN, f32::max);
    let highest_lake = bundle
        .water
        .iter()
        .filter(|body| !body.sea)
        .map(|body| body.level_m)
        .fold(f32::MIN, f32::max);
    bundle.season.snow_line_m = snow_line(&readings, &before).filter(|&line| line < top);
    bundle.season.ice_line_m = ice_line(&before).filter(|&line| line <= highest_lake);
}

/// Snow and ice come from the weather, so they go with it
pub fn without_weather(season: &mut Season) {
    season.snow_line_m = None;
    season.ice_line_m = None;
}
