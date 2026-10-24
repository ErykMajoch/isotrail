use std::fs::{self, File};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use isotrail_geo::{
    Cache, CachedFetcher, Fetcher, Footprint, Grid, OVERPASS_URL, OverpassSource, RecordedFetcher,
    TerrariumSource, TileId,
};
use isotrail_testkit::diorama_bundle;
use isotrail_track::{GpxReader, TrackReader};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn isotrail(cache_dir: &Path, current_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(args)
        .current_dir(current_dir)
        .env("ISOTRAIL_CACHE_DIR", cache_dir)
        .env_remove("ISOTRAIL_OVERPASS_URL")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn png_size(path: &Path) -> (u32, u32) {
    let decoder = png::Decoder::new(std::io::BufReader::new(File::open(path).unwrap()));
    let info = decoder.read_info().unwrap();
    (info.info().width, info.info().height)
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
    let body = OverpassSource::<RecordedFetcher>::request_body(&grid);
    osm.post(OVERPASS_URL, &body).unwrap();
}

#[test]
fn renders_a_gpx_file_from_cached_data_as_a_story_png() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir);
    let image = temp.path().join("snowdon.png");
    let gpx = fixtures().join("gpx/snowdon.gpx");
    let output = isotrail(
        &cache_dir,
        temp.path(),
        &[
            "render",
            gpx.to_str().unwrap(),
            "-o",
            image.to_str().unwrap(),
            "--detail",
            "32",
            "--offline",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    assert!(
        stdout.starts_with("Rendered 1080 × 1920 from 360 × 640 pixels\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("Wrote {}\n", image.display())),
        "{stdout}"
    );
    assert_eq!(png_size(&image), (1080, 1920));
}

#[test]
fn renders_a_bundle_into_the_current_directory_by_default() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("hill.itb"), diorama_bundle().to_bytes()).unwrap();
    let output = isotrail(
        &temp.path().join("cache"),
        temp.path(),
        &["render", "hill.itb"],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(png_size(&temp.path().join("hill.png")), (1080, 1920));
}

#[test]
fn other_image_formats_are_refused_before_any_work() {
    let temp = tempfile::tempdir().unwrap();
    let output = isotrail(
        &temp.path().join("cache"),
        temp.path(),
        &["render", "missing.gpx", "-o", "hike.gif"],
    );
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("only .png output is supported"), "{stderr}");
    assert!(!temp.path().join("hike.gif").exists());
}

#[test]
fn both_meshers_render_and_unknown_ones_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("hill.itb"), diorama_bundle().to_bytes()).unwrap();
    let cache = temp.path().join("cache");
    for mesher in ["voxel", "smooth"] {
        let image = format!("{mesher}.png");
        let output = isotrail(
            &cache,
            temp.path(),
            &["render", "hill.itb", "-o", &image, "--mesher", mesher],
        );
        assert!(output.status.success(), "{}", text(&output.stderr));
        assert_eq!(png_size(&temp.path().join(&image)), (1080, 1920));
    }
    let voxel = fs::read(temp.path().join("voxel.png")).unwrap();
    assert_ne!(voxel, fs::read(temp.path().join("smooth.png")).unwrap());

    let output = isotrail(
        &cache,
        temp.path(),
        &["render", "hill.itb", "--mesher", "marching-cubes"],
    );
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("invalid value 'marching-cubes'"),
        "{stderr}"
    );
}

#[test]
fn block_height_and_exaggeration_change_the_image() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("hill.itb"), diorama_bundle().to_bytes()).unwrap();
    let cache = temp.path().join("cache");
    let render = |image: &str, extra: &[&str]| {
        let mut args = vec!["render", "hill.itb", "-o", image];
        args.extend_from_slice(extra);
        isotrail(&cache, temp.path(), &args)
    };
    let mut images = Vec::new();
    for (image, extra) in [
        ("plain.png", &[][..]),
        ("flat.png", &["--block-height", "0.5"][..]),
        ("tall.png", &["--exaggeration", "4"][..]),
    ] {
        let output = render(image, extra);
        assert!(output.status.success(), "{}", text(&output.stderr));
        images.push(fs::read(temp.path().join(image)).unwrap());
    }
    for (i, a) in images.iter().enumerate() {
        for b in &images[i + 1..] {
            assert_ne!(a, b);
        }
    }

    let output = render("x.png", &["--mesher", "smooth", "--block-height", "2"]);
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("only applies to --mesher voxel"),
        "{stderr}"
    );
}
