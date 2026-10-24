use std::fs::File;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use isotrail_geo::{
    Cache, CachedFetcher, FeatureSource, Fetcher, Footprint, Grid, OVERPASS_URL, OverpassSource,
    RecordedFetcher, TerrariumSource, TileId,
};
use isotrail_track::{GpxReader, TrackReader};

const OVERPASS_ENV: &str = "ISOTRAIL_OVERPASS_URL";
const MIRROR: &str = "https://overpass.example.invalid/api/interpreter";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn isotrail(cache_dir: &Path, args: &[&str]) -> Output {
    isotrail_with(cache_dir, args, None)
}

fn isotrail_with(cache_dir: &Path, args: &[&str], overpass_env: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_isotrail"));
    command
        .args(args)
        .env("ISOTRAIL_CACHE_DIR", cache_dir)
        .env_remove(OVERPASS_ENV)
        .stdin(Stdio::null());
    if let Some(url) = overpass_env {
        command.env(OVERPASS_ENV, url);
    }
    command.output().unwrap()
}

fn snowdon_grid() -> Grid {
    let mut file = File::open(fixtures().join("gpx/snowdon.gpx")).unwrap();
    let track = GpxReader.read(&mut file).unwrap().clean().unwrap();
    Grid::new(
        &Footprint::from_track(&track).unwrap(),
        NonZeroU32::new(32).unwrap(),
    )
}

/// Copies recorded responses into a fresh cache, as if fetched earlier
fn warm(cache_dir: &Path, map_data: bool) {
    let dem = CachedFetcher::new(
        RecordedFetcher::new(fixtures().join("dem")),
        Cache::new(cache_dir),
    );
    for x in [1000, 1001] {
        let url = TerrariumSource::<RecordedFetcher>::url(TileId { z: 11, x, y: 666 });
        dem.get(&url).unwrap();
    }
    if map_data {
        let osm = CachedFetcher::new(
            RecordedFetcher::new(fixtures().join("osm")),
            Cache::new(cache_dir),
        );
        let body = OverpassSource::<RecordedFetcher>::request_body(&snowdon_grid());
        osm.post(OVERPASS_URL, &body).unwrap();
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn png_size(path: &Path) -> (u32, u32) {
    let decoder = png::Decoder::new(std::io::BufReader::new(File::open(path).unwrap()));
    let info = decoder.read_info().unwrap();
    (info.info().width, info.info().height)
}

fn gpx() -> String {
    fixtures().join("gpx/snowdon.gpx").display().to_string()
}

#[test]
fn offline_bundle_writes_debug_images_from_the_cache() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, true);

    let debug = temp.path().join("debug");
    let output = isotrail(
        &cache_dir,
        &[
            "bundle",
            &gpx(),
            "--detail",
            "32",
            "--offline",
            "--debug-dir",
            debug.to_str().unwrap(),
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    assert!(
        stdout.starts_with("Heightmap 32 × 16 cells of 118.3 m from 2 terrain tiles at zoom 11"),
        "{stdout}"
    );
    let features = OverpassSource::new(RecordedFetcher::new(fixtures().join("osm")))
        .features(&snowdon_grid())
        .unwrap();
    let map_data = format!(
        "Map data: {} areas and {} lines from OpenStreetMap",
        features.areas.len(),
        features.lines.len()
    );
    assert!(stdout.contains(&map_data), "{stdout}");
    assert_eq!(png_size(&debug.join("heightmap.png")), (32, 16));
    assert_eq!(png_size(&debug.join("hillshade.png")), (128, 64));
}

#[test]
fn offline_bundle_without_cached_tiles_explains_why() {
    let temp = tempfile::tempdir().unwrap();
    let debug = temp.path().join("debug");
    let output = isotrail(
        &temp.path().join("cache"),
        &[
            "bundle",
            &gpx(),
            "--offline",
            "--debug-dir",
            debug.to_str().unwrap(),
        ],
    );
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("could not fetch elevation tile"),
        "{stderr}"
    );
    assert!(stderr.contains("offline mode is on"), "{stderr}");
    assert!(!debug.exists());
}

#[test]
fn offline_bundle_without_cached_map_data_explains_why() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, false);
    let debug = temp.path().join("debug");
    let output = isotrail(
        &cache_dir,
        &[
            "bundle",
            &gpx(),
            "--detail",
            "32",
            "--offline",
            "--debug-dir",
            debug.to_str().unwrap(),
        ],
    );
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("could not fetch OpenStreetMap features"),
        "{stderr}"
    );
    assert!(stderr.contains("offline mode is on"), "{stderr}");
    assert!(!debug.exists());
}

#[test]
fn overpass_endpoint_comes_from_the_flag_or_the_environment() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, true);
    let debug = temp.path().join("debug");
    let args = [
        "bundle",
        &gpx(),
        "--detail",
        "32",
        "--offline",
        "--debug-dir",
        debug.to_str().unwrap(),
    ];

    let flag = [&args[..], &["--overpass-url", MIRROR]].concat();
    for output in [
        isotrail(&cache_dir, &flag),
        isotrail_with(&cache_dir, &args, Some(MIRROR)),
    ] {
        assert!(!output.status.success());
        let stderr = text(&output.stderr);
        assert!(
            stderr.contains(&format!("{MIRROR} is not in the cache")),
            "{stderr}"
        );
    }
    assert!(isotrail(&cache_dir, &args).status.success());
}

#[test]
fn detail_is_range_checked() {
    let temp = tempfile::tempdir().unwrap();
    let output = isotrail(
        temp.path(),
        &["bundle", &gpx(), "--detail", "2", "--debug-dir", "unused"],
    );
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("--detail"));
}
