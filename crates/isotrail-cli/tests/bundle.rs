use std::fs::File;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use isotrail_bundle::SceneBundle;
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
fn offline_bundle_writes_the_bundle_and_debug_images_from_the_cache() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, true);

    let debug = temp.path().join("debug");
    let itb = temp.path().join("snowdon.itb");
    let output = isotrail(
        &cache_dir,
        &[
            "bundle",
            &gpx(),
            "-o",
            itb.to_str().unwrap(),
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
    assert!(stdout.contains("\nLandcover: "), "{stdout}");
    assert!(stdout.contains(", bare rock "), "{stdout}");
    assert!(stdout.contains("levelled\n"), "{stdout}");
    assert!(stdout.contains("\nRoute: "), "{stdout}");
    let size = std::fs::metadata(&itb).unwrap().len().div_ceil(1024);
    assert!(
        stdout.contains(&format!("Wrote {} ({size} KB)\n", itb.display())),
        "{stdout}"
    );
    let bundle = SceneBundle::from_bytes(&std::fs::read(&itb).unwrap()).unwrap();
    assert_eq!((bundle.grid.cols, bundle.grid.rows), (32, 16));
    assert_eq!(bundle.meta.title, "Synthetic Snowdon Descent");
    assert!(!bundle.route.points.is_empty());
    assert_eq!(png_size(&debug.join("heightmap.png")), (32, 16));
    for name in ["hillshade.png", "landcover.png", "water.png", "paths.png"] {
        assert_eq!(png_size(&debug.join(name)), (128, 64), "{name}");
    }
    let legend = std::fs::read_to_string(debug.join("landcover-legend.txt")).unwrap();
    assert_eq!(legend.lines().count(), 13);
    assert!(legend.contains("  water\n"), "{legend}");
}

#[test]
fn offline_bundle_without_cached_tiles_explains_why() {
    let temp = tempfile::tempdir().unwrap();
    let debug = temp.path().join("debug");
    let itb = temp.path().join("hike.itb");
    let output = isotrail(
        &temp.path().join("cache"),
        &[
            "bundle",
            &gpx(),
            "-o",
            itb.to_str().unwrap(),
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
    assert!(!debug.exists() && !itb.exists());
}

#[test]
fn offline_bundle_without_cached_map_data_explains_why() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, false);
    let debug = temp.path().join("debug");
    let itb = temp.path().join("hike.itb");
    let output = isotrail(
        &cache_dir,
        &[
            "bundle",
            &gpx(),
            "-o",
            itb.to_str().unwrap(),
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
    assert!(!debug.exists() && !itb.exists());
}

#[test]
fn overpass_endpoint_comes_from_the_flag_or_the_environment() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, true);
    let itb = temp.path().join("hike.itb");
    let args = [
        "bundle",
        &gpx(),
        "-o",
        itb.to_str().unwrap(),
        "--detail",
        "32",
        "--offline",
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
        &["bundle", &gpx(), "--detail", "2", "-o", "unused.itb"],
    );
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("--detail"));
}

#[test]
fn stats_of_a_bundle_match_the_gpx() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, true);
    let itb = temp.path().join("hike.itb");
    let output = isotrail(
        &cache_dir,
        &[
            "bundle",
            &gpx(),
            "-o",
            itb.to_str().unwrap(),
            "--detail",
            "32",
            "--offline",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(!text(&output.stdout).contains("png"));
    let renamed = temp.path().join("hike.bin");
    std::fs::copy(&itb, &renamed).unwrap();

    for extra in [&[][..], &["--json"], &["--units", "imperial"]] {
        let stats = |file: &str| {
            let output = isotrail(&cache_dir, &[&["stats", file][..], extra].concat());
            assert!(output.status.success(), "{}", text(&output.stderr));
            text(&output.stdout)
        };
        let expected = stats(&gpx());
        assert_eq!(stats(itb.to_str().unwrap()), expected, "{extra:?}");
        assert_eq!(stats(renamed.to_str().unwrap()), expected, "{extra:?}");
    }
}

#[test]
fn stats_of_a_damaged_bundle_explains_why() {
    let temp = tempfile::tempdir().unwrap();
    for (name, bytes, reason) in [
        (
            "damaged.itb",
            &b"ITB1\x01\x00not deflate"[..],
            "the bundle is damaged",
        ),
        ("newer.dat", b"ITB1\x09\x00", "format version 9"),
        ("hike.itb", b"<gpx/>", "not an isotrail scene bundle"),
    ] {
        let path = temp.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let output = isotrail(temp.path(), &["stats", path.to_str().unwrap()]);
        assert!(!output.status.success());
        let stderr = text(&output.stderr);
        assert!(stderr.contains("could not read"), "{stderr}");
        assert!(stderr.contains(reason), "{stderr}");
    }
}

#[test]
fn exaggeration_can_be_set_by_hand() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, true);
    let itb = temp.path().join("snowdon.itb");
    let args = [
        "bundle",
        &gpx(),
        "-o",
        itb.to_str().unwrap(),
        "--detail",
        "32",
        "--offline",
        "--exaggeration",
        "7.5",
    ];
    let output = isotrail(&cache_dir, &args);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).contains(" m, drawn 7.5× taller\n"));
    let bundle = SceneBundle::from_bytes(&std::fs::read(&itb).unwrap()).unwrap();
    assert_eq!(bundle.grid.exaggeration, 7.5);

    let output = isotrail(
        &cache_dir,
        &["bundle", &gpx(), "-o", "x.itb", "--exaggeration", "0"],
    );
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("must be between 0.1 and 100"));
}
