use std::fs::File;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use isotrail_bundle::SceneBundle;
use isotrail_geo::{
    Cache, CachedFetcher, FeatureSource, FetchError, Fetcher, Footprint, Grid,
    OPEN_METEO_ARCHIVE_URL, OPEN_METEO_CUSTOMER_ARCHIVE_URL, OVERPASS_URL, OpenMeteoSource,
    OverpassSource, RecordedFetcher, TERRARIUM_URL, TerrariumSource, TileId, WeatherRequest,
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
        .env("ISOTRAIL_CONFIG", "")
        .env_remove(OVERPASS_ENV)
        .env_remove("ISOTRAIL_TERRAIN_URL")
        .env_remove("ISOTRAIL_WEATHER_URL")
        .env_remove("ISOTRAIL_OPEN_METEO_KEY")
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
        stdout.starts_with("Heightmap 32 x 16 cells of 118.3m from 2 terrain tiles at zoom 11"),
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
        stdout.contains("\nWeather: drizzle, 9 – 10°C, 0.6mm rain (Open-Meteo archive)\n"),
        "{stdout}"
    );
    assert!(stdout.contains("\nRoute: "), "{stdout}");
    let size = std::fs::metadata(&itb).unwrap().len().div_ceil(1024);
    assert!(
        stdout.contains(&format!("Wrote {} ({size}KB)\n", itb.display())),
        "{stdout}"
    );
    let bundle = SceneBundle::from_bytes(&std::fs::read(&itb).unwrap()).unwrap();
    assert_eq!((bundle.grid.cols, bundle.grid.rows), (32, 16));
    assert_eq!(bundle.meta.title, "Synthetic Snowdon Descent");
    assert_eq!(
        bundle.meta.provenance.map_data,
        "OpenStreetMap via https://overpass-api.de"
    );
    assert_eq!(bundle.meta.attribution[0], "© OpenStreetMap contributors");
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
        stderr.contains(
            "the elevation data for this area isn't in the cache yet; run once without --offline"
        ),
        "{stderr}"
    );
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
        stderr.contains("the map data for this area isn't in the cache yet"),
        "{stderr}"
    );
    assert!(stderr.contains("--no-map-data"), "{stderr}");
    assert!(!itb.exists());
    // The folder is made before any download, but nothing is written into it
    assert_eq!(std::fs::read_dir(&debug).unwrap().count(), 0);
}

#[test]
fn no_map_data_builds_from_the_elevation_alone() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, false);
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
            "--no-weather",
            "--no-map-data",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    assert!(
        stdout.contains("\nMap data: none (--no-map-data)\n"),
        "{stdout}"
    );
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("building the diorama from the elevation alone"),
        "{stderr}"
    );
    let bundle = SceneBundle::from_bytes(&std::fs::read(&itb).unwrap()).unwrap();
    assert_eq!(bundle.meta.provenance.map_data, "none");
    assert_eq!(bundle.meta.attribution, ["Terrain Tiles (AWS)"]);
    assert!(bundle.water.is_empty());
    assert!(bundle.terrain.lines.iter().all(|&mask| mask == 0));
}

#[test]
fn the_title_comes_from_the_flag_or_a_config_file() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, false);
    let config = temp.path().join("hike.toml");
    std::fs::write(
        &config,
        "[render]
palette = \"aap-64\"
title = \"Shared\"

[bundle]
title = \"Summit day\"
no-map-data = true
",
    )
    .unwrap();
    let itb = temp.path().join("hike.itb");
    let bundle = |extra: &[&str]| {
        let mut args = vec![
            "bundle",
            &gpx(),
            "-o",
            itb.to_str().unwrap(),
            "--detail",
            "32",
            "--offline",
            "--no-weather",
            "--config",
            config.to_str().unwrap(),
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        args.extend(extra.iter().map(|arg| (*arg).to_owned()));
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = isotrail(&cache_dir, &args);
        assert!(output.status.success(), "{}", text(&output.stderr));
        SceneBundle::from_bytes(&std::fs::read(&itb).unwrap()).unwrap()
    };
    let saved = bundle(&[]);
    assert_eq!(saved.meta.title, "Summit day");
    assert_eq!(saved.meta.provenance.map_data, "none");
    assert_eq!(bundle(&["--title", "Łańcuch"]).meta.title, "Łańcuch");
}

#[test]
fn a_failing_overpass_server_names_the_ways_out() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, false);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0; 65_536];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let reply = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        std::io::Write::write_all(&mut stream, reply.as_bytes()).unwrap();
    });
    let itb = temp.path().join("hike.itb");
    let url = format!("http://127.0.0.1:{port}/api/interpreter");
    let output = isotrail(
        &cache_dir,
        &[
            "bundle",
            &gpx(),
            "-o",
            itb.to_str().unwrap(),
            "--detail",
            "32",
            "--no-weather",
            "--overpass-url",
            &url,
        ],
    );
    server.join().unwrap();
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("Overpass server is busy or failing. Try again later, use another server with --overpass-url, or pass --no-map-data"),
        "{stderr}"
    );
    assert!(stderr.contains("returned HTTP 400"), "{stderr}");
    assert!(!itb.exists());
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
            stderr.contains("the map data from https://overpass.example.invalid for this area"),
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
            &b"ITB1\x06\x00not deflate"[..],
            "the bundle is damaged",
        ),
        (
            "old.itb",
            b"ITB1\x01\x00",
            "create it again from the GPX with `isotrail bundle`",
        ),
        ("newer.dat", b"ITB1\x09\x00", "made by a newer isotrail"),
        ("hike.itb", b"<gpx/>", "not an isotrail scene bundle"),
    ] {
        let path = temp.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let output = isotrail(temp.path(), &["stats", path.to_str().unwrap()]);
        assert!(!output.status.success());
        let stderr = text(&output.stderr);
        assert!(stderr.contains("can't read"), "{stderr}");
        assert!(stderr.contains(reason), "{stderr}");
    }
}

#[test]
fn bundling_a_bundle_says_what_it_is() {
    let temp = tempfile::tempdir().unwrap();
    let itb = temp.path().join("hike.itb");
    std::fs::write(&itb, isotrail_testkit::hike_bundle().to_bytes().unwrap()).unwrap();
    let again = temp.path().join("again.itb");
    let output = isotrail(
        temp.path(),
        &[
            "bundle",
            itb.to_str().unwrap(),
            "-o",
            again.to_str().unwrap(),
            "--offline",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = text(&output.stderr);
    assert!(stderr.contains("is already a scene bundle"), "{stderr}");
    assert!(!stderr.contains("UTF-8"), "{stderr}");
    assert!(!again.exists());
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
            stderr.contains(
                "the weather for this hike isn't in the cache, so it is left out; run once without --offline to add it"
            ),
            warning,
            "{stderr}"
        );
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
        "  Temp       9 – 10°C\n",
        "  Rain       0.6mm\n",
        "  Snowfall   none\n",
        "  Credit     Weather data by Open-Meteo.com\n",
        "  Sat 14 Jun 2025\n",
        "  10:00     9.6°C  light drizzle",
        "  11:00     8.6°C  drizzle",
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
    assert!(text(&output.stderr).contains("the weather for this hike isn't in the cache"));
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
    assert!(text(&output.stdout).contains("m, drawn 7.5x taller\n"));
    let bundle = SceneBundle::from_bytes(&std::fs::read(&itb).unwrap()).unwrap();
    assert_eq!(bundle.grid.exaggeration, 7.5);

    let output = isotrail(
        &cache_dir,
        &["bundle", &gpx(), "-o", "x.itb", "--exaggeration", "0"],
    );
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("expected a number from 0.1 to 100"));
}

/// Answers for one server with the recordings of another
struct Mirror {
    recorded: RecordedFetcher,
    from: &'static str,
    to: &'static str,
}

impl Fetcher for Mirror {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let url = url.replace(self.from, self.to);
        let url = url.split("&apikey=").next().unwrap_or_default();
        let url = url.split("?key=").next().unwrap_or_default();
        self.recorded.get(url)
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, FetchError> {
        self.recorded.post(url, body)
    }
}

#[test]
fn terrain_and_weather_can_come_from_other_servers_and_cache_folders() {
    const TILES: &str = "http://tiles.example.invalid/terrarium/{z}/{x}/{y}.png?key=s3cret";
    const KEY: &str = "test-key-123";
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("chosen cache");
    let cache = || Cache::new(&cache_dir);
    let tiles = CachedFetcher::new(
        Mirror {
            recorded: RecordedFetcher::new(fixtures().join("dem")),
            from: "http://tiles.example.invalid/terrarium",
            to: "https://s3.amazonaws.com/elevation-tiles-prod/terrarium",
        },
        cache(),
    );
    for x in [1000, 1001] {
        let url = TerrariumSource::<Mirror>::url_from(TILES, TileId { z: 11, x, y: 666 });
        tiles.get(&url).unwrap();
    }
    let osm = CachedFetcher::new(RecordedFetcher::new(fixtures().join("osm")), cache());
    osm.post(
        OVERPASS_URL,
        &OverpassSource::<RecordedFetcher>::request_body(&snowdon_grid()),
    )
    .unwrap();
    let mut file = File::open(fixtures().join("gpx/snowdon.gpx")).unwrap();
    let track = GpxReader.read(&mut file).unwrap().clean().unwrap();
    let request = WeatherRequest::from_track(&track, 0).unwrap();
    let weather = CachedFetcher::new(
        Mirror {
            recorded: RecordedFetcher::new(fixtures().join("weather")),
            from: OPEN_METEO_CUSTOMER_ARCHIVE_URL,
            to: OPEN_METEO_ARCHIVE_URL,
        },
        cache(),
    );
    for url in [
        OpenMeteoSource::<RecordedFetcher>::url(OPEN_METEO_CUSTOMER_ARCHIVE_URL, &request),
        OpenMeteoSource::<RecordedFetcher>::days_url(OPEN_METEO_CUSTOMER_ARCHIVE_URL, &request),
    ] {
        weather.get(&format!("{url}&apikey={KEY}")).unwrap();
    }

    // The environment's cache folder is empty: --cache-dir wins over it
    let itb = temp.path().join("mirrored.itb");
    let output = Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args([
            "bundle",
            &gpx(),
            "-o",
            itb.to_str().unwrap(),
            "--detail",
            "32",
            "--offline",
        ])
        .args([
            "--terrain-url",
            TILES,
            "--cache-dir",
            cache_dir.to_str().unwrap(),
        ])
        .env("ISOTRAIL_CACHE_DIR", temp.path().join("empty"))
        .env("ISOTRAIL_CONFIG", "")
        .env("ISOTRAIL_OPEN_METEO_KEY", KEY)
        .env_remove(OVERPASS_ENV)
        .env_remove("ISOTRAIL_TERRAIN_URL")
        .env_remove("ISOTRAIL_WEATHER_URL")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    assert!(stdout.contains("\nWeather: drizzle"), "{stdout}");
    let bundle = SceneBundle::from_bytes(&std::fs::read(&itb).unwrap()).unwrap();
    assert_eq!(
        bundle.meta.provenance.elevation,
        "http://tiles.example.invalid/terrarium/{z}/{x}/{y}.png?key=redacted at zoom 11"
    );
    assert!(bundle.weather.is_some());
    // No key reaches the cache's records
    for entry in std::fs::read_dir(&cache_dir).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(KEY) && !text.contains("s3cret"));
    }

    // Without the flag, the environment's empty folder is used
    let output = Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args([
            "bundle",
            &gpx(),
            "-o",
            itb.to_str().unwrap(),
            "--detail",
            "32",
            "--offline",
        ])
        .env("ISOTRAIL_CACHE_DIR", temp.path().join("empty"))
        .env("ISOTRAIL_CONFIG", "")
        .env("ISOTRAIL_TERRAIN_URL", TILES)
        .env_remove(OVERPASS_ENV)
        .env_remove("ISOTRAIL_OPEN_METEO_KEY")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("tiles.example.invalid"), "{stderr}");
    assert!(!stderr.contains("s3cret"), "{stderr}");
    assert!(TERRARIUM_URL.contains("{z}"));
}

#[test]
fn server_addresses_are_checked() {
    let temp = tempfile::tempdir().unwrap();
    for (flag, value, message) in [
        (
            "--terrain-url",
            "https://tiles.example/{z}/{x}.png",
            "{y} is missing",
        ),
        ("--weather-url", "weather.example", "https:// or http://"),
    ] {
        let output = isotrail(temp.path(), &["bundle", &gpx(), "-o", "a.itb", flag, value]);
        assert_eq!(output.status.code(), Some(2), "{flag}");
        let stderr = text(&output.stderr);
        assert!(stderr.contains(message), "{stderr}");
    }
}

/// Answers `count` Overpass requests with the recorded map data
fn overpass_server(count: usize) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let body = RecordedFetcher::new(fixtures().join("osm"))
        .post(
            OVERPASS_URL,
            &OverpassSource::<RecordedFetcher>::request_body(&snowdon_grid()),
        )
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://127.0.0.1:{}/api/interpreter",
        listener.local_addr().unwrap().port()
    );
    let server = std::thread::spawn(move || {
        for _ in 0..count {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let lower = line.to_ascii_lowercase();
                if let Some(value) = lower.strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                if line == "\r\n" || line.is_empty() {
                    break;
                }
            }
            let mut request = vec![0; length];
            reader.read_exact(&mut request).unwrap();
            let mut stream = reader.into_inner();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
        }
    });
    (url, server)
}

#[test]
fn the_first_download_of_a_run_is_announced() {
    const LINE: &str =
        "Downloading elevation and map data for this area (kept in the cache for next time)";
    let temp = tempfile::tempdir().unwrap();
    let (url, server) = overpass_server(3);
    let run = |cache: &str, extra: &[&str]| {
        let cache_dir = temp.path().join(cache);
        if !cache_dir.exists() {
            warm(&cache_dir, false);
        }
        let itb = temp.path().join(format!("{cache}.itb"));
        let args = [
            "bundle",
            &gpx(),
            "-o",
            itb.to_str().unwrap(),
            "--detail",
            "32",
            "--no-weather",
            "--overpass-url",
            &url,
        ];
        let output = isotrail(&cache_dir, &[&args[..], extra].concat());
        assert!(output.status.success(), "{}", text(&output.stderr));
        output
    };
    // The map data isn't cached yet: the line comes once, before it
    let cold = run("cold", &[]);
    let stderr = text(&cold.stderr);
    assert_eq!(stderr.matches(LINE).count(), 1, "{stderr}");
    assert!(!text(&cold.stdout).contains("Downloading"));
    // Everything is cached now
    let warm = run("cold", &[]);
    assert!(
        !text(&warm.stderr).contains("Downloading"),
        "{}",
        text(&warm.stderr)
    );
    for (cache, flag) in [("quiet", "-q"), ("json", "--json")] {
        let output = run(cache, &[flag]);
        assert!(
            !text(&output.stderr).contains("Downloading"),
            "{flag}: {}",
            text(&output.stderr)
        );
    }
    server.join().unwrap();
}

/// The fixture's walk with every time 17s past the minute, so no time it
/// holds is on a quarter hour
fn walk(dir: &Path) -> PathBuf {
    let gpx = std::fs::read_to_string(fixtures().join("gpx/snowdon.gpx")).unwrap();
    let path = dir.join("walk.gpx");
    std::fs::write(&path, gpx.replace(":00Z</time>", ":17Z</time>")).unwrap();
    path
}

fn read_bundle(path: &Path) -> SceneBundle {
    SceneBundle::from_bytes(&std::fs::read(path).unwrap()).unwrap()
}

fn json(output: &Output) -> serde_json::Value {
    assert!(output.status.success(), "{}", text(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Bundles `gpx` offline into `dir/name` with `extra` flags
fn bundle_walk(cache_dir: &Path, gpx: &Path, dir: &Path, name: &str, extra: &[&str]) -> PathBuf {
    let itb = dir.join(name);
    let args = [
        &[
            "bundle",
            gpx.to_str().unwrap(),
            "-o",
            itb.to_str().unwrap(),
            "--detail",
            "32",
            "--offline",
        ][..],
        extra,
    ]
    .concat();
    let output = isotrail(cache_dir, &args);
    assert!(output.status.success(), "{}", text(&output.stderr));
    itb
}

#[test]
fn coarse_times_keep_no_recorded_time_and_no_description() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, true);
    warm_weather(&cache_dir);
    let gpx = walk(temp.path());
    let recorded = bundle_walk(&cache_dir, &gpx, temp.path(), "recorded.itb", &[]);
    let coarse = bundle_walk(
        &cache_dir,
        &gpx,
        temp.path(),
        "coarse.itb",
        &["--times", "coarse", "--no-desc"],
    );

    let stats = |itb: &Path| {
        json(&isotrail(
            &cache_dir,
            &["stats", itb.to_str().unwrap(), "--json"],
        ))
    };
    let (before, after) = (stats(&recorded), stats(&coarse));
    assert_eq!(before["start"], "2025-06-14T09:00:17Z");
    assert!(before["desc"].is_string());
    assert_eq!(after["start"], "2025-06-14T09:00:00Z");
    assert_eq!(after["start_local"], "2025-06-14T10:00:00+01:00");
    assert_eq!(after["end"], "2025-06-14T09:15:00Z");
    assert!(after["desc"].is_null());
    for key in ["moving_s", "elapsed_s", "distance_m", "gain_m", "timezone"] {
        assert_eq!(after[key], before[key], "{key}");
    }

    let mut file = File::open(&gpx).unwrap();
    let track = GpxReader.read(&mut file).unwrap();
    let times: Vec<i64> = track
        .points()
        .filter_map(|point| point.time)
        .map(|time| time.timestamp())
        .collect();
    let (recorded, coarse) = (read_bundle(&recorded), read_bundle(&coarse));
    let stored: Vec<i64> = coarse
        .route
        .points
        .iter()
        .map(|point| point.time.unwrap())
        .chain(coarse.sun.iter().map(|sample| sample.time))
        .chain(coarse.moon.iter().map(|sample| sample.time))
        .collect();
    assert!(stored.len() > coarse.route.points.len());
    for time in stored {
        assert!(!times.contains(&time), "{time}");
    }
    assert_eq!(coarse.prop_seed, recorded.prop_seed);
    assert_eq!(coarse.meta.local_date, recorded.meta.local_date);
    assert_eq!(coarse.season, recorded.season);
    assert!(coarse.weather.is_some());
}

#[test]
fn coarse_times_draw_the_same_scene_under_a_held_sun() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, true);
    warm_weather(&cache_dir);
    let gpx = walk(temp.path());
    let render = |extra: &[&str], name: &str| {
        let png = temp.path().join(name);
        let args = [
            &[
                "render",
                gpx.to_str().unwrap(),
                "-o",
                png.to_str().unwrap(),
                "--detail",
                "32",
                "--offline",
                "--aspect",
                "square",
                "--scale",
                "6",
                "--sun",
                "150,40",
                "--no-weather",
            ][..],
            extra,
        ]
        .concat();
        let output = isotrail(&cache_dir, &args);
        assert!(output.status.success(), "{}", text(&output.stderr));
        std::fs::read(png).unwrap()
    };
    let recorded = render(&[], "recorded.png");
    assert!(recorded == render(&["--times", "coarse"], "coarse.png"));
}

#[test]
fn no_times_is_a_track_without_times_but_for_the_seed() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir, true);
    let gpx = walk(temp.path());
    let itb = temp.path().join("none.itb");
    let output = isotrail(
        &cache_dir,
        &[
            "bundle",
            gpx.to_str().unwrap(),
            "-o",
            itb.to_str().unwrap(),
            "--detail",
            "32",
            "--offline",
            "--times",
            "none",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let warnings: Vec<String> = text(&output.stderr).lines().map(String::from).collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("--times none: leaving out the date and times"));

    let untimed_gpx = temp.path().join("untimed.gpx");
    let timed = std::fs::read_to_string(&gpx).unwrap();
    let lines: Vec<&str> = timed
        .lines()
        .filter(|line| !line.contains("<time>"))
        .collect();
    std::fs::write(&untimed_gpx, lines.join("\n")).unwrap();
    let untimed = bundle_walk(&cache_dir, &untimed_gpx, temp.path(), "untimed.itb", &[]);

    let (none, mut untimed) = (read_bundle(&itb), read_bundle(&untimed));
    let recorded = read_bundle(&bundle_walk(
        &cache_dir,
        &gpx,
        temp.path(),
        "recorded.itb",
        &["--no-weather"],
    ));
    assert_eq!(none.prop_seed, recorded.prop_seed);
    assert_ne!(untimed.prop_seed, none.prop_seed);
    untimed.prop_seed = none.prop_seed;
    assert_eq!(none, untimed);
}

#[test]
fn time_options_need_a_gpx_file_and_its_date() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    let itb = temp.path().join("hike.itb");
    std::fs::write(&itb, isotrail_testkit::hike_bundle().to_bytes().unwrap()).unwrap();
    let site = temp.path().join("site");
    let png = temp.path().join("hike.png");
    let gpx = walk(temp.path());
    let config = temp.path().join("coarse.toml");
    std::fs::write(&config, "[render]\ntimes = \"coarse\"\n").unwrap();
    for (args, flag) in [
        (
            vec![
                "render",
                itb.to_str().unwrap(),
                "-o",
                png.to_str().unwrap(),
                "--config",
                config.to_str().unwrap(),
            ],
            "--times coarse",
        ),
        (
            vec![
                "embed",
                itb.to_str().unwrap(),
                "-o",
                site.to_str().unwrap(),
                "--times",
                "coarse",
            ],
            "--times coarse",
        ),
        (
            vec![
                "render",
                itb.to_str().unwrap(),
                "-o",
                png.to_str().unwrap(),
                "--times",
                "none",
            ],
            "--times none",
        ),
        (
            vec![
                "render",
                gpx.to_str().unwrap(),
                "-o",
                png.to_str().unwrap(),
                "--times",
                "none",
                "--time",
                "10:00",
            ],
            "--time needs the hike's date",
        ),
    ] {
        let output = isotrail(&cache_dir, &args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(
            text(&output.stderr).contains(flag),
            "{}",
            text(&output.stderr)
        );
    }
    assert!(!png.exists());
    assert!(!cache_dir.exists());
    let written = std::fs::read_dir(&site).map_or(0, |dir| dir.count());
    assert_eq!(written, 0);
}

#[test]
fn embed_leaves_out_a_bundles_description() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    let mut bundle = isotrail_testkit::hike_bundle();
    bundle.meta.desc = Some("Down from the summit".into());
    let itb = temp.path().join("hike.itb");
    std::fs::write(&itb, bundle.to_bytes().unwrap()).unwrap();
    for (flags, desc) in [
        (&[][..], Some("Down from the summit")),
        (&["--no-desc"][..], None),
    ] {
        let site = temp.path().join(format!("site{}", flags.len()));
        let args = [
            &[
                "embed",
                itb.to_str().unwrap(),
                "-o",
                site.to_str().unwrap(),
                "--scale",
                "6",
            ][..],
            flags,
        ]
        .concat();
        let output = isotrail(&cache_dir, &args);
        assert!(output.status.success(), "{}", text(&output.stderr));
        assert_eq!(
            read_bundle(&site.join("hike.itb")).meta.desc.as_deref(),
            desc
        );
    }
}
