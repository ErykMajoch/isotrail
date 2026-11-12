use std::f64::consts::FRAC_PI_2;
use std::ffi::OsString;
use std::fs::{self, File};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use isotrail_bundle::SceneBundle;
use isotrail_geo::{
    Cache, CachedFetcher, Fetcher, Footprint, Grid, OPEN_METEO_ARCHIVE_URL, OVERPASS_URL,
    OpenMeteoSource, OverpassSource, RecordedFetcher, TerrariumSource, TileId, WeatherRequest,
};
use isotrail_testkit::{decode_gif, diorama_bundle, has_ffmpeg, hike_bundle, probe_video};
use isotrail_track::{GpxReader, TrackReader};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn isotrail(cache_dir: &Path, current_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(args)
        .current_dir(current_dir)
        .env("ISOTRAIL_CACHE_DIR", cache_dir)
        .env("ISOTRAIL_CONFIG", user_config(current_dir))
        .env_remove("ISOTRAIL_OVERPASS_URL")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

/// `user-config.toml` in the current directory when there is one, else none
fn user_config(current_dir: &Path) -> OsString {
    let path = current_dir.join("user-config.toml");
    if path.is_file() {
        path.into_os_string()
    } else {
        OsString::new()
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
    fs::write(
        temp.path().join("hike.itb"),
        hike_bundle().to_bytes().unwrap(),
    )
    .unwrap();
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
    let weather = CachedFetcher::new(
        RecordedFetcher::new(fixtures().join("weather")),
        Cache::new(cache_dir),
    );
    let request = WeatherRequest::from_track(&track, 0).unwrap();
    let url = OpenMeteoSource::<RecordedFetcher>::url(OPEN_METEO_ARCHIVE_URL, &request);
    weather.get(&url).unwrap();
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
fn renders_a_gif_of_a_track_whose_times_run_backwards() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir);
    // The same walk in two segments sharing a point, so the slab is the one
    // cached, the second stamped a day before the first
    let gpx = fs::read_to_string(fixtures().join("gpx/snowdon.gpx")).unwrap();
    let point = |n: usize| gpx.match_indices("<trkpt").nth(n).unwrap().0;
    let (head, tail) = (&gpx[..point(8)], &gpx[point(7)..]);
    let tail = tail.replace("2025-06-14", "2025-06-13");
    fs::write(
        temp.path().join("jumped.gpx"),
        format!("{head}</trkseg>\n<trkseg>\n{tail}"),
    )
    .unwrap();
    let output = isotrail(
        &cache_dir,
        temp.path(),
        &[
            "render",
            "jumped.gpx",
            "-o",
            "jumped.gif",
            "--detail",
            "32",
            "--offline",
            "--no-weather",
            "--aspect",
            "square",
            "--scale",
            "10",
            "--duration",
            "1s",
            "--fps",
            "10",
            "--pace",
            "time",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stderr = text(&output.stderr);
    assert_eq!(
        stderr.matches("the times run backwards in 1 place").count(),
        1,
        "{stderr}"
    );
    let frames = decode_gif(&fs::read(temp.path().join("jumped.gif")).unwrap());
    assert_eq!(frames.len(), 10);
}

#[test]
fn terrain_too_big_for_the_gpu_names_the_flags_to_lower() {
    let temp = tempfile::tempdir().unwrap();
    let grid = isotrail_testkit::grid(1024, 1024, 10.0);
    let heights: Vec<f32> = (0..grid.cells())
        .map(|i| (i as u64 * 7_919 % 1_000) as f32)
        .collect();
    let classes = vec![isotrail_bundle::Landcover::Grassland; grid.cells()];
    let jagged = isotrail_testkit::bundle(grid, &heights, &classes);
    fs::write(temp.path().join("jagged.itb"), jagged.to_bytes().unwrap()).unwrap();
    let output = isotrail(
        &temp.path().join("cache"),
        temp.path(),
        &["render", "jagged.itb", "--exaggeration", "100"],
    );
    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stderr));
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("of GPU memory, more than this GPU allows"),
        "{stderr}"
    );
    assert!(stderr.contains("lower --exaggeration"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(!temp.path().join("jagged.png").exists());
}

#[test]
fn renders_a_bundle_into_the_current_directory_by_default() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("hill.itb"),
        diorama_bundle().to_bytes().unwrap(),
    )
    .unwrap();
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
        &["render", "missing.gpx", "-o", "hike.webm"],
    );
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("only .png, .gif and .mp4 output are supported"),
        "{stderr}"
    );
    assert!(!temp.path().join("hike.webm").exists());
}

#[test]
fn renders_an_animated_gif() {
    let temp = with_hike();
    let output = isotrail(
        &temp.path().join("cache"),
        temp.path(),
        &[
            "render",
            "hike.itb",
            "-o",
            "hike.gif",
            "--aspect",
            "square",
            "--scale",
            "10",
            "--duration",
            "1s",
            "--fps",
            "5",
            "--pace",
            "time",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    assert!(
        stdout.starts_with("Rendered 5 frames of 1080 × 1080 from 108 × 108 pixels\n"),
        "{stdout}"
    );
    // The synthetic hike has no times to pace by
    assert!(text(&output.stderr).contains("no times"));
    let frames = decode_gif(&fs::read(temp.path().join("hike.gif")).unwrap());
    assert_eq!(frames.len(), 5);
    assert_eq!((frames[0].width, frames[0].height), (1080, 1080));
    assert_eq!(frames.iter().map(|f| f.delay_cs).sum::<u16>(), 100);
    assert_ne!(frames[0].rgba, frames[4].rgba);
}

#[test]
fn renders_an_mp4() {
    if !has_ffmpeg() {
        return;
    }
    let temp = with_hike();
    let output = isotrail(
        &temp.path().join("cache"),
        temp.path(),
        &[
            "render",
            "hike.itb",
            "-o",
            "hike.mp4",
            "--aspect",
            "square",
            "--scale",
            "10",
            "--duration",
            "1s",
            "--fps",
            "5",
            "--palette",
            "none",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    assert!(
        stdout.starts_with(
            "Rendered 5 frames of 1080 × 1080 from 108 × 108 pixels
"
        ),
        "{stdout}"
    );
    let info = probe_video(&temp.path().join("hike.mp4"));
    assert_eq!(info.codec, "h264");
    assert_eq!(info.pixel_format, "yuv420p");
    assert_eq!((info.width, info.height), (1080, 1080));
    assert_eq!(info.frames, 5);
    assert_eq!(info.frame_rate, "5/1");
}

#[test]
fn transparent_mp4s_are_refused_before_any_work() {
    let temp = with_hike();
    let output = isotrail(
        &temp.path().join("cache"),
        temp.path(),
        &[
            "render",
            "hike.itb",
            "-o",
            "hike.mp4",
            "--background",
            "transparent",
        ],
    );
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("MP4 output can't be transparent"),
        "{stderr}"
    );
    assert!(!temp.path().join("hike.mp4").exists());
}

#[test]
fn animation_settings_are_checked_before_any_work() {
    let temp = with_hike();
    let refused = |args: &[&str]| {
        let mut line = vec!["render", "hike.itb", "-o", "hike.gif"];
        line.extend(args);
        let output = isotrail(&temp.path().join("cache"), temp.path(), &line);
        assert!(!output.status.success(), "{args:?}");
        text(&output.stderr)
    };
    let stderr = refused(&["--fps", "12", "--step-fps", "20"]);
    assert!(
        stderr.contains("--step-fps 20 is more than --fps 12"),
        "{stderr}"
    );
    let stderr = refused(&["--duration", "0.5s"]);
    assert!(stderr.contains("between 1s and 60s"), "{stderr}");
    let stderr = refused(&["--fps", "60"]);
    assert!(stderr.contains("60"), "{stderr}");
    let stderr = refused(&["--orbit", "400"]);
    assert!(stderr.contains("between 0 and 360"), "{stderr}");
    let stderr = refused(&["--palette", "none"]);
    assert!(stderr.contains("GIF output needs a palette"), "{stderr}");
    assert!(!temp.path().join("hike.gif").exists());
}

#[test]
fn both_meshers_render_and_unknown_ones_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("hill.itb"),
        diorama_bundle().to_bytes().unwrap(),
    )
    .unwrap();
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
    fs::write(
        temp.path().join("hill.itb"),
        diorama_bundle().to_bytes().unwrap(),
    )
    .unwrap();
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

    // Typed, it does nothing but warn; from a config file, not even that
    let output = render("x.png", &["--mesher", "smooth", "--block-height", "2"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("only applies to --mesher voxel"),
        "{stderr}"
    );
    fs::write(
        temp.path().join("user-config.toml"),
        "[render]\nblock-height = 2\n",
    )
    .unwrap();
    let output = render("y.png", &["--mesher", "smooth"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stderr = text(&output.stderr);
    assert!(!stderr.contains("block-height"), "{stderr}");
    assert_eq!(
        fs::read(temp.path().join("x.png")).unwrap(),
        fs::read(temp.path().join("y.png")).unwrap()
    );
}

#[test]
fn palettes_come_by_name_or_from_a_hex_file() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("hill.itb"),
        diorama_bundle().to_bytes().unwrap(),
    )
    .unwrap();
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
        &[
            "render",
            "hike.itb",
            "--aspect",
            "story,video,portrait,feed,square,wide",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    for (aspect, size, internal) in [
        ("story", (1080, 1920), "360 × 640"),
        ("video", (1080, 1920), "360 × 640"),
        ("portrait", (1080, 1440), "360 × 480"),
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
fn the_scale_must_divide_both_sides() {
    let temp = with_hike();
    let cache = temp.path().join("cache");
    let output = isotrail(&cache, temp.path(), &["render", "hike.itb", "--scale", "5"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).starts_with(
        "Rendered 1080 × 1920 from 216 × 384 pixels
"
    ));
    let output = isotrail(&cache, temp.path(), &["render", "hike.itb", "--scale", "4"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).starts_with("Rendered 1080 × 1920 from 270 × 480 pixels\n"));
    let output = isotrail(&cache, temp.path(), &["render", "hike.itb", "--scale", "7"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("--scale 7 doesn't divide the 1080 × 1920 image (story)"),
        "{stderr}"
    );
    let output = isotrail(
        &cache,
        temp.path(),
        &["render", "hike.itb", "--scale", "17"],
    );
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("must be a whole number from 1 to 16"),
        "{stderr}"
    );
}

#[test]
fn any_size_renders_with_its_own_safe_area() {
    let temp = with_hike();
    let cache = temp.path().join("cache");
    let output = isotrail(
        &cache,
        temp.path(),
        &[
            "render", "hike.itb", "--size", "1000x700", "--scale", "3", "-o", "a.png",
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{}", text(&output.stderr));
    assert!(text(&output.stderr).contains("--scale 3 doesn't divide the 1000 × 700 image"));

    let output = isotrail(
        &cache,
        temp.path(),
        &[
            "render", "hike.itb", "--size", "1000x700", "--scale", "5", "-o", "a.png",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).starts_with("Rendered 1000 × 700 from 200 × 140 pixels\n"));
    assert_eq!(png_size(&temp.path().join("a.png")), (1000, 700));

    let output = isotrail(
        &cache,
        temp.path(),
        &[
            "render",
            "hike.itb",
            "--size",
            "1000x700",
            "--scale",
            "5",
            "--safe-area",
            "10,30,10,5",
            "-o",
            "b.png",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_ne!(
        fs::read(temp.path().join("a.png")).unwrap(),
        fs::read(temp.path().join("b.png")).unwrap()
    );

    // A size and an aspect can't both be typed, but a typed size beats a
    // configured aspect
    let output = isotrail(
        &cache,
        temp.path(),
        &[
            "render", "hike.itb", "--size", "600x400", "--aspect", "wide",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).contains("cannot be used with"));
    fs::write(
        temp.path().join("user-config.toml"),
        "[render]\naspect = [\"story\", \"wide\"]\nsafe-area = [10, 5, 10, 5]\n",
    )
    .unwrap();
    let output = isotrail(
        &cache,
        temp.path(),
        &[
            "render", "hike.itb", "--size", "600x400", "--scale", "2", "-o", "c.png",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(png_size(&temp.path().join("c.png")), (600, 400));

    // An MP4 needs even sides, checked before any work
    let output = isotrail(
        &cache,
        temp.path(),
        &[
            "render", "hike.itb", "--size", "601x400", "--scale", "1", "-o", "d.mp4",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).contains("an even number of pixels"));
}

#[test]
fn the_camera_can_turn_tilt_and_zoom() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("hill.itb"),
        diorama_bundle().to_bytes().unwrap(),
    )
    .unwrap();
    let cache = temp.path().join("cache");
    let render = |image: &str, extra: &[&str]| {
        let mut args = vec!["render", "hill.itb", "--scale", "6", "-o", image];
        args.extend_from_slice(extra);
        isotrail(&cache, temp.path(), &args)
    };
    let mut images = Vec::new();
    for (image, extra) in [
        ("plain.png", &[][..]),
        ("steep.png", &["--pitch", "60"][..]),
        ("turned.png", &["--turn", "-90"][..]),
        ("north.png", &["--heading", "0"][..]),
        ("close.png", &["--zoom", "1.5"][..]),
    ] {
        let output = render(image, extra);
        assert!(output.status.success(), "{image}: {}", text(&output.stderr));
        images.push(fs::read(temp.path().join(image)).unwrap());
    }
    for (i, a) in images.iter().enumerate() {
        for b in &images[i + 1..] {
            assert_ne!(a, b);
        }
    }

    // The defaults spelled out change nothing
    let output = render("same.png", &["--pitch", "30", "--turn", "0", "--zoom", "1"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(fs::read(temp.path().join("same.png")).unwrap(), images[0]);

    // From a config file too
    fs::write(
        temp.path().join("user-config.toml"),
        "[render]\npitch = 60\n",
    )
    .unwrap();
    let output = render("steep-config.png", &[]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(
        fs::read(temp.path().join("steep-config.png")).unwrap(),
        images[1]
    );

    for (flag, value, reason) in [
        ("--pitch", "85", "between 10 and 80"),
        ("--turn", "400", "between -360 and 360"),
        ("--heading", "400", "between 0 and 360"),
        ("--zoom", "3", "between 0.5 and 2"),
    ] {
        let output = render("bad.png", &[flag, value]);
        assert_eq!(output.status.code(), Some(2), "{flag} {value}");
        let stderr = text(&output.stderr);
        assert!(stderr.contains(reason), "{stderr}");
    }
}

#[test]
fn parts_of_the_scene_light_and_route_can_be_changed() {
    let temp = with_hike();
    let cache = temp.path().join("cache");
    let render = |image: &str, extra: &[&str]| {
        let mut args = vec!["render", "hike.itb", "--scale", "6", "-o", image];
        args.extend_from_slice(extra);
        isotrail(&cache, temp.path(), &args)
    };
    let night = ["--sun", "200,-20"];
    let rain = ["--weather", "rain"];
    let mut images = Vec::new();
    for (image, extra) in [
        ("plain.png", &[][..]),
        ("bare.png", &["--prop-density", "0"][..]),
        ("dense.png", &["--prop-density", "2"][..]),
        ("pathless.png", &["--paths", "off"][..]),
        ("dry.png", &["--streams", "off"][..]),
        ("stone.png", &["--strata", "off"][..]),
        ("thin.png", &["--base-blocks", "2"][..]),
        ("flat.png", &["--outlines", "0"][..]),
        ("bold.png", &["--outlines", "2"][..]),
        ("unshadowed.png", &["--shadows", "off"][..]),
        ("night.png", &night[..]),
        (
            "starless.png",
            &[night[0], night[1], "--night-sky", "off"][..],
        ),
        ("rain.png", &rain[..]),
        (
            "rain-unseen.png",
            &[rain[0], rain[1], "--weather-effects", "clouds"][..],
        ),
        ("fog.png", &["--weather", "fog"][..]),
        ("blue.png", &["--route-colour", "#2050e0"][..]),
        ("wide-line.png", &["--route-width", "4"][..]),
        ("unmarked.png", &["--markers", "none"][..]),
        ("column.png", &["--layout", "column"][..]),
        ("signed.png", &["--credit-extra", "Walked by A. Hiker"][..]),
    ] {
        let output = render(image, extra);
        assert!(output.status.success(), "{image}: {}", text(&output.stderr));
        images.push((image, fs::read(temp.path().join(image)).unwrap()));
    }
    for (i, (name, a)) in images.iter().enumerate() {
        for (other, b) in &images[i + 1..] {
            assert!(a != b, "{name} and {other} are the same");
        }
    }

    // The defaults spelled out change nothing, nor do effects with no weather
    let output = render(
        "same.png",
        &[
            "--prop-density",
            "1",
            "--paths",
            "on",
            "--strata",
            "on",
            "--base-blocks",
            "6",
            "--outlines",
            "1",
            "--shadows",
            "on",
            "--weather-effects",
            "none",
            "--route-color",
            "#D64834",
            "--route-width",
            "2",
            "--route-hidden",
            "dashed",
            "--markers",
            "start,finish,head",
            "--layout",
            "auto",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(fs::read(temp.path().join("same.png")).unwrap(), images[0].1);

    // From a config file too
    fs::write(
        temp.path().join("user-config.toml"),
        "[render]\nstrata = false\nweather = \"rain\"\n",
    )
    .unwrap();
    let output = render("config.png", &["--weather", "rain", "--strata", "on"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(
        fs::read(temp.path().join("config.png")).unwrap(),
        images[12].1
    );
    let output = render("config-stone.png", &["--weather", "clear"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let output = render(
        "stone-clear.png",
        &["--strata", "off", "--weather", "clear"],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(
        fs::read(temp.path().join("config-stone.png")).unwrap(),
        fs::read(temp.path().join("stone-clear.png")).unwrap()
    );
    fs::remove_file(temp.path().join("user-config.toml")).unwrap();

    for (args, reason) in [
        (&["--prop-density", "3"][..], "between 0 and 2"),
        (&["--paths", "maybe"][..], "expected on or off"),
        (&["--base-blocks", "30"][..], "between 1 and 24"),
        (&["--outlines", "3"][..], "between 0 and 2"),
        (
            &["--weather-effects", "wind"][..],
            "clouds, shade, rain, snow, haze",
        ),
        (&["--weather", "sunny"][..], "clear, cloudy, overcast"),
        (&["--sun", "200"][..], "AZIMUTH,ELEVATION"),
        (
            &["--sun", "200,20", "--time", "12:00"][..],
            "cannot be used with",
        ),
        (&["--route-colour", "red"][..], "#d64834"),
        (&["--route-width", "5"][..], "between 1 and 4"),
        (
            &["--route-hidden", "faint"][..],
            "dashed, dim, solid or off",
        ),
        (&["--markers", "flag"][..], "start, finish, head"),
        (&["--layout", "grid"][..], "auto, stacked or column"),
    ] {
        let output = render("bad.png", args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = text(&output.stderr);
        assert!(stderr.contains(reason), "{args:?}: {stderr}");
    }
}

#[test]
fn clips_keep_the_chosen_sky_sun_and_markers() {
    let temp = with_hike();
    let cache = temp.path().join("cache");
    let clip = |name: &str, extra: &[&str]| {
        let mut args = vec![
            "render",
            "hike.itb",
            "--scale",
            "10",
            "--duration",
            "1",
            "--fps",
            "4",
            "-o",
            name,
        ];
        args.extend_from_slice(extra);
        let output = isotrail(&cache, temp.path(), &args);
        assert!(output.status.success(), "{name}: {}", text(&output.stderr));
        decode_gif(&fs::read(temp.path().join(name)).unwrap())
    };
    let plain = clip("plain.gif", &[]);
    let foggy = clip("fog.gif", &["--weather", "fog"]);
    let held = clip("held.gif", &["--sun", "250,15", "--markers", "none"]);
    assert_eq!(plain.len(), foggy.len());
    for frames in [&foggy, &held] {
        assert!(frames.iter().zip(&plain).all(|(a, b)| a != b));
    }
}

#[test]
fn the_slab_can_be_padded_and_turned_north() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    warm(&cache_dir);
    let gpx = fixtures().join("gpx/snowdon.gpx");
    let bundle = |name: &str, extra: &[&str]| {
        let mut args = vec![
            "bundle",
            gpx.to_str().unwrap(),
            "-o",
            name,
            "--detail",
            "32",
            "--offline",
            "--no-map-data",
            "--no-weather",
        ];
        args.extend_from_slice(extra);
        let output = isotrail(&cache_dir, temp.path(), &args);
        assert!(output.status.success(), "{name}: {}", text(&output.stderr));
        SceneBundle::from_bytes(&fs::read(temp.path().join(name)).unwrap()).unwrap()
    };
    let plain = bundle("plain.itb", &[]);
    let north = bundle("north.itb", &["--footprint", "north"]);
    assert_ne!(plain.grid.rotation_rad, 0.0);
    assert!(
        north.grid.rotation_rad.abs() < 1e-9 || (north.grid.rotation_rad - FRAC_PI_2).abs() < 1e-9
    );
    let tight = bundle("tight.itb", &["--padding", "5,200"]);
    let length = |b: &SceneBundle| b.grid.cols as f64 * b.grid.cell_size_m;
    assert!(
        length(&tight) < length(&plain),
        "{} {}",
        length(&tight),
        length(&plain)
    );
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
fn no_weather_leaves_out_the_weather_a_bundle_holds() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let mut bundle = hike_bundle();
    bundle.weather = Some(isotrail_testkit::synthetic_weather(1_750_000_000, 3));
    bundle.sun = vec![isotrail_bundle::SunSample {
        time: 1_750_003_600,
        azimuth_deg: 150.0,
        elevation_deg: 40.0,
    }];
    bundle.moon = vec![isotrail_bundle::MoonSample {
        time: 1_750_003_600,
        azimuth_deg: 20.0,
        elevation_deg: -30.0,
        illuminated: 0.5,
    }];
    fs::write(temp.path().join("wet.itb"), bundle.to_bytes().unwrap()).unwrap();
    let render = |extra: &[&str], output: &str| {
        let mut args = vec!["render", "wet.itb", "--no-attribution", "-o", output];
        args.extend_from_slice(extra);
        let output = isotrail(&cache, temp.path(), &args);
        assert!(output.status.success(), "{}", text(&output.stderr));
        (
            text(&output.stdout),
            fs::read(temp.path().join(args[4])).unwrap(),
        )
    };
    let (wet, wet_png) = render(&[], "wet.png");
    assert!(
        wet.ends_with(
            "Made-up Tiles · Weather data by Open-Meteo.com
"
        ),
        "{wet}"
    );
    let (dry, dry_png) = render(&["--no-weather"], "dry.png");
    assert!(
        dry.ends_with(
            "Made-up Tiles
"
        ),
        "{dry}"
    );
    assert!(wet_png != dry_png, "the weather made no difference");
}

#[test]
fn seasons_can_be_forced_and_snow_and_ice_go_with_the_weather() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let mut snowy = diorama_bundle();
    snowy.season = isotrail_bundle::Season {
        leaf_day: Some(15),
        snow_line_m: Some(400.0),
        ice_line_m: Some(0.0),
    };
    let mut bare = snowy.clone();
    bare.season.snow_line_m = None;
    bare.season.ice_line_m = None;
    fs::write(temp.path().join("snowy.itb"), snowy.to_bytes().unwrap()).unwrap();
    fs::write(temp.path().join("bare.itb"), bare.to_bytes().unwrap()).unwrap();
    fs::write(
        temp.path().join("winter.toml"),
        "[render]
season = \"winter\"
",
    )
    .unwrap();
    let render = |input: &str, image: &str, extra: &[&str]| {
        let mut args = vec!["render", input, "-o", image];
        args.extend_from_slice(extra);
        let output = isotrail(&cache, temp.path(), &args);
        assert!(output.status.success(), "{}", text(&output.stderr));
        fs::read(temp.path().join(image)).unwrap()
    };
    let bare_png = render("bare.itb", "bare.png", &[]);
    assert_ne!(render("snowy.itb", "snowy.png", &[]), bare_png);
    assert_eq!(
        render("snowy.itb", "dry.png", &["--no-weather"]),
        bare_png,
        "snow and ice come from the weather"
    );
    let winter = render("bare.itb", "winter.png", &["--season", "winter"]);
    assert_ne!(winter, bare_png);
    assert_eq!(
        render("bare.itb", "configured.png", &["--config", "winter.toml"]),
        winter
    );
    assert_ne!(
        render("bare.itb", "autumn.png", &["--season", "autumn"]),
        bare_png
    );

    let output = isotrail(
        &cache,
        temp.path(),
        &["render", "bare.itb", "--season", "monsoon"],
    );
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("spring, summer, autumn, winter"),
        "{stderr}"
    );
}

#[test]
fn lighting_can_be_turned_down_by_flag_or_config() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    fs::write(
        temp.path().join("hill.itb"),
        diorama_bundle().to_bytes().unwrap(),
    )
    .unwrap();
    fs::write(
        temp.path().join("flat.toml"),
        "[render]
occlusion = 0
soft-shadows = 0
god-rays = \"off\"
",
    )
    .unwrap();
    let render = |image: &str, extra: &[&str]| {
        let mut args = vec!["render", "hill.itb", "-o", image];
        args.extend_from_slice(extra);
        let output = isotrail(&cache, temp.path(), &args);
        assert!(output.status.success(), "{}", text(&output.stderr));
        fs::read(temp.path().join(image)).unwrap()
    };
    let full = render("full.png", &[]);
    let flat = render(
        "flat.png",
        &[
            "--occlusion",
            "0",
            "--soft-shadows",
            "0",
            "--god-rays",
            "off",
        ],
    );
    assert_ne!(full, flat);
    assert_eq!(render("configured.png", &["--config", "flat.toml"]), flat);
    assert_ne!(render("rays.png", &["--god-rays", "on"]), full);

    for (flag, value, reason) in [
        ("--occlusion", "1.5", "between 0 and 1"),
        ("--soft-shadows", "5", "between 0 and 4"),
        ("--god-rays", "always", "auto, on, off"),
    ] {
        let output = isotrail(&cache, temp.path(), &["render", "hill.itb", flag, value]);
        assert!(!output.status.success(), "{flag} {value}");
        let stderr = text(&output.stderr);
        assert!(stderr.contains(reason), "{stderr}");
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

#[test]
fn the_time_of_day_relights_the_scene() {
    let temp = with_hike();
    let cache = temp.path().join("cache");
    fs::write(
        temp.path().join("hill.itb"),
        diorama_bundle().to_bytes().unwrap(),
    )
    .unwrap();
    let render = |input: &str, extra: &[&str]| {
        let mut args = vec!["render", input];
        args.extend_from_slice(extra);
        isotrail(&cache, temp.path(), &args)
    };
    for (image, time) in [("start.png", None), ("night.png", Some("21:00"))] {
        let mut extra = vec!["-o", image];
        extra.extend(time.map(|time| ["--time", time]).into_iter().flatten());
        let output = render("hike.itb", &extra);
        assert!(output.status.success(), "{}", text(&output.stderr));
    }
    let (day, night) = (
        png_corner(&temp.path().join("start.png")),
        png_corner(&temp.path().join("night.png")),
    );
    let brightness = |rgba: [u8; 4]| rgba[..3].iter().map(|&c| u32::from(c)).sum::<u32>();
    assert!(brightness(night) * 2 < brightness(day), "{night:?} {day:?}");

    let output = render("hike.itb", &["--time", "9pm"]);
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("such as 20:30"));
    let output = render("hill.itb", &["--time", "21:00"]);
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("track with times"));
}

#[test]
fn a_coarse_scale_gives_parts_up_and_moves_the_credit_to_the_caption() {
    let temp = with_hike();
    let cache = temp.path().join("cache");
    let output = isotrail(
        &cache,
        temp.path(),
        &[
            "render",
            "hike.itb",
            "--scale",
            "10",
            "--aspect",
            "story,wide",
        ],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stderr = text(&output.stderr);
    for frame in ["story", "wide"] {
        let warning = format!("the overlays don't fit the {frame} frame at --scale 10, so it has");
        assert!(stderr.contains(&warning), "{stderr}");
    }
    assert!(stderr.contains("no profile"), "{stderr}");
    let stdout = text(&output.stdout);
    assert_eq!(
        stdout.matches("Credit for the caption: ").count(),
        1,
        "{stdout}"
    );

    // A finer scale has room for everything
    let output = isotrail(&cache, temp.path(), &["render", "hike.itb", "--scale", "3"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(output.stderr.is_empty(), "{}", text(&output.stderr));
    assert!(!text(&output.stdout).contains("Credit for the caption"));
}
