use std::fmt::Display;
use std::path::Path;

use isotrail_geo::{
    Answer, Fetchers, Progress, ReplayFetcher, Request, RequestFailure, SceneOptions, Warning,
    prepare, requests,
};
use isotrail_options::Settings;
use isotrail_track::{GpxReader, Track, TrackReader};
use wasm_bindgen::prelude::*;

/// Reads the time zone boundaries, tzf-dist's `lite.tzb`. Every build needs
/// them, since the zone sets the hike's date
#[wasm_bindgen(js_name = loadTimezones)]
pub fn load_timezones(tzb: &[u8]) -> Result<(), JsError> {
    console_error_panic_hook::set_once();
    isotrail_track::load_timezones(tzb)
        .map_err(|error| JsError::new(&format!("can't read the time zones: {error}")))
}

/// A request whose answer the build needs
#[wasm_bindgen]
pub struct Wanted {
    request: Request,
}

#[wasm_bindgen]
impl Wanted {
    /// The key `hold` takes, which is also the command line's cache key
    #[wasm_bindgen(getter)]
    pub fn key(&self) -> String {
        self.request.key()
    }

    #[wasm_bindgen(getter)]
    pub fn method(&self) -> String {
        self.request.method().to_owned()
    }

    #[wasm_bindgen(getter)]
    pub fn url(&self) -> String {
        self.request.url.clone()
    }

    /// A POST's form body, none for a GET
    #[wasm_bindgen(getter)]
    pub fn body(&self) -> Option<Vec<u8>> {
        self.request.body.clone()
    }
}

/// Builds a scene file from a GPX file, as `isotrail bundle` does. The
/// answers come from the page: `requests` lists what to fetch first, and
/// `build` either makes the scene or lists in `missing` what it still needs
#[wasm_bindgen]
pub struct SceneBuilder {
    track: Track,
    /// From the file, as the scene is built with it
    file_title: String,
    title: Option<String>,
    exaggeration: Option<f64>,
    options: SceneOptions,
    now: i64,
    replay: ReplayFetcher,
    missing: Vec<Request>,
    warnings: Vec<String>,
}

#[wasm_bindgen]
impl SceneBuilder {
    /// `gpx` is the file's bytes and `name` its name, which gives the title
    /// when the track has none. `options` is a JSON object keyed by the long
    /// flag names of `isotrail render`, such as `{"times": "coarse"}`. `now`
    /// is in seconds since 1970 and picks the weather's archive or forecast
    #[wasm_bindgen(constructor)]
    pub fn new(gpx: &[u8], name: &str, options: &str, now: f64) -> Result<SceneBuilder, JsError> {
        console_error_panic_hook::set_once();
        if !isotrail_track::timezones_loaded() {
            return Err(JsError::new(
                "the time zones aren't loaded: call loadTimezones first",
            ));
        }
        let render = Settings::from_json(options, "the options")
            .and_then(|settings| settings.options())
            .map_err(error)?;
        if render.data.offline {
            return Err(JsError::new(
                "offline in the options: a page has no cache folder to build from",
            ));
        }
        if render.data.cache_dir.get().is_some() {
            return Err(JsError::new(
                "cache-dir in the options: a page has no cache folder",
            ));
        }
        let track = GpxReader.parse(gpx).and_then(Track::clean).map_err(error)?;
        Ok(SceneBuilder {
            file_title: track.title(Path::new(name)),
            title: render.overlay.given_title().map(str::to_owned),
            exaggeration: render.exaggeration,
            options: render.data.options(None),
            track,
            now: now as i64,
            replay: ReplayFetcher::new(),
            missing: Vec::new(),
            warnings: Vec::new(),
        })
    }

    /// What the build asks for first and isn't held yet, so it can all be
    /// fetched at once
    pub fn requests(&self) -> Result<Vec<Wanted>, JsError> {
        let all = requests(&self.track, &self.options, self.now).map_err(error)?;
        Ok(all
            .into_iter()
            .filter(|request| !self.replay.holds(request))
            .map(|request| Wanted { request })
            .collect())
    }

    /// Holds the answer to the request with this key: its HTTP status and body
    pub fn hold(&self, key: String, status: u16, body: Vec<u8>) {
        let answer = if (200..300).contains(&status) {
            Answer::Body(body)
        } else {
            Answer::Status { status, body }
        };
        self.replay.hold(key, answer);
    }

    /// Holds a request that got no answer at all
    pub fn failed(&self, key: String, message: String, timed_out: bool) {
        let failure = RequestFailure { message, timed_out };
        self.replay.hold(key, Answer::Failed(failure));
    }

    /// The scene file's bytes, or nothing while an answer is missing
    pub fn build(&mut self) -> Result<Option<Vec<u8>>, JsError> {
        let mut heard = Heard::default();
        let fetchers = Fetchers {
            data: &self.replay,
            map_data: &self.replay,
        };
        let built = prepare(
            &self.track,
            &self.file_title,
            &self.options,
            fetchers,
            self.now,
            &mut heard,
        );
        self.missing = self.replay.take_missed();
        if !self.missing.is_empty() {
            return Ok(None);
        }
        let mut bundle = built.map_err(error)?.bundle;
        if let Some(exaggeration) = self.exaggeration {
            bundle.grid.exaggeration = exaggeration;
        }
        if let Some(title) = &self.title {
            bundle.meta.title.clone_from(title);
        }
        self.warnings = heard.0;
        bundle.to_bytes().map(Some).map_err(error)
    }

    /// The requests the last build stopped at
    #[wasm_bindgen(getter)]
    pub fn missing(&self) -> Vec<Wanted> {
        self.missing
            .iter()
            .map(|request| Wanted {
                request: request.clone(),
            })
            .collect()
    }

    /// What the scene left out, by name, such as `weather`
    #[wasm_bindgen(getter)]
    pub fn warnings(&self) -> Vec<String> {
        self.warnings.clone()
    }
}

#[derive(Default)]
struct Heard(Vec<String>);

impl Progress for Heard {
    fn warn(&mut self, warning: Warning) {
        let name = match warning {
            Warning::MapDataSkipped => "map-data-skipped",
            Warning::TimesLeftOut => "times-left-out",
            Warning::NoWeatherRequest(_) => "no-weather-request",
            Warning::Weather(_) => "weather",
            Warning::DaysBefore(_) => "days-before",
        };
        self.0.push(name.to_owned());
    }
}

fn error(error: impl Display) -> JsError {
    JsError::new(&format!("{error:#}"))
}
