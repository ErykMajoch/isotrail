use std::collections::HashMap;
use std::error::Error;
use std::fmt::Display;
use std::path::Path;
use std::time::Duration;

use isotrail_geo::{
    Answer, BoundingBox, DemError, FetchError, Fetchers, GeoError, OVERPASS_URL, Progress,
    ReplayFetcher, Request, Retry, RetryPolicy, SceneError, SceneOptions, UNREACHABLE_RETRY,
    Warning, origin, plan, prepare,
};
use isotrail_options::data::distance_text;
use isotrail_options::plain;
use isotrail_track::{GpxReader, Track, TrackReader, media_kind};

use crate::notes::heard_while;
use crate::page::read_options;
use crate::{SERVER_NAMES, map_data_servers, server_name};

/// The largest slab a page fetches map data for, in square kilometres. Map
/// data grows with the area, and a browser holds the whole answer at once
pub const MAX_MAP_DATA_KM2: f64 = 1000.0;

/// How long a page waits before asking a busy Overpass server again. Each
/// answer can take a minute already, so a page gives up sooner than the
/// command line
pub const MAP_DATA_BUSY_WAITS: [Duration; 2] = [Duration::from_secs(5), Duration::from_secs(15)];

const WITHOUT_MAP_DATA: &str = "build the diorama without lakes, woods and paths";
const WAYS_OUT: &str = "try again later, or build the diorama without lakes, woods and paths";

/// What a request fetches
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Elevation,
    MapData,
    Weather,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Elevation => "elevation",
            Kind::MapData => "map-data",
            Kind::Weather => "weather",
        }
    }
}

/// Why a scene can't be built, as a short reason a page can act on and a
/// message to show
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub reason: &'static str,
    pub message: String,
}

impl Failure {
    pub(crate) fn new(reason: &'static str, message: impl Into<String>) -> Failure {
        Failure {
            reason,
            message: message.into(),
        }
    }

    /// No network at all, which only the page can tell
    pub fn offline() -> Failure {
        Failure::new(
            "offline",
            "there's no network connection; connect to the internet and try again",
        )
    }
}

/// A pause before asking again for answers that failed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wait {
    pub kind: Kind,
    pub retry: Retry,
    /// The attempt that follows the pause, counting the first
    pub attempt: usize,
    pub attempts: usize,
    pub delay: Duration,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Scene(Vec<u8>),
    /// Answers are missing: fetch [`Builder::missing`] and build again
    Missing,
    /// Answers failed and are worth another try after the pause: fetch
    /// [`Builder::missing`] then
    Wait(Wait),
}

/// The pauses before each further attempt at a request of this kind
pub fn waits(kind: Kind, retry: Retry) -> Vec<Duration> {
    let policy = match (kind, retry) {
        (Kind::MapData, Retry::Busy) => return MAP_DATA_BUSY_WAITS.to_vec(),
        (Kind::MapData, Retry::Unreachable) => UNREACHABLE_RETRY,
        _ => RetryPolicy::default(),
    };
    (1..policy.max_attempts)
        .map(|attempt| policy.delay(attempt - 1, None))
        .collect()
}

/// Builds a scene file from a GPX file against answers fetched by someone
/// else, as `isotrail bundle` builds one against the network
pub struct Builder {
    track: Track,
    shown: String,
    file_title: String,
    title: Option<String>,
    exaggeration: Option<f64>,
    options: SceneOptions,
    padding: String,
    now: i64,
    plan: Vec<Request>,
    replay: ReplayFetcher,
    missing: Vec<Request>,
    warnings: Vec<String>,
    /// What reading the track warned about, then what the scene left out
    read_notes: Vec<String>,
    notes: Vec<String>,
    /// Further attempts made at each request, by key
    retried: HashMap<String, usize>,
    /// The Overpass servers asked in turn, and the one asked now
    servers: Vec<String>,
    server: usize,
    /// True when the options name their own Overpass server
    typed_server: bool,
    /// Rounds of the servers that all failed
    rounds: usize,
    dropped: Vec<String>,
}

impl Builder {
    /// `gpx` is the file's bytes and `name` its name. `options` is a JSON
    /// object keyed by the long flag names of `isotrail render`, and `now`
    /// is in seconds since 1970. A slab too large for map data is refused
    /// here, before anything is fetched
    pub fn new(gpx: &[u8], name: &str, options: &str, now: i64) -> Result<Builder, Failure> {
        let shown = if name.is_empty() { "the file" } else { name };
        if let Some(kind) = media_kind(gpx) {
            return Err(Failure::new(
                "input",
                format!("{shown} is {kind}, not a GPX file"),
            ));
        }
        if isotrail_bundle::is_bundle(gpx) {
            return Err(Failure::new(
                "input",
                format!("{shown} is already a scene file, not a GPX file"),
            ));
        }
        let render = read_options(options)?;
        let (track, read_notes) = heard_while(|| GpxReader.parse(gpx).and_then(Track::clean));
        let track = track.map_err(|error| {
            Failure::new("input", format!("can't read {shown}: {}", chain(&error)))
        })?;
        let mut builder = Builder {
            shown: shown.to_owned(),
            file_title: track.title(Path::new(name)),
            title: render.overlay.given_title().map(str::to_owned),
            exaggeration: render.exaggeration,
            options: render.data.options(None),
            padding: render.data.padding_text(),
            track,
            now,
            plan: Vec::new(),
            replay: ReplayFetcher::new(),
            missing: Vec::new(),
            warnings: Vec::new(),
            read_notes,
            notes: Vec::new(),
            retried: HashMap::new(),
            servers: Vec::new(),
            server: 0,
            typed_server: render.data.overpass_url != OVERPASS_URL,
            rounds: 0,
            dropped: Vec::new(),
        };
        let plan =
            plan(&builder.track, &builder.options, now).map_err(|error| builder.failure(&error))?;
        let long_m = f64::from(plan.grid.cols) * plan.grid.cell_size_m;
        let short_m = f64::from(plan.grid.rows) * plan.grid.cell_size_m;
        if builder.options.map_data && long_m * short_m / 1e6 > MAX_MAP_DATA_KM2 {
            return Err(Failure::new(
                "too-large",
                format!(
                    "the land around this route is {:.0} x {:.0}km, too large to fetch map data for \
                     in a browser; build the diorama without lakes, woods and paths, or use the \
                     isotrail command",
                    long_m / 1000.0,
                    short_m / 1000.0
                ),
            ));
        }
        builder.servers = if builder.typed_server {
            vec![builder.options.overpass_url.clone()]
        } else {
            map_data_servers(&BoundingBox::around(&plan.grid))
        };
        builder.plan = plan.requests;
        builder.switch_to(0);
        Ok(builder)
    }

    /// Asks these Overpass servers in turn instead of the built-in ones, unless
    /// the options name their own. Called before anything is fetched
    pub fn use_servers(&mut self, servers: Vec<String>) {
        if self.typed_server || servers.is_empty() {
            return;
        }
        self.servers = servers;
        self.switch_to(0);
    }

    /// Leaves out the built-in Overpass servers with these names, keeping the
    /// others in their order. Called before anything is fetched. Options that
    /// name their own server keep it. Refused when no server is left to ask
    /// for map data
    pub fn skip_servers(&mut self, skip: &[String]) -> Result<(), Failure> {
        if let Some(unknown) = skip
            .iter()
            .find(|name| !SERVER_NAMES.contains(&name.as_str()))
        {
            return Err(Failure::new(
                "usage",
                format!(
                    "there's no Overpass server called {unknown} to leave out; use {}, {} or {}",
                    SERVER_NAMES[0], SERVER_NAMES[1], SERVER_NAMES[2]
                ),
            ));
        }
        if self.typed_server {
            return Ok(());
        }
        let kept: Vec<String> = self
            .servers
            .iter()
            .filter(|url| server_name(url).is_none_or(|name| !skip.iter().any(|s| s == name)))
            .cloned()
            .collect();
        if kept.is_empty() {
            if !self.options.map_data {
                return Ok(());
            }
            return Err(Failure::new(
                "no-server",
                format!(
                    "none of the Overpass servers left can fetch the map data here; allow another, \
                     or {WITHOUT_MAP_DATA}"
                ),
            ));
        }
        self.servers = kept;
        self.switch_to(0);
        Ok(())
    }

    /// The Overpass servers asked in turn
    pub fn servers(&self) -> &[String] {
        &self.servers
    }

    /// The Overpass server asked now
    pub fn server(&self) -> &str {
        &self.servers[self.server]
    }

    /// The keys of a map data request at the other servers. An answer kept
    /// under any of them serves as well, and is held under its own key
    pub fn aliases(&self, request: &Request) -> Vec<String> {
        let Some(body) = &request.body else {
            return Vec::new();
        };
        self.servers
            .iter()
            .filter(|server| **server != request.url)
            .map(|server| Request::post(server.clone(), body.clone()).key())
            .collect()
    }

    /// The keys whose answers the last build threw away, so a page drops its
    /// copies of them too
    pub fn dropped(&self) -> &[String] {
        &self.dropped
    }

    /// What the build asks for first and isn't held yet
    pub fn requests(&self) -> Vec<Request> {
        self.plan
            .iter()
            .filter(|request| !self.replay.holds(request))
            .cloned()
            .collect()
    }

    /// Holds the answer to the request with this key. A map data answer from
    /// another of the servers makes that server the one asked
    pub fn hold(&mut self, key: String, answer: Answer) {
        if matches!(answer, Answer::Body(_))
            && let Some(body) = self.map_request().and_then(|request| request.body)
            && let Some(server) = self
                .servers
                .iter()
                .position(|server| Request::post(server.clone(), body.clone()).key() == key)
            && server != self.server
        {
            self.switch_to(server);
        }
        self.replay.hold(key, answer);
    }

    /// The requests the last build stopped at
    pub fn missing(&self) -> &[Request] {
        &self.missing
    }

    /// What the scene left out, by name, such as `weather`
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// What the command line would warn about while it reads the track and
    /// builds the scene, each once, worded for a page
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    pub fn kind(&self, request: &Request) -> Kind {
        let terrain = self
            .options
            .terrain_url
            .split('{')
            .next()
            .unwrap_or_default();
        if request.body.is_some() {
            Kind::MapData
        } else if request.url.starts_with(terrain) {
            Kind::Elevation
        } else {
            Kind::Weather
        }
    }

    /// Builds the scene from the answers held, or says what to fetch first
    pub fn build(&mut self) -> Result<Outcome, Failure> {
        self.dropped.clear();
        let mut heard = Heard::default();
        let fetchers = Fetchers {
            data: &self.replay,
            map_data: &self.replay,
            waits: false,
        };
        let (built, built_notes) = heard_while(|| {
            prepare(
                &self.track,
                &self.file_title,
                &self.options,
                fetchers,
                self.now,
                &mut heard,
            )
        });
        let missed = self.replay.take_missed();
        let again: Vec<(Request, Retry)> = self
            .replay
            .take_failed()
            .into_iter()
            .filter(|(request, _)| self.kind(request) != Kind::MapData)
            .collect();
        if !missed.is_empty() {
            self.missing = missed;
            return Ok(Outcome::Missing);
        }
        // A server asked outside its region answers with nothing, so an empty
        // answer is checked with the next server
        let empty = matches!(&built, Ok(prepared) if prepared
            .features
            .as_ref()
            .is_some_and(|features| features.areas.is_empty() && features.lines.is_empty()));
        let map_error = match &built {
            Err(SceneError::MapData(error)) => Some(error),
            _ => None,
        };
        if (map_error.is_some() || (empty && self.server + 1 < self.servers.len()))
            && let Some(outcome) = self.next_server(map_error.and_then(|error| error.retry()))
        {
            return Ok(outcome);
        }
        if !again.is_empty() {
            let waits: Option<Vec<Wait>> = again
                .iter()
                .map(|(request, retry)| self.next_wait(request, *retry))
                .collect();
            if let Some(waits) = waits {
                for (request, _) in &again {
                    *self.retried.entry(request.key()).or_default() += 1;
                    self.replay.drop_answer(&request.key());
                    self.dropped.push(request.key());
                }
                self.missing = again.into_iter().map(|(request, _)| request).collect();
                let delay = waits
                    .iter()
                    .map(|wait| wait.delay)
                    .max()
                    .unwrap_or_default();
                let shown = waits
                    .iter()
                    .copied()
                    .max_by_key(|wait| (wait.kind == Kind::MapData, wait.delay))
                    .unwrap_or(waits[0]);
                return Ok(Outcome::Wait(Wait { delay, ..shown }));
            }
        }
        let mut bundle = built.map_err(|error| self.failure(&error))?.bundle;
        if let Some(exaggeration) = self.exaggeration {
            bundle.grid.exaggeration = exaggeration;
        }
        if let Some(title) = &self.title {
            bundle.meta.title.clone_from(title);
        }
        self.missing.clear();
        self.warnings = heard.names;
        self.notes = self.read_notes.clone();
        for note in built_notes.into_iter().chain(heard.notes) {
            if !self.notes.contains(&note) {
                self.notes.push(note);
            }
        }
        bundle
            .to_bytes()
            .map(Outcome::Scene)
            .map_err(|error| Failure::new("other", chain(&error)))
    }

    /// The pause before the next attempt at a request, or none when its
    /// attempts are used up
    fn next_wait(&self, request: &Request, retry: Retry) -> Option<Wait> {
        let kind = self.kind(request);
        let waits = waits(kind, retry);
        let used = self.retried.get(&request.key()).copied().unwrap_or(0);
        Some(Wait {
            kind,
            retry,
            attempt: used + 2,
            attempts: waits.len() + 1,
            delay: *waits.get(used)?,
        })
    }

    /// After the map data failed: the next server, or once every server has
    /// failed, a pause and then the first again. None when that was the last
    /// try or trying again can't help
    fn next_server(&mut self, retry: Option<Retry>) -> Option<Outcome> {
        let outcome = if self.server + 1 < self.servers.len() {
            self.drop_map_answer();
            self.switch_to(self.server + 1);
            Outcome::Missing
        } else {
            let retry = retry?;
            let waits = waits(Kind::MapData, retry);
            let delay = *waits.get(self.rounds)?;
            self.rounds += 1;
            self.drop_map_answer();
            self.switch_to(0);
            Outcome::Wait(Wait {
                kind: Kind::MapData,
                retry,
                attempt: self.rounds + 1,
                attempts: waits.len() + 1,
                delay,
            })
        };
        self.missing = self.map_request().into_iter().collect();
        Some(outcome)
    }

    fn switch_to(&mut self, server: usize) {
        self.server = server;
        self.options.overpass_url.clone_from(&self.servers[server]);
        for request in &mut self.plan {
            if request.body.is_some() {
                request.url.clone_from(&self.servers[server]);
            }
        }
    }

    fn map_request(&self) -> Option<Request> {
        self.plan
            .iter()
            .find(|request| request.body.is_some())
            .cloned()
    }

    fn drop_map_answer(&mut self) {
        if let Some(request) = self.map_request() {
            self.replay.drop_answer(&request.key());
            self.dropped.push(request.key());
        }
    }

    /// The tool's message for `error`, without its flags
    fn failure(&self, error: &SceneError) -> Failure {
        match error {
            SceneError::TrimmedAway { length_m } => {
                let cuts: Vec<String> = [
                    (self.options.trim_start_m, "start"),
                    (self.options.trim_end_m, "end"),
                ]
                .into_iter()
                .filter(|&(metres, _)| metres > 0.0)
                .map(|(metres, from)| format!("{} from the {from}", distance_text(metres)))
                .collect();
                Failure::new(
                    "usage",
                    format!(
                        "trimming {} leaves none of the route, which is {} long; trim less",
                        cuts.join(" and "),
                        distance_text(*length_m)
                    ),
                )
            }
            SceneError::NoGround => Failure::new(
                "no-ground",
                format!(
                    "the route stays on one spot, so a padding of {} leaves no ground to draw; \
                     give it some metres, such as 15,500",
                    self.padding
                ),
            ),
            SceneError::Footprint(GeoError::EmptyTrack | GeoError::Track(_)) => Failure::new(
                "input",
                format!("can't read {}: {}", self.shown, chain(error)),
            ),
            SceneError::Elevation(DemError::Fetch { source, .. }) => {
                let server = origin(&self.options.terrain_url);
                match source.retry() {
                    Some(Retry::Busy) => Failure::new(
                        "busy",
                        format!(
                            "the elevation server {server} is busy or failing ({}); try again later",
                            busy_reason(source)
                        ),
                    ),
                    Some(Retry::Unreachable) => unreachable(&server, "try again"),
                    None => Failure::new("network", chain(error)),
                }
            }
            SceneError::MapData(osm) => {
                let several = self.servers.len() > 1;
                match osm.retry() {
                    Some(Retry::Busy) => Failure::new(
                        "busy",
                        format!(
                            "OpenStreetMap's Overpass {} busy or failing ({}); {WAYS_OUT}",
                            if several { "servers are" } else { "server is" },
                            osm.busy_reason().unwrap_or_default().trim_end_matches('.')
                        ),
                    ),
                    Some(Retry::Unreachable) if several => {
                        unreachable("OpenStreetMap's Overpass servers", WAYS_OUT)
                    }
                    Some(Retry::Unreachable) => {
                        unreachable(&origin(&self.options.overpass_url), WAYS_OUT)
                    }
                    None => Failure::new("network", format!("{}; {WAYS_OUT}", chain(error))),
                }
            }
            SceneError::Elevation(_) => Failure::new("network", chain(error)),
            _ => Failure::new("other", chain(error)),
        }
    }
}

fn unreachable(server: &str, then: &str) -> Failure {
    Failure::new(
        "unreachable",
        format!("could not reach {server}; check the connection, then {then}"),
    )
}

fn busy_reason(error: &FetchError) -> String {
    match error {
        FetchError::Status { status, .. } => format!("HTTP {status}"),
        FetchError::Body { .. } => "the answer was cut off".to_owned(),
        _ => "no answer in time".to_owned(),
    }
}

/// The error and its causes on one line, no cause repeated, as the command
/// line prints them
fn chain(error: &(dyn Error + 'static)) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut cause = Some(error);
    while let Some(error) = cause {
        let text = plain(&error.to_string());
        if !text.is_empty() && !parts.iter().any(|part| part.contains(&text)) {
            parts.push(text);
        }
        cause = error.source();
    }
    parts.join(": ")
}

#[derive(Default)]
struct Heard {
    names: Vec<String>,
    notes: Vec<String>,
}

impl Progress for Heard {
    /// The command line's warnings, without its flags
    fn warn(&mut self, warning: Warning) {
        let (name, note) = match &warning {
            Warning::MapDataSkipped => (
                "map-data-skipped",
                "the diorama is built from the elevation alone, without lakes, woods or paths"
                    .to_owned(),
            ),
            Warning::TimesLeftOut => (
                "times-left-out",
                "the date and times are left out, with the light, weather and season that follow from them"
                    .to_owned(),
            ),
            Warning::NoWeatherRequest(error) => (
                "no-weather-request",
                format!("{error}, so the weather is left out"),
            ),
            Warning::Weather(error) => (
                "weather",
                format!("can't get the weather, so it is left out: {}", chain(error)),
            ),
            Warning::DaysBefore(error) => (
                "days-before",
                format!(
                    "can't get the weather before the hike, so lakes are left open: {}",
                    chain(error)
                ),
            ),
        };
        self.names.push(name.to_owned());
        self.notes.push(note);
    }
}

impl Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.reason, self.message)
    }
}
