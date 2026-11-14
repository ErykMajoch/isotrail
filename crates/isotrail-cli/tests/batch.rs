use std::fs::{self, File};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::SystemTime;

use isotrail_geo::{
    Cache, CachedFetcher, Fetcher, Footprint, Grid, OPEN_METEO_ARCHIVE_URL, OVERPASS_URL,
    OpenMeteoSource, OverpassSource, RecordedFetcher, TerrariumSource, TileId, WeatherRequest,
};
use isotrail_track::{GpxReader, TrackReader};
use serde_json::Value;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn isotrail(cache_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(args)
        .env("ISOTRAIL_CACHE_DIR", cache_dir)
        .env("ISOTRAIL_CONFIG", "")
        .env_remove("ISOTRAIL_OVERPASS_URL")
        .env_remove("ISOTRAIL_TERRAIN_URL")
        .env_remove("ISOTRAIL_WEATHER_URL")
        .env_remove("ISOTRAIL_OPEN_METEO_KEY")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Copies the recorded Snowdon responses into a fresh cache
fn warm(cache_dir: &Path) {
    let mut file = File::open(fixtures().join("gpx/snowdon.gpx")).unwrap();
    let track = GpxReader.read(&mut file).unwrap().clean().unwrap();
    let grid = Grid::new(
        &Footprint::from_track(&track).unwrap(),
        NonZeroU32::new(32).unwrap(),
    );
    let dem = CachedFetcher::new(
        RecordedFetcher::new(fixtures().join("dem")),
        Cache::new(cache_dir),
    );
    for x in [1000, 1001] {
        let url = TerrariumSource::<RecordedFetcher>::url(TileId { z: 11, x, y: 666 });
        dem.get(&url).unwrap();
    }
    let osm = CachedFetcher::new(
        RecordedFetcher::new(fixtures().join("osm")),
        Cache::new(cache_dir),
    );
    osm.post(
        OVERPASS_URL,
        &OverpassSource::<RecordedFetcher>::request_body(&grid),
    )
    .unwrap();
    let weather = CachedFetcher::new(
        RecordedFetcher::new(fixtures().join("weather")),
        Cache::new(cache_dir),
    );
    let request = WeatherRequest::from_track(&track, 0).unwrap();
    let url = OpenMeteoSource::<RecordedFetcher>::url(OPEN_METEO_ARCHIVE_URL, &request);
    weather.get(&url).unwrap();
}

/// The synthetic Snowdon walk under another name, so the recorded data
/// covers it
fn walk(name: &str) -> String {
    fs::read_to_string(fixtures().join("gpx/snowdon.gpx"))
        .unwrap()
        .replace("Synthetic Snowdon Descent", name)
}

/// A folder of GPX files, an output folder and a warm cache
struct Batch {
    temp: tempfile::TempDir,
}

impl Batch {
    fn new(files: &[(&str, String)]) -> Batch {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("in")).unwrap();
        for (name, contents) in files {
            fs::write(temp.path().join("in").join(name), contents).unwrap();
        }
        warm(&temp.path().join("cache"));
        Batch { temp }
    }

    fn dir(&self) -> PathBuf {
        self.temp.path().join("in")
    }

    fn out(&self, name: &str) -> PathBuf {
        self.temp.path().join("out").join(name)
    }

    fn run(&self, extra: &[&str]) -> Output {
        let dir = self.dir();
        let out = self.temp.path().join("out");
        let mut args = vec![
            "batch",
            dir.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--offline",
            "--detail",
            "32",
            "--scale",
            "6",
        ];
        args.extend(extra);
        isotrail(&self.temp.path().join("cache"), &args)
    }

    fn written(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.temp.path().join("out"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn modified(&self, name: &str) -> SystemTime {
        fs::metadata(self.out(name)).unwrap().modified().unwrap()
    }

    fn json(&self, name: &str) -> Value {
        serde_json::from_slice(&fs::read(self.out(name)).unwrap()).unwrap()
    }
}

fn three() -> Batch {
    Batch::new(&[
        ("a.gpx", walk("Walk A")),
        ("b.gpx", walk("Walk B")),
        ("c.gpx", walk("Walk C")),
    ])
}

/// Each hike's result in a run's JSON summary
fn results(output: &Output) -> Vec<(String, String)> {
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    summary["hikes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hike| {
            let input = hike["input"].as_str().unwrap();
            let name = Path::new(input).file_name().unwrap().to_string_lossy();
            (
                name.into_owned(),
                hike["result"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|&(name, result)| (name.to_owned(), result.to_owned()))
        .collect()
}

#[test]
fn three_hikes_make_nine_files_and_a_second_run_writes_nothing() {
    let batch = three();
    let output = batch.run(&[]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(
        batch.written(),
        [
            "a.itb", "a.json", "a.png", "b.itb", "b.json", "b.png", "c.itb", "c.json", "c.png"
        ]
    );
    let stdout = text(&output.stdout);
    assert!(stdout.contains("a.gpx (1 of 3)\n"), "{stdout}");
    assert!(stdout.ends_with("Hikes: 3 made\n"), "{stdout}");

    let json = batch.json("b.json");
    assert_eq!(json["name"], "Walk B");
    assert!(json["distance_m"].as_f64().unwrap() > 0.0);
    assert!(json["weather"]["summary"].is_object(), "{json:#}");
    assert_eq!(json["profile"].as_array().unwrap().len(), 64);
    assert!(json["format"].is_u64());
    let files = json["files"].as_array().unwrap();
    assert_eq!(files[0]["path"], "b.itb");
    assert_eq!(files[1]["path"], "b.png");
    assert_eq!(files[1]["width"], 1080);
    assert_eq!(json["made_from"]["gpx"], "b.gpx");
    assert_eq!(json["made_from"]["gpx_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(json["made_from"]["isotrail"], env!("CARGO_PKG_VERSION"));

    let before: Vec<SystemTime> = batch.written().iter().map(|n| batch.modified(n)).collect();
    let again = batch.run(&[]);
    assert!(again.status.success(), "{}", text(&again.stderr));
    let stdout = text(&again.stdout);
    assert!(!stdout.contains("Wrote"), "{stdout}");
    assert!(stdout.contains("b.gpx (2 of 3): unchanged\n"), "{stdout}");
    assert!(
        stdout.ends_with("Nothing to do: all 3 hikes are unchanged\n"),
        "{stdout}"
    );
    let after: Vec<SystemTime> = batch.written().iter().map(|n| batch.modified(n)).collect();
    assert_eq!(before, after);
}

#[test]
fn a_change_redoes_only_what_depends_on_it() {
    let batch = three();
    assert!(batch.run(&[]).status.success());
    let scenes = || ["a.itb", "b.itb", "c.itb"].map(|name| batch.modified(name));
    let first = scenes();

    fs::write(batch.dir().join("b.gpx"), walk("Walk B, renamed")).unwrap();
    let output = batch.run(&["--json"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(
        results(&output),
        pairs(&[
            ("a.gpx", "unchanged"),
            ("b.gpx", "made"),
            ("c.gpx", "unchanged")
        ])
    );
    assert_eq!(batch.json("b.json")["name"], "Walk B, renamed");
    let second = scenes();
    assert_eq!((first[0], first[2]), (second[0], second[2]));
    assert_ne!(first[1], second[1]);

    let pictures = batch.modified("a.png");
    let output = batch.run(&["--json", "--palette", "resurrect-64"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(
        results(&output),
        pairs(&[
            ("a.gpx", "redrawn"),
            ("b.gpx", "redrawn"),
            ("c.gpx", "redrawn")
        ])
    );
    assert_eq!(scenes(), second);
    assert_ne!(batch.modified("a.png"), pictures);

    fs::remove_file(batch.out("c.png")).unwrap();
    let output = batch.run(&["--json", "--palette", "resurrect-64"]);
    assert_eq!(
        results(&output),
        pairs(&[
            ("a.gpx", "unchanged"),
            ("b.gpx", "unchanged"),
            ("c.gpx", "redrawn")
        ])
    );
    assert!(batch.out("c.png").is_file());

    let output = batch.run(&["--json", "--palette", "resurrect-64", "--profile", "8"]);
    assert_eq!(
        results(&output),
        pairs(&[
            ("a.gpx", "rewritten"),
            ("b.gpx", "rewritten"),
            ("c.gpx", "rewritten")
        ])
    );
    assert_eq!(batch.json("a.json")["profile"].as_array().unwrap().len(), 8);
    assert_eq!(scenes(), second);

    let output = batch.run(&["--json", "--palette", "resurrect-64", "--force"]);
    assert_eq!(
        results(&output),
        pairs(&[("a.gpx", "made"), ("b.gpx", "made"), ("c.gpx", "made")])
    );
}

#[test]
fn a_broken_gpx_file_doesnt_stop_the_others() {
    let batch = Batch::new(&[
        ("a.gpx", walk("Walk A")),
        ("broken.gpx", "<gpx><trk><trkseg><trkpt".to_owned()),
        ("notes.txt", "not a hike".to_owned()),
    ]);
    let output = batch.run(&["--json"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(stderr.contains("broken.gpx"), "{stderr}");
    assert!(
        stderr.contains("error: 1 of 2 hikes failed; see the error above"),
        "{stderr}"
    );
    assert_eq!(
        results(&output),
        pairs(&[("a.gpx", "made"), ("broken.gpx", "failed")])
    );
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["hikes"][1]["error"]["reason"], "input");
    assert_eq!(batch.written(), ["a.itb", "a.json", "a.png"]);
}

#[test]
fn names_that_differ_only_by_case_both_fail() {
    let batch = Batch::new(&[("Walk.gpx", walk("Upper")), ("walk.gpx", walk("Lower"))]);
    if fs::read_dir(batch.dir()).unwrap().count() < 2 {
        eprintln!("note: this file system ignores case, so it can't hold both files");
        return;
    }
    let output = batch.run(&[]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("Walk.gpx and walk.gpx would write the same files"),
        "{stderr}"
    );
    assert!(stderr.contains("2 of 2 hikes failed"), "{stderr}");
}

#[test]
fn a_folder_without_gpx_files_is_refused() {
    let batch = Batch::new(&[("notes.txt", "not a hike".to_owned())]);
    let output = batch.run(&[]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(stderr.contains("has no GPX files in it"), "{stderr}");

    fs::remove_dir_all(batch.dir()).unwrap();
    let output = batch.run(&[]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("can't open the folder") && stderr.contains("it doesn't exist"),
        "{stderr}"
    );
}
