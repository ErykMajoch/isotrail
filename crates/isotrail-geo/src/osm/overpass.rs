use std::collections::BTreeMap;
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use tracing::{debug, info, warn};

use super::query::form_body;
use super::rings::assemble;
use super::{
    Area, BoundingBox, FeatureSource, Line, OsmError, OsmFeatures, OsmId, OsmKind, Polygon, Tags,
    overpass_query,
};
use crate::{FetchError, Fetcher, Grid, RetryPolicy, origin};

pub const OVERPASS_URL: &str = "https://overpass-api.de/api/interpreter";
/// For a server that doesn't answer at all, which is more often a wrong
/// address or no network than a busy server
pub const UNREACHABLE_RETRY: RetryPolicy = RetryPolicy {
    max_attempts: 3,
    base_delay: Duration::from_secs(2),
    max_delay: Duration::from_secs(4),
};

/// Map features from an Overpass server. It retries by itself, so give it a
/// fetcher that tries each request once
#[derive(Debug, Clone)]
pub struct OverpassSource<F> {
    fetcher: F,
    endpoint: String,
    busy: RetryPolicy,
    unreachable: RetryPolicy,
}

impl<F: Fetcher> OverpassSource<F> {
    pub fn new(fetcher: F) -> OverpassSource<F> {
        OverpassSource {
            fetcher,
            endpoint: OVERPASS_URL.to_owned(),
            busy: RetryPolicy::overpass(),
            unreachable: UNREACHABLE_RETRY,
        }
    }

    pub fn retry(self, busy: RetryPolicy, unreachable: RetryPolicy) -> OverpassSource<F> {
        OverpassSource {
            busy,
            unreachable,
            ..self
        }
    }

    pub fn endpoint(self, url: impl Into<String>) -> OverpassSource<F> {
        OverpassSource {
            endpoint: url.into(),
            ..self
        }
    }

    /// The form body POSTed for a grid
    pub fn request_body(grid: &Grid) -> Vec<u8> {
        form_body(&overpass_query(
            &BoundingBox::around(grid),
            grid.cell_size_m,
        ))
    }

    fn attempt(&self, body: &[u8]) -> Result<OsmFeatures, OsmError> {
        let bytes = self
            .fetcher
            .post(&self.endpoint, body)
            .map_err(|source| OsmError::Fetch { source })?;
        debug!(bytes = bytes.len(), "Overpass response");
        parse(&bytes).inspect_err(|_| {
            if let Err(err) = self.fetcher.forget(&self.endpoint, Some(body)) {
                warn!("could not drop the bad Overpass response from the cache: {err}");
            }
        })
    }

    /// The policy that applies to a failure and how to describe it, or none
    /// when trying again can't help
    fn retry_for(&self, error: &OsmError) -> Option<(RetryPolicy, String)> {
        match error {
            OsmError::Fetch { source } if source.is_unreachable() => Some((
                self.unreachable,
                format!("could not reach {}", origin(&self.endpoint)),
            )),
            OsmError::Fetch { source } if source.is_transient() => {
                let reason = match source {
                    FetchError::Status { status, .. } => format!("HTTP {status}"),
                    FetchError::Body { .. } => "the answer was cut off".to_owned(),
                    _ => "no answer in time".to_owned(),
                };
                Some((self.busy, format!("Overpass is busy ({reason})")))
            }
            OsmError::Json { .. } => Some((
                self.busy,
                "Overpass is busy (the answer was not map data)".to_owned(),
            )),
            OsmError::Overpass { remark } if is_busy_remark(remark) => {
                Some((self.busy, format!("Overpass is busy ({remark})")))
            }
            _ => None,
        }
    }
}

impl<F: Fetcher> FeatureSource for OverpassSource<F> {
    fn features(&self, grid: &Grid) -> Result<OsmFeatures, OsmError> {
        let body = Self::request_body(grid);
        info!(endpoint = %self.endpoint, "fetching OpenStreetMap features from Overpass");
        let mut attempt = 1;
        loop {
            let error = match self.attempt(&body) {
                Ok(features) => return Ok(features),
                Err(error) => error,
            };
            match self.retry_for(&error) {
                Some((policy, reason)) if attempt < policy.max_attempts => {
                    let delay = policy.delay(attempt - 1, retry_after(&error));
                    warn!(
                        "{reason}; trying again in {:.0} s (attempt {attempt} of {})",
                        delay.as_secs_f64(),
                        policy.max_attempts
                    );
                    thread::sleep(delay);
                    attempt += 1;
                }
                _ if attempt > 1 => {
                    return Err(OsmError::GaveUp {
                        attempts: attempt,
                        last: Box::new(error),
                    });
                }
                _ => return Err(error),
            }
        }
    }
}

fn retry_after(error: &OsmError) -> Option<Duration> {
    match error {
        OsmError::Fetch { source } => source.retry_after(),
        _ => None,
    }
}

/// Overpass reports overload inside an HTTP 200 as a remark
fn is_busy_remark(remark: &str) -> bool {
    let remark = remark.to_ascii_lowercase();
    [
        "timed out",
        "timeout",
        "rate limit",
        "rate_limit",
        "too many",
    ]
    .iter()
    .any(|busy| remark.contains(busy))
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    elements: Vec<Element>,
    remark: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Element {
    Way(Way),
    Relation(Relation),
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct Way {
    id: i64,
    #[serde(default)]
    tags: BTreeMap<String, String>,
    #[serde(default)]
    geometry: Vec<Option<LatLon>>,
}

#[derive(Deserialize)]
struct Relation {
    id: i64,
    #[serde(default)]
    tags: BTreeMap<String, String>,
    #[serde(default)]
    members: Vec<Member>,
}

#[derive(Deserialize)]
struct Member {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    geometry: Vec<Option<LatLon>>,
}

#[derive(Deserialize, Clone, Copy)]
struct LatLon {
    lat: f64,
    lon: f64,
}

pub(super) fn parse(bytes: &[u8]) -> Result<OsmFeatures, OsmError> {
    let response: Response =
        serde_json::from_slice(bytes).map_err(|source| OsmError::Json { source })?;
    if let Some(remark) = response.remark
        && remark.contains("error")
    {
        return Err(OsmError::Overpass { remark });
    }

    let mut features = OsmFeatures::default();
    let mut skipped = 0;
    for element in response.elements {
        match element {
            Element::Way(way) => {
                let id = OsmId {
                    kind: OsmKind::Way,
                    id: way.id,
                };
                let tags = Tags(way.tags);
                match points(&way.geometry) {
                    Some(points) if is_line(&tags) => {
                        features.lines.push(Line { id, tags, points })
                    }
                    Some(outer) if outer.len() >= 4 && outer[0] == outer[outer.len() - 1] => {
                        features.areas.push(Area {
                            id,
                            tags,
                            polygons: vec![Polygon {
                                outer,
                                inner: Vec::new(),
                            }],
                        });
                    }
                    _ => {
                        debug!(%id, "skipping an incomplete or unclosed way");
                        skipped += 1;
                    }
                }
            }
            Element::Relation(relation) => {
                let id = OsmId {
                    kind: OsmKind::Relation,
                    id: relation.id,
                };
                let (mut outer, mut inner) = (Vec::new(), Vec::new());
                for member in relation.members.iter().filter(|m| m.kind == "way") {
                    let Some(points) = points(&member.geometry) else {
                        continue;
                    };
                    match member.role.as_str() {
                        "outer" | "" => outer.push(points),
                        "inner" => inner.push(points),
                        _ => {}
                    }
                }
                let (polygons, dropped) = assemble(outer, inner);
                if dropped > 0 {
                    debug!(%id, dropped, "multipolygon pieces that don't form rings");
                }
                if polygons.is_empty() {
                    debug!(%id, "skipping a multipolygon without a closed outer ring");
                    skipped += 1;
                } else {
                    features.areas.push(Area {
                        id,
                        tags: Tags(relation.tags),
                        polygons,
                    });
                }
            }
            Element::Other => {}
        }
    }
    if skipped > 0 {
        debug!(skipped, "OpenStreetMap elements left out");
    }
    Ok(features)
}

fn is_line(tags: &Tags) -> bool {
    tags.get("highway").is_some()
        || tags.get("waterway").is_some()
        || tags.is("natural", "cliff")
        || tags.is("natural", "coastline")
}

/// None when any point is missing, so a partial way is never mistaken for a
/// whole one
fn points(geometry: &[Option<LatLon>]) -> Option<Vec<(f64, f64)>> {
    let points: Option<Vec<_>> = geometry.iter().map(|p| p.map(|p| (p.lat, p.lon))).collect();
    points.filter(|p| p.len() >= 2)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::num::NonZeroU32;

    use isotrail_track::{Segment, Track, TrackPoint};

    use super::*;
    use crate::{Cache, CachedFetcher, Footprint, HttpFetcher};

    const TIMED_OUT: &str = r#"{"version":0.6,"elements":[],
        "remark":"runtime error: Query timed out in \"query\" at line 3 after 2 seconds."}"#;

    #[derive(Default)]
    struct Fake {
        body: &'static str,
        calls: Cell<u32>,
    }

    impl Fetcher for Fake {
        fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
            Err(FetchError::status(url, 405))
        }

        fn post(&self, _url: &str, _body: &[u8]) -> Result<Vec<u8>, FetchError> {
            self.calls.set(self.calls.get() + 1);
            Ok(self.body.as_bytes().to_vec())
        }
    }

    fn grid() -> Grid {
        let at = |lat, lon| TrackPoint {
            lat,
            lon,
            ele: None,
            time: None,
        };
        let track = Track {
            segments: vec![Segment {
                points: vec![at(50.0, 10.0), at(50.01, 10.02)],
            }],
            ..Track::default()
        };
        Grid::new(
            &Footprint::from_track(&track).unwrap(),
            NonZeroU32::new(16).unwrap(),
        )
    }

    #[test]
    fn ways_become_lines_or_areas_by_their_tags() {
        let json = r#"{"elements":[
            {"type":"node","id":1,"lat":50.0,"lon":10.0,"tags":{"natural":"peak"}},
            {"type":"way","id":2,"tags":{"highway":"path","sac_scale":"hiking"},
             "geometry":[{"lat":50.0,"lon":10.0},{"lat":50.1,"lon":10.1}]},
            {"type":"way","id":3,"tags":{"natural":"water"},
             "geometry":[{"lat":50.0,"lon":10.0},{"lat":50.0,"lon":10.1},{"lat":50.1,"lon":10.1},{"lat":50.0,"lon":10.0}]},
            {"type":"way","id":4,"tags":{"natural":"cliff"},
             "geometry":[{"lat":50.0,"lon":10.0},{"lat":50.0,"lon":10.1},{"lat":50.1,"lon":10.1},{"lat":50.0,"lon":10.0}]},
            {"type":"way","id":5,"tags":{"landuse":"meadow"},
             "geometry":[{"lat":50.0,"lon":10.0},{"lat":50.0,"lon":10.1},{"lat":50.1,"lon":10.1}]},
            {"type":"way","id":6,"tags":{"waterway":"stream"},
             "geometry":[{"lat":50.0,"lon":10.0},null,{"lat":50.1,"lon":10.1}]},
            {"type":"way","id":7,"tags":{"highway":"track"},"geometry":[{"lat":50.0,"lon":10.0}]},
            {"type":"way","id":8,"tags":{"natural":"coastline"},
             "geometry":[{"lat":50.0,"lon":10.0},{"lat":50.1,"lon":10.0},{"lat":50.1,"lon":10.1},{"lat":50.0,"lon":10.0}]}
        ]}"#;
        let features = parse(json.as_bytes()).unwrap();
        let ids = |ids: Vec<OsmId>| ids.into_iter().map(|id| id.id).collect::<Vec<_>>();
        assert_eq!(
            ids(features.lines.iter().map(|l| l.id).collect()),
            [2, 4, 8]
        );
        assert_eq!(ids(features.areas.iter().map(|a| a.id).collect()), [3]);
        let path = &features.lines[0];
        assert!(path.tags.is("highway", "path"));
        assert_eq!(path.tags.get("sac_scale"), Some("hiking"));
        assert_eq!(path.points, [(50.0, 10.0), (50.1, 10.1)]);
        assert_eq!(features.areas[0].polygons[0].outer.len(), 4);
    }

    #[test]
    fn error_remarks_and_bad_json_are_errors() {
        assert!(matches!(
            parse(TIMED_OUT.as_bytes()),
            Err(OsmError::Overpass { remark }) if remark.starts_with("runtime error")
        ));
        assert!(matches!(
            parse(b"<html>busy</html>"),
            Err(OsmError::Json { .. })
        ));
        let harmless = r#"{"elements":[],"remark":"note: nothing found"}"#;
        assert_eq!(parse(harmless.as_bytes()).unwrap(), OsmFeatures::default());
    }

    #[test]
    fn bad_responses_are_not_kept_in_the_cache() {
        let temp = tempfile::tempdir().unwrap();
        let fake = Fake {
            body: TIMED_OUT,
            ..Fake::default()
        };
        let cached = CachedFetcher::new(&fake, Cache::new(temp.path()));
        let source = OverpassSource::new(&cached).retry(FAST, QUICK);
        for _ in 0..2 {
            let error = source.features(&grid()).unwrap_err();
            assert!(
                matches!(&error, OsmError::GaveUp { last, .. } if matches!(**last, OsmError::Overpass { .. })),
                "{error:?}"
            );
        }
        assert_eq!(fake.calls.get(), 12);
        assert_eq!(cached.cache().stats(None).unwrap().files, 0);
    }

    #[test]
    fn good_responses_are_cached_under_the_endpoint() {
        let temp = tempfile::tempdir().unwrap();
        let fake = Fake {
            body: r#"{"elements":[]}"#,
            ..Fake::default()
        };
        let cached = CachedFetcher::new(&fake, Cache::new(temp.path()));
        let mirror = "https://overpass.example.org/api/interpreter";
        let source = OverpassSource::new(&cached).endpoint(mirror);
        source.features(&grid()).unwrap();
        source.features(&grid()).unwrap();
        assert_eq!(fake.calls.get(), 1);

        let offline = CachedFetcher::new(&fake, Cache::new(temp.path())).offline(true);
        let error = OverpassSource::new(&offline).features(&grid()).unwrap_err();
        assert!(
            matches!(&error, OsmError::Fetch { source: FetchError::Offline { url } } if url == OVERPASS_URL),
            "{error:?}"
        );
        assert!(
            OverpassSource::new(&offline)
                .endpoint(mirror)
                .features(&grid())
                .is_ok()
        );
    }

    const FAST: RetryPolicy = RetryPolicy {
        max_attempts: 6,
        base_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
    };
    const QUICK: RetryPolicy = RetryPolicy {
        max_attempts: 3,
        ..FAST
    };
    const EMPTY: &str = r#"{"elements":[]}"#;

    /// Answers each POST with the next scripted reply: a body, or a status
    struct Script {
        replies: RefCell<Vec<Result<&'static str, u16>>>,
        calls: Cell<u32>,
    }

    impl Script {
        fn new(replies: Vec<Result<&'static str, u16>>) -> Script {
            Script {
                replies: RefCell::new(replies),
                calls: Cell::new(0),
            }
        }
    }

    impl Fetcher for Script {
        fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
            Err(FetchError::status(url, 405))
        }

        fn post(&self, url: &str, _body: &[u8]) -> Result<Vec<u8>, FetchError> {
            self.calls.set(self.calls.get() + 1);
            match self.replies.borrow_mut().remove(0) {
                Ok(body) => Ok(body.as_bytes().to_vec()),
                Err(status) => Err(FetchError::status(url, status)),
            }
        }
    }

    fn features(script: &Script) -> Result<OsmFeatures, OsmError> {
        OverpassSource::new(script)
            .retry(FAST, QUICK)
            .features(&grid())
    }

    #[test]
    fn busy_remarks_html_and_cut_off_json_are_tried_again() {
        for busy in [TIMED_OUT, "<html>busy</html>", r#"{"elements":[{"type":"#] {
            let script = Script::new(vec![Ok(busy), Err(504), Ok(EMPTY)]);
            assert_eq!(features(&script).unwrap(), OsmFeatures::default());
            assert_eq!(script.calls.get(), 3);
        }
        let limited = r#"{"elements":[],"remark":"runtime error: rate_limited"}"#;
        let script = Script::new(vec![Ok(limited), Ok(EMPTY)]);
        assert!(features(&script).is_ok());
        assert_eq!(script.calls.get(), 2);
    }

    #[test]
    fn a_busy_server_is_given_up_on_after_the_last_attempt() {
        let statuses = [429, 502, 503, 504, 500, 504];
        let script = Script::new(statuses.iter().map(|&s| Err(s)).collect());
        let error = features(&script).unwrap_err();
        assert_eq!(script.calls.get(), 6);
        assert!(
            matches!(&error, OsmError::GaveUp { attempts: 6, last }
                if matches!(**last, OsmError::Fetch { source: FetchError::Status { status: 504, .. } })),
            "{error:?}"
        );
        assert_eq!(error.to_string(), "Overpass still failed after 6 attempts");
    }

    #[test]
    fn other_failures_are_not_tried_again() {
        let script = Script::new(vec![Err(400)]);
        assert!(matches!(
            features(&script),
            Err(OsmError::Fetch {
                source: FetchError::Status { status: 400, .. }
            })
        ));
        let memory = r#"{"elements":[],"remark":"runtime error: Query run out of memory."}"#;
        let script = Script::new(vec![Ok(memory)]);
        assert!(matches!(features(&script), Err(OsmError::Overpass { .. })));
        assert_eq!(script.calls.get(), 1);
    }

    struct Counting<F> {
        inner: F,
        calls: Cell<u32>,
    }

    impl<F: Fetcher> Fetcher for Counting<F> {
        fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
            self.inner.get(url)
        }

        fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
            self.calls.set(self.calls.get() + 1);
            self.inner.post(url, body)
        }
    }

    #[test]
    fn an_unreachable_server_is_given_up_on_sooner() {
        let port = crate::fetch::CLOSED_PORT;
        let counting = Counting {
            inner: HttpFetcher::new().unwrap().with_retry(RetryPolicy::once()),
            calls: Cell::new(0),
        };
        let error = OverpassSource::new(&counting)
            .endpoint(format!("http://127.0.0.1:{port}/api/interpreter"))
            .retry(FAST, QUICK)
            .features(&grid())
            .unwrap_err();
        assert_eq!(counting.calls.get(), 3);
        assert!(
            matches!(error, OsmError::GaveUp { attempts: 3, .. }),
            "{error:?}"
        );
    }
}
