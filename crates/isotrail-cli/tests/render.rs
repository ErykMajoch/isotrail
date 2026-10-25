use std::fs::{self, File};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use isotrail_geo::{
    Cache, CachedFetcher, Fetcher, Footprint, Grid, OVERPASS_URL, OverpassSource, RecordedFetcher,
    TerrariumSource, TileId,
};
use isotrail_testkit::{diorama_bundle, hike_bundle};
use isotrail_track::{GpxReader, TrackReader};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn isotrail(cache_dir: &Path, current_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(args)
        .current_dir(current_dir)
        .env("ISOTRAIL_CACHE_DIR", cache_dir)
        .env("ISOTRAIL_CONFIG", current_dir.join("user-config.toml"))
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

fn png_corner(path: &Path) -> [u8; 4] {
    let decoder = png::Decoder::new(std::io::BufReader::new(File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut rgba).unwrap();
    [rgba[0], rgba[1], rgba[2], rgba[3]]
}

/// A temporary directory holding `hike.itb`
fn with_hike() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("hike.itb"), hike_bundle().to_bytes()).unwrap();
    temp
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

#[test]
fn palettes_come_by_name_or_from_a_hex_file() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("hill.itb"), diorama_bundle().to_bytes()).unwrap();
    fs::write(temp.path().join("mono.hex"), "000000\r\n#ffffff\r\n").unwrap();
    fs::write(temp.path().join("broken.hex"), "000000\nnot-a-colour\n").unwrap();
    let cache = temp.path().join("cache");
    let render = |image: &str, palette: &str| {
        isotrail(
            &cache,
            temp.path(),
            &["render", "hill.itb", "-o", image, "--palette", palette],
        )
    };
    let mut images = Vec::new();
    for (image, palette) in [
        ("default.png", "apollo"),
        ("named.png", "AAP-64"),
        ("file.png", "mono.hex"),
        ("none.png", "none"),
    ] {
        let output = render(image, palette);
        assert!(output.status.success(), "{}", text(&output.stderr));
        images.push(fs::read(temp.path().join(image)).unwrap());
    }
    for (i, a) in images.iter().enumerate() {
        for b in &images[i + 1..] {
            assert_ne!(a, b);
        }
    }
    let output = isotrail(
        &cache,
        temp.path(),
        &["render", "hill.itb", "-o", "plain.png"],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(fs::read(temp.path().join("plain.png")).unwrap(), images[0]);

    let output = render("x.png", "sepia");
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("unknown palette \"sepia\": expected apollo, resurrect-64, aap-64"),
        "{stderr}"
    );
    let output = render("x.png", "broken.hex");
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("line 2"), "{stderr}");
    assert!(!temp.path().join("x.png").exists());
}

#[test]
fn every_aspect_gets_its_own_image() {
    let temp = with_hike();
    let output = isotrail(
        &temp.path().join("cache"),
        temp.path(),
        &["render", "hike.itb", "--aspect", "story,feed,square,wide"],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    for (aspect, size, internal) in [
        ("story", (1080, 1920), "360 × 640"),
        ("feed", (1080, 1350), "360 × 450"),
        ("square", (1080, 1080), "360 × 360"),
        ("wide", (1920, 1080), "640 × 360"),
    ] {
        let image = temp.path().join(format!("hike-{aspect}.png"));
        assert_eq!(png_size(&image), size, "{aspect}");
        assert!(
            stdout.contains(&format!("from {internal} pixels")),
            "{stdout}"
        );
    }
    assert!(!stdout.contains("Credit"), "{stdout}");
    assert!(!temp.path().join("hike.png").exists());

    let output = isotrail(
        &temp.path().join("cache"),
        temp.path(),
        &["render", "hike.itb", "--aspect", "feed,story,feed"],
    );
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("--aspect lists feed twice"), "{stderr}");
}

#[test]
fn the_scale_must_divide_every_size() {
    let temp = with_hike();
    let cache = temp.path().join("cache");
    let output = isotrail(&cache, temp.path(), &["render", "hike.itb", "--scale", "5"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).starts_with(
        "Rendered 1080 × 1920 from 216 × 384 pixels
"
    ));
    let output = isotrail(&cache, temp.path(), &["render", "hike.itb", "--scale", "4"]);
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("must be 2, 3, 5, 6 or 10"), "{stderr}");
}

#[test]
fn backgrounds_can_be_transparent_or_a_colour() {
    let temp = with_hike();
    let cache = temp.path().join("cache");
    for (image, background, corner) in [
        ("clear.png", "transparent", [0, 0, 0, 0]),
        ("plum.png", "#5a2d4e", [0x5a, 0x2d, 0x4e, 255]),
    ] {
        let output = isotrail(
            &cache,
            temp.path(),
            &[
                "render",
                "hike.itb",
                "-o",
                image,
                "--background",
                background,
            ],
        );
        assert!(output.status.success(), "{}", text(&output.stderr));
        assert_eq!(png_corner(&temp.path().join(image)), corner, "{background}");
    }
    let output = isotrail(
        &cache,
        temp.path(),
        &["render", "hike.itb", "--background", "plum"],
    );
    assert!(!output.status.success());
}

#[test]
fn a_credit_left_off_the_image_is_printed_for_the_caption() {
    let temp = with_hike();
    let cache = temp.path().join("cache");
    for flags in [&["--no-attribution"][..], &["--overlay", "none"][..]] {
        let mut args = vec!["render", "hike.itb"];
        args.extend_from_slice(flags);
        let output = isotrail(&cache, temp.path(), &args);
        assert!(output.status.success(), "{}", text(&output.stderr));
        let stdout = text(&output.stdout);
        assert!(
            stdout.ends_with(
                "Credit for the caption: © Synthetic contributors · Made-up Tiles
"
            ),
            "{stdout}"
        );
    }
}

#[test]
fn settings_come_from_flags_then_the_given_file_then_the_user_file() {
    let temp = with_hike();
    let cache = temp.path().join("cache");
    fs::write(
        temp.path().join("user-config.toml"),
        "[render]
scale = 5
aspect = \"wide\"
background = \"transparent\"
",
    )
    .unwrap();
    fs::write(
        temp.path().join("hike.toml"),
        "[render]
aspect = [\"square\"]
no-attribution = true
",
    )
    .unwrap();
    let output = isotrail(
        &cache,
        temp.path(),
        &[
            "render",
            "hike.itb",
            "--config",
            "hike.toml",
            "--scale",
            "6",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    assert!(
        stdout.starts_with(
            "Rendered 1080 × 1080 from 180 × 180 pixels
"
        ),
        "{stdout}"
    );
    assert!(stdout.contains("Credit for the caption"), "{stdout}");
    assert_eq!(png_corner(&temp.path().join("hike.png"))[3], 0);

    // Only the user file
    let output = isotrail(&cache, temp.path(), &["render", "hike.itb"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).starts_with(
        "Rendered 1920 × 1080 from 384 × 216 pixels
"
    ));

    fs::write(
        temp.path().join("bad.toml"),
        "[render]
pallete = \"apollo\"
",
    )
    .unwrap();
    let output = isotrail(
        &cache,
        temp.path(),
        &["render", "hike.itb", "--config", "bad.toml"],
    );
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("unknown setting \"pallete\""), "{stderr}");
}
