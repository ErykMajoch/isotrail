use std::cell::RefCell;
use std::collections::HashSet;
use std::fs::File;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use isotrail_geo::{
    Answer, FetchError, Fetcher, Fetchers, OSM_ATTRIBUTION, Prepared, Progress, RecordedFetcher,
    ReplayFetcher, Request, SceneError, SceneOptions, SeasonKind, Step, Times, Warning, prepare,
    requests,
};
use isotrail_track::{GpxReader, Track, TrackReader};

/// 2026-10-01, long after the recorded hike
const LATER: i64 = 1_790_812_800;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn snowdon() -> Track {
    let mut file = File::open(fixtures().join("gpx/snowdon.gpx")).unwrap();
    GpxReader.read(&mut file).unwrap().clean().unwrap()
}

/// Elevation tiles and weather, each from its own folder of recordings
struct Data([RecordedFetcher; 2]);

impl Data {
    fn new() -> Data {
        Data([
            RecordedFetcher::new(fixtures().join("dem")),
            RecordedFetcher::new(fixtures().join("weather")),
        ])
    }
}

impl Fetcher for Data {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.0[0].get(url).or_else(|_| self.0[1].get(url))
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
        self.0[0].post(url, body)
    }
}

#[derive(Default)]
struct Heard {
    steps: Vec<Step>,
    warnings: Vec<Warning>,
}

impl Progress for Heard {
    fn step(&mut self, step: Step) {
        self.steps.push(step);
    }

    fn warn(&mut self, warning: Warning) {
        self.warnings.push(warning);
    }
}

fn options() -> SceneOptions {
    SceneOptions {
        detail: NonZeroU32::new(32).unwrap(),
        ..SceneOptions::default()
    }
}

fn build(options: &SceneOptions) -> (Result<Prepared, SceneError>, Heard) {
    let data = Data::new();
    let map_data = RecordedFetcher::new(fixtures().join("osm"));
    let fetchers = Fetchers {
        data: &data,
        map_data: &map_data,
        waits: true,
    };
    let mut heard = Heard::default();
    let built = prepare(&snowdon(), "Snowdon", options, fetchers, LATER, &mut heard);
    (built, heard)
}

#[test]
fn builds_the_scene_from_recorded_data() {
    let (built, heard) = build(&options());
    let prepared = built.unwrap();
    assert!(heard.warnings.is_empty(), "{:?}", heard.warnings);
    assert_eq!(
        heard.steps,
        [
            Step::Elevation {
                tiles: prepared.tiles
            },
            Step::MapData,
            Step::Weather,
            Step::Season
        ]
    );
    let bundle = &prepared.bundle;
    assert_eq!(bundle.meta.title, "Snowdon");
    assert!(prepared.features.is_some());
    assert!(bundle.meta.attribution.iter().any(|c| c == OSM_ATTRIBUTION));
    assert!(bundle.weather.is_some());
    assert!(bundle.meta.provenance.weather.is_some());
    assert!(bundle.route.points.iter().all(|p| p.time.is_some()));
    assert!(bundle.meta.provenance.generator.starts_with("isotrail "));

    let (again, _) = build(&options());
    assert_eq!(
        again.unwrap().bundle.to_bytes().unwrap(),
        bundle.to_bytes().unwrap()
    );
}

#[test]
fn left_out_parts_are_warned_about() {
    let (built, heard) = build(&SceneOptions {
        map_data: false,
        times: Times::None,
        ..options()
    });
    let bundle = built.unwrap().bundle;
    assert!(matches!(
        heard.warnings[..],
        [Warning::MapDataSkipped, Warning::TimesLeftOut]
    ));
    assert!(!heard.steps.contains(&Step::MapData));
    assert!(bundle.weather.is_none());
    assert!(!bundle.meta.attribution.iter().any(|c| c == OSM_ATTRIBUTION));
    assert!(bundle.route.points.iter().all(|p| p.time.is_none()));
}

#[test]
fn a_forced_season_replaces_the_hike_s_own() {
    let (built, _) = build(&SceneOptions {
        season: Some(SeasonKind::Winter),
        ..options()
    });
    let season = built.unwrap().bundle.season;
    assert_eq!(season.leaf_day, Some(15));
    assert!(season.snow_line_m.is_some());
}

#[test]
fn trimming_everything_away_names_the_length() {
    let (built, _) = build(&SceneOptions {
        trim_start_m: 1_000_000.0,
        ..options()
    });
    match built {
        Err(SceneError::TrimmedAway { length_m }) => assert!(length_m > 100.0),
        _ => panic!("expected the route to be trimmed away"),
    }
}

/// Notes every request on its way to the recordings
struct Noting<'a> {
    inner: &'a dyn Fetcher,
    asked: &'a RefCell<HashSet<Request>>,
}

impl Fetcher for Noting<'_> {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.asked.borrow_mut().insert(Request::get(url));
        self.inner.get(url)
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
        self.asked
            .borrow_mut()
            .insert(Request::post(url, body.to_vec()));
        self.inner.post(url, body)
    }
}

fn variants() -> [SceneOptions; 4] {
    [
        options(),
        SceneOptions {
            times: Times::Coarse,
            ..options()
        },
        SceneOptions {
            map_data: false,
            ..options()
        },
        SceneOptions {
            trim_start_m: 300.0,
            trim_end_m: 200.0,
            map_data: false,
            ..options()
        },
    ]
}

#[test]
fn the_plan_is_what_a_build_asks_for() {
    for options in variants() {
        let asked = RefCell::new(HashSet::new());
        let data = Data::new();
        let osm = RecordedFetcher::new(fixtures().join("osm"));
        let fetchers = Fetchers {
            data: &Noting {
                inner: &data,
                asked: &asked,
            },
            map_data: &Noting {
                inner: &osm,
                asked: &asked,
            },
            waits: true,
        };
        prepare(
            &snowdon(),
            "Snowdon",
            &options,
            fetchers,
            LATER,
            &mut Heard::default(),
        )
        .unwrap();
        let planned: HashSet<Request> = requests(&snowdon(), &options, LATER)
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(planned, asked.into_inner(), "{options:?}");
    }
}

/// Fetches each request from the recordings, as a page fetches them
fn fetch_into(replay: &ReplayFetcher, wanted: Vec<Request>) {
    let data = Data::new();
    let osm = RecordedFetcher::new(fixtures().join("osm"));
    for request in wanted {
        let fetched = match &request.body {
            None => data.get(&request.url),
            Some(body) => osm.post(&request.url, body),
        };
        let answer = match fetched {
            Ok(body) => Answer::Body(body),
            Err(_) => Answer::Status {
                status: 404,
                body: Vec::new(),
            },
        };
        replay.hold(request.key(), answer);
    }
}

#[test]
fn replayed_answers_build_the_same_scene() {
    for options in variants() {
        let replay = ReplayFetcher::new();
        fetch_into(&replay, requests(&snowdon(), &options, LATER).unwrap());
        let fetchers = Fetchers {
            data: &replay,
            map_data: &replay,
            waits: true,
        };
        let replayed = prepare(
            &snowdon(),
            "Snowdon",
            &options,
            fetchers,
            LATER,
            &mut Heard::default(),
        )
        .unwrap();
        assert!(replay.take_missed().is_empty(), "{options:?}");
        let (direct, _) = build(&options);
        assert_eq!(
            replayed.bundle.to_bytes().unwrap(),
            direct.unwrap().bundle.to_bytes().unwrap(),
            "{options:?}"
        );
    }
}

#[test]
fn without_the_plan_a_replay_stops_at_each_missing_tile() {
    let replay = ReplayFetcher::new();
    let fetchers = Fetchers {
        data: &replay,
        map_data: &replay,
        waits: true,
    };
    let built = prepare(
        &snowdon(),
        "Snowdon",
        &options(),
        fetchers,
        LATER,
        &mut Heard::default(),
    );
    assert!(matches!(built, Err(SceneError::Elevation(_))));
    let missed = replay.take_missed();
    assert_eq!(missed.len(), 1);
    fetch_into(&replay, missed);
    let built = prepare(
        &snowdon(),
        "Snowdon",
        &options(),
        fetchers,
        LATER,
        &mut Heard::default(),
    );
    assert!(built.is_err());
    assert_eq!(replay.take_missed().len(), 1);
}

#[test]
fn missing_elevation_is_an_elevation_error() {
    let nothing = RecordedFetcher::new(fixtures().join("osm"));
    let fetchers = Fetchers {
        data: &nothing,
        map_data: &nothing,
        waits: true,
    };
    let built = prepare(
        &snowdon(),
        "Snowdon",
        &options(),
        fetchers,
        LATER,
        &mut Heard::default(),
    );
    assert!(matches!(built, Err(SceneError::Elevation(_))));
}
