use std::fs::File;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use isotrail_geo::{
    FetchError, Fetcher, Fetchers, OSM_ATTRIBUTION, Prepared, Progress, RecordedFetcher,
    SceneError, SceneOptions, SeasonKind, Step, Times, Warning, prepare,
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

#[test]
fn missing_elevation_is_an_elevation_error() {
    let nothing = RecordedFetcher::new(fixtures().join("osm"));
    let fetchers = Fetchers {
        data: &nothing,
        map_data: &nothing,
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
