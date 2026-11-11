use std::fs::File;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use isotrail_bundle::SceneBundle;
use isotrail_geo::{
    Cache, CachedFetcher, FeatureSource, Fetcher, Footprint, Grid, OPEN_METEO_ARCHIVE_URL,
    OVERPASS_URL, OpenMeteoSource, OverpassSource, RecordedFetcher, TerrariumSource, TileId,
    WeatherRequest,
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

fn warm_weather(cache_dir: &Path) {
    let mut file = File::open(fixtures().join("gpx/snowdon.gpx")).unwrap();
    let track = GpxReader.read(&mut file).unwrap().clean().unwrap();
    let request = WeatherRequest::from_track(&track, 0).unwrap();
    let weather = CachedFetcher::new(
        RecordedFetcher::new(fixtures().join("weather")),
        Cache::new(cache_dir),
    );
    for url in [
        OpenMeteoSource::<RecordedFetcher>::url(OPEN_METEO_ARCHIVE_URL, &request),
        OpenMeteoSource::<RecordedFetcher>::days_url(OPEN_METEO_ARCHIVE_URL, &request),
    ] {
        weather.get(&url).unwrap();
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
    warm_weather(&cache_dir);

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
    assert!(
        stdout.contains("\nWeather: drizzle, 9 – 10 °C, 0.6 mm rain (Open-Meteo archive)\n"),
        "{stdout}"
    );
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
    assert_eq!(bundle.season.leaf_day, Some(165));
    assert_eq!(
        (bundle.season.snow_line_m, bundle.season.ice_line_m),
        (None, None)
    );
    let weather = bundle.weather.unwrap();
    assert_eq!(weather.samples.len(), 4);
    assert_eq!(
        bundle.meta.provenance.weather.as_deref(),
        Some("Open-Meteo archive at 1 location")
    );
    assert!(text(&output.stderr).is_empty(), "{}", text(&output.stderr));
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
    assert!(!itb.exists());
    // The folder is made before any download, but nothing is written into it
    assert_eq!(std::fs::read_dir(&debug).unwrap().count(), 0);
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
    assert!(!itb.exists());
    // The folder is made before any download, but nothing is written into it
    assert_eq!(std::fs::read_dir(&debug).unwrap().count(), 0);
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
    warm_weather(&cache_dir);
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

    for extra in [
        &[][..],
        &["--json"],
        &["--units", "imperial"],
        &["--weather", "--offline"],
        &["--weather", "--offline", "--json"],
    ] {
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
            &b"ITB1\x05\x00not deflate"[..],
            "the bundle is damaged",
        ),
        (
            "old.itb",
            b"ITB1\x01\x00",
            "create it again with `isotrail bundle`",
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
fn weather_is_left_out_when_it_cannot_be_found_or_is_not_wanted() {
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
    for (extra, warning) in [(&[][..], true), (&["--no-weather"][..], false)] {
        let output = isotrail(&cache_dir, &[&args[..], extra].concat());
        assert!(output.status.success(), "{}", text(&output.stderr));
        assert!(text(&output.stdout).contains("\nWeather: none\n"));
        let stderr = text(&output.stderr);
        assert_eq!(
            stderr.contains("could not get the weather, so it is left out"),
            warning,
            "{stderr}"
        );
        if warning {
            assert!(stderr.contains("is not in the cache"), "{stderr}");
        }
        let bundle = SceneBundle::from_bytes(&std::fs::read(&itb).unwrap()).unwrap();
        assert_eq!(bundle.weather, None);
        assert_eq!(bundle.meta.provenance.weather, None);

        let output = isotrail(&cache_dir, &["stats", itb.to_str().unwrap(), "--weather"]);
        assert!(output.status.success());
        assert!(text(&output.stderr).contains("the bundle has no weather"));
        assert!(!text(&output.stdout).contains("Credit"));
    }
}

#[test]
fn stats_show_the_weather_hour_by_hour() {
    let temp = tempfile::tempdir().unwrap();
    warm_weather(temp.path());
    let output = isotrail(temp.path(), &["stats", &gpx(), "--weather", "--offline"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    for line in [
        "  Weather    drizzle (Open-Meteo archive)\n",
        "  Temp       9 – 10 °C\n",
        "  Rain       0.6 mm\n",
        "  Snowfall   none\n",
        "  Credit     Weather data by Open-Meteo.com\n",
        "  Sat 14 Jun 2025\n",
        "  10:00    9.6 °C  light drizzle",
        "  11:00    8.6 °C  drizzle",
    ] {
        assert!(stdout.contains(line), "{line} in {stdout}");
    }

    let output = isotrail(
        temp.path(),
        &["stats", &gpx(), "--weather", "--offline", "--json"],
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let weather = &json["weather"];
    assert_eq!(weather["dataset"], "archive");
    assert_eq!(weather["attribution"], "Weather data by Open-Meteo.com");
    let samples = weather["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 4);
    assert_eq!(samples[1]["time"], "2025-06-14T09:00:00Z");
    assert_eq!(samples[1]["condition"], "light drizzle");
    for key in [
        "temperature_c",
        "weather_code",
        "cloud_cover_pct",
        "cloud_cover_low_pct",
        "precipitation_mm",
        "rain_mm",
        "snowfall_cm",
        "snow_depth_m",
        "wind_speed_m_s",
        "wind_direction_deg",
    ] {
        assert!(samples[1][key].is_number(), "{key}");
    }

    let plain = isotrail(temp.path(), &["stats", &gpx(), "--json"]);
    let json: serde_json::Value = serde_json::from_slice(&plain.stdout).unwrap();
    assert!(json.get("weather").is_none());

    let cold = tempfile::tempdir().unwrap();
    let output = isotrail(cold.path(), &["stats", &gpx(), "--weather", "--offline"]);
    assert!(output.status.success());
    assert!(text(&output.stderr).contains("could not get the weather"));
    assert!(text(&output.stdout).contains("Distance"));
    assert!(!text(&output.stdout).contains("Credit"));

    let untimed = fixtures().join("gpx/no-time.gpx");
    let output = isotrail(
        cold.path(),
        &["stats", untimed.to_str().unwrap(), "--weather", "--offline"],
    );
    assert!(output.status.success());
    assert!(text(&output.stderr).contains("the track has no times"));
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
