use isotrail_geo::{Answer, Request, RequestFailure, Retry};
use js_sys::{Object, Reflect};
use wasm_bindgen::prelude::*;

use crate::{Builder, Failure, Kind, Outcome, Wait};

/// Reads the time zone boundaries, tzf-dist's `lite.tzb`. Every build needs
/// them, since the zone sets the hike's date
#[wasm_bindgen(js_name = loadTimezones)]
pub fn load_timezones(tzb: &[u8]) -> Result<(), JsError> {
    console_error_panic_hook::set_once();
    isotrail_track::load_timezones(tzb)
        .map_err(|error| JsError::new(&format!("can't read the time zones: {error}")))
}

/// The failure for a page with no network, as an `Error` with a `reason`
#[wasm_bindgen]
pub fn offline() -> JsValue {
    thrown(Failure::offline())
}

/// A request whose answer the build needs
#[wasm_bindgen]
pub struct Wanted {
    request: Request,
    kind: Kind,
    aliases: Vec<String>,
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

    /// `elevation`, `map-data` or `weather`
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        self.kind.name().to_owned()
    }

    /// For map data, the keys of the same request at the other Overpass
    /// servers. An answer kept under one of them can be held under it instead
    #[wasm_bindgen(getter)]
    pub fn aliases(&self) -> Vec<String> {
        self.aliases.clone()
    }
}

/// Builds a scene file from a GPX file, as `isotrail bundle` does. The
/// answers come from the page: `requests` lists what to fetch first, and
/// `build` either makes the scene, or lists in `missing` what it still
/// needs, after the pause in `wait` when answers failed. Failures are thrown
/// as an `Error` with a `reason`
#[wasm_bindgen]
pub struct SceneBuilder {
    builder: Builder,
    wait: Option<Wait>,
}

#[wasm_bindgen]
impl SceneBuilder {
    /// `gpx` is the file's bytes and `name` its name, which gives the title
    /// when the track has none. `options` is a JSON object keyed by the long
    /// flag names of `isotrail render`, such as `{"times": "coarse"}`. `now`
    /// is in seconds since 1970 and picks the weather's archive or forecast
    #[wasm_bindgen(constructor)]
    pub fn new(gpx: &[u8], name: &str, options: &str, now: f64) -> Result<SceneBuilder, JsValue> {
        console_error_panic_hook::set_once();
        if !isotrail_track::timezones_loaded() {
            return Err(
                JsError::new("the time zones aren't loaded: call loadTimezones first").into(),
            );
        }
        let builder = Builder::new(gpx, name, options, now as i64).map_err(thrown)?;
        Ok(SceneBuilder {
            builder,
            wait: None,
        })
    }

    /// Asks these Overpass servers in turn for the map data, in place of the
    /// built-in ones, unless the options name their own. Call it before
    /// `requests`
    #[wasm_bindgen(js_name = useServers)]
    pub fn use_servers(&mut self, servers: Vec<String>) {
        self.builder.use_servers(servers);
    }

    /// Leaves out the built-in Overpass servers with these names: `dresel`,
    /// `vk-maps` or `overpass-api`. Call it before `requests`. Throws with the
    /// reason `no-server` when none is left to ask for map data
    #[wasm_bindgen(js_name = skipServers)]
    pub fn skip_servers(&mut self, skip: Vec<String>) -> Result<(), JsValue> {
        self.builder.skip_servers(&skip).map_err(thrown)
    }

    /// The Overpass server the map data is asked of now
    #[wasm_bindgen(getter)]
    pub fn server(&self) -> String {
        self.builder.server().to_owned()
    }

    /// What the build asks for first and isn't held yet, so it can all be
    /// fetched at once
    pub fn requests(&self) -> Vec<Wanted> {
        self.wanted(self.builder.requests())
    }

    /// Holds the answer to the request with this key: its HTTP status and body
    pub fn hold(&mut self, key: String, status: u16, body: Vec<u8>) {
        let answer = if (200..300).contains(&status) {
            Answer::Body(body)
        } else {
            Answer::Status { status, body }
        };
        self.builder.hold(key, answer);
    }

    /// Holds a request that got no answer at all
    pub fn failed(&mut self, key: String, message: String, timed_out: bool) {
        let failure = RequestFailure { message, timed_out };
        self.builder.hold(key, Answer::Failed(failure));
    }

    /// The scene file's bytes, or nothing while answers are missing
    pub fn build(&mut self) -> Result<Option<Vec<u8>>, JsValue> {
        self.wait = None;
        match self.builder.build().map_err(thrown)? {
            Outcome::Scene(bytes) => Ok(Some(bytes)),
            Outcome::Missing => Ok(None),
            Outcome::Wait(wait) => {
                self.wait = Some(wait);
                Ok(None)
            }
        }
    }

    /// The requests the last build stopped at
    #[wasm_bindgen(getter)]
    pub fn missing(&self) -> Vec<Wanted> {
        self.wanted(self.builder.missing().to_vec())
    }

    /// The pause to make before fetching `missing`, when the last build
    /// stopped at answers that failed: `{ kind, why, attempt, attempts, ms }`,
    /// with `why` `busy` or `unreachable`
    #[wasm_bindgen(getter)]
    pub fn wait(&self) -> JsValue {
        let Some(wait) = self.wait else {
            return JsValue::UNDEFINED;
        };
        let why = match wait.retry {
            Retry::Busy => "busy",
            Retry::Unreachable => "unreachable",
        };
        object(&[
            ("kind", wait.kind.name().into()),
            ("why", why.into()),
            ("attempt", (wait.attempt as u32).into()),
            ("attempts", (wait.attempts as u32).into()),
            ("ms", (wait.delay.as_millis() as f64).into()),
        ])
    }

    /// What the scene left out, by name, such as `weather`
    #[wasm_bindgen(getter)]
    pub fn warnings(&self) -> Vec<String> {
        self.builder.warnings().to_vec()
    }

    /// The keys whose answers the last build threw away, as unusable or
    /// failed, so the page drops any copy it keeps
    #[wasm_bindgen(getter)]
    pub fn dropped(&self) -> Vec<String> {
        self.builder.dropped().to_vec()
    }
}

impl SceneBuilder {
    fn wanted(&self, requests: Vec<Request>) -> Vec<Wanted> {
        requests
            .into_iter()
            .map(|request| Wanted {
                kind: self.builder.kind(&request),
                aliases: self.builder.aliases(&request),
                request,
            })
            .collect()
    }
}

fn thrown(failure: Failure) -> JsValue {
    let error = js_sys::Error::new(&failure.message);
    let _ = Reflect::set(&error, &"reason".into(), &failure.reason.into());
    error.into()
}

fn object(fields: &[(&str, JsValue)]) -> JsValue {
    let object = Object::new();
    for (name, value) in fields {
        let _ = Reflect::set(&object, &(*name).into(), value);
    }
    object.into()
}
