use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use isotrail_testkit::hike_bundle;

/// Nothing listens here, so a download that shouldn't happen fails at once
const CLOSED_TILES: &str = "http://127.0.0.1:1/{z}/{x}/{y}.png";
const CLOSED_OVERPASS: &str = "http://127.0.0.1:1/api/interpreter";

/// Words from Rust and the operating system that a message must not show
const LEAKS: [&str; 6] = ["digit", "literal", "..=", "stream", "Caused by", "os error"];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn isotrail<S: AsRef<OsStr>>(dir: &Path, args: &[S]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(args)
        .current_dir(dir)
        .env("ISOTRAIL_CACHE_DIR", dir.join("cache"))
        .env("ISOTRAIL_CONFIG", "")
        .env("ISOTRAIL_TERRAIN_URL", CLOSED_TILES)
        .env("ISOTRAIL_OVERPASS_URL", CLOSED_OVERPASS)
        .env("ISOTRAIL_WEATHER_URL", "http://127.0.0.1:1")
        .env_remove("ISOTRAIL_OPEN_METEO_KEY")
        .env_remove("ISOTRAIL_FFMPEG")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A folder holding `hike.gpx` (the Snowdon recording) and `hike.itb`
fn with_hike() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::copy(
        fixtures().join("gpx/snowdon.gpx"),
        temp.path().join("hike.gpx"),
    )
    .unwrap();
    fs::write(
        temp.path().join("hike.itb"),
        hike_bundle().to_bytes().unwrap(),
    )
    .unwrap();
    temp
}

/// The exit code, and the error alone on its first line with nothing leaked
fn assert_error(output: &Output, code: i32, message: &str) {
    let stderr = stderr(output);
    assert_eq!(output.status.code(), Some(code), "{stderr}");
    let first = stderr.lines().next().unwrap_or_default();
    assert!(first.starts_with("error: "), "{stderr}");
    assert!(first.contains(message), "{stderr}");
    for leak in LEAKS {
        assert!(!stderr.contains(leak), "{leak} in {stderr}");
    }
}

#[test]
fn the_size_named_in_the_help_renders_at_the_defaults() {
    let temp = with_hike();
    let help = isotrail(temp.path(), &["render", "--help"]);
    let help = String::from_utf8_lossy(&help.stdout);
    let option = help.find("--size").expect("--size in the help");
    let at = option + help[option..].find("such as ").expect("an example size");
    let size: String = help[at + "such as ".len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == 'x')
        .collect();
    assert!(size.contains('x'), "{size}");
    let output = isotrail(
        temp.path(),
        &["render", "hike.itb", "--size", &size, "-o", "sized.png"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(temp.path().join("sized.png").is_file());
}

#[test]
fn a_hike_without_a_command_is_shown_the_render_command() {
    let temp = with_hike();
    let output = isotrail(temp.path(), &["hike.gpx"]);
    assert_error(
        &output,
        2,
        "hike.gpx isn't a command; to make a picture of it, type: isotrail render hike.gpx",
    );
}

#[test]
fn a_folder_is_not_a_hike() {
    let temp = with_hike();
    fs::create_dir(temp.path().join("site")).unwrap();
    for name in ["site", "."] {
        let output = isotrail(temp.path(), &["render", name]);
        assert_error(
            &output,
            1,
            &format!("{name} is a folder; give a GPX file or a scene bundle (.itb)"),
        );
    }
    let output = isotrail(temp.path(), &["stats", "site"]);
    assert_error(&output, 1, "site is a folder");
}

#[test]
fn a_text_file_is_not_a_gpx_file() {
    let temp = with_hike();
    fs::write(temp.path().join("notes.txt"), "Walked up Snowdon\n").unwrap();
    let output = isotrail(temp.path(), &["render", "notes.txt"]);
    assert_error(
        &output,
        1,
        "can't read notes.txt: it holds plain text, not GPX",
    );
}

#[test]
fn a_picture_or_video_is_not_a_hike() {
    let temp = with_hike();
    fs::write(temp.path().join("hike.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    fs::write(temp.path().join("clip.mp4"), b"\0\0\0\x18ftypisom").unwrap();
    let output = isotrail(temp.path(), &["render", "hike.png"]);
    assert_error(
        &output,
        1,
        "hike.png is an image, not a GPX file or a scene bundle (.itb)",
    );
    assert_eq!(
        fs::read(temp.path().join("hike.png")).unwrap(),
        b"\x89PNG\r\n\x1a\n"
    );
    let output = isotrail(temp.path(), &["bundle", "clip.mp4", "-o", "clip.itb"]);
    assert_error(&output, 1, "clip.mp4 is a video");
}

#[test]
fn names_at_the_start_of_a_message_keep_their_capitals() {
    let temp = with_hike();
    fs::create_dir(temp.path().join("Hikes")).unwrap();
    fs::write(temp.path().join("Photo.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    let output = isotrail(temp.path(), &["render", "Hikes"]);
    assert_error(&output, 1, "error: Hikes is a folder");
    let output = isotrail(temp.path(), &["render", "Photo.png", "-o", "out.png"]);
    assert_error(&output, 1, "error: Photo.png is an image");
    let output = isotrail(temp.path(), &["batch", "Hikes", "-o", "site"]);
    assert_error(&output, 1, "error: Hikes has no GPX files in it");
    let output = isotrail(temp.path(), &["batch", "Photo.png", "-o", "site"]);
    assert_error(&output, 1, "error: Photo.png is a file");
}

#[test]
fn offline_with_an_empty_cache_says_to_go_online_once() {
    let temp = with_hike();
    let output = Command::new(env!("CARGO_BIN_EXE_isotrail"))
        .args(["render", "hike.gpx", "--offline"])
        .current_dir(temp.path())
        .env("ISOTRAIL_CACHE_DIR", temp.path().join("cache"))
        .env("ISOTRAIL_CONFIG", "")
        .env_remove("ISOTRAIL_TERRAIN_URL")
        .env_remove("ISOTRAIL_OVERPASS_URL")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_error(
        &output,
        1,
        "the elevation data for this area isn't in the cache yet; run once without --offline \
         to download it",
    );
}

#[test]
fn part_of_an_option_name_points_to_the_whole_one() {
    let temp = with_hike();
    let output = isotrail(temp.path(), &["render", "hike.itb", "--colour", "red"]);
    let partial = stderr(&output);
    assert_eq!(output.status.code(), Some(2), "{partial}");
    assert!(partial.contains("'--route-colour'"), "{partial}");
    assert!(!partial.contains("-- --colour"), "{partial}");
    let output = isotrail(temp.path(), &["render", "hike.itb", "--no-credit"]);
    let unknown = stderr(&output);
    assert_eq!(output.status.code(), Some(2), "{unknown}");
    assert!(
        unknown.contains("unexpected argument '--no-credit'"),
        "{unknown}"
    );
    assert!(!unknown.contains("tip:"), "{unknown}");
}

#[test]
fn value_errors_name_the_range_in_plain_words() {
    let temp = with_hike();
    fs::write(
        temp.path().join("three.toml"),
        "[render]\nscale = \"three\"\n",
    )
    .unwrap();
    let mut utf16 = vec![0xff, 0xfe];
    utf16.extend("[render]\n".encode_utf16().flat_map(u16::to_le_bytes));
    fs::write(temp.path().join("utf16.toml"), utf16).unwrap();
    fn render<'a>(extra: &[&'a str]) -> Vec<&'a str> {
        [&["render", "hike.gpx"][..], extra].concat()
    }
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (
            render(&["--detail", "abc"]),
            "expected a whole number from 8 to 1024",
        ),
        (
            render(&["--fps", "x"]),
            "expected a whole number from 1 to 60",
        ),
        (
            render(&["--fps", "0"]),
            "expected a whole number from 1 to 60",
        ),
        (
            render(&["--crf", "x"]),
            "expected a whole number from 0 to 51",
        ),
        (
            render(&["--crf", "52"]),
            "expected a whole number from 0 to 51",
        ),
        (
            render(&["--gif-scale", "0"]),
            "expected a whole number from 1 to 16",
        ),
        (
            render(&["--scale", "three"]),
            "expected a whole number from 1 to 16",
        ),
        (render(&["--pitch", "x"]), "expected a number from 10 to 80"),
        (render(&["--pitch", ""]), "expected a number from 10 to 80"),
        (
            render(&["--pitch", "-30"]),
            "expected a number from 10 to 80",
        ),
        (
            render(&["--exaggeration", "x"]),
            "expected a number from 0.1 to 100",
        ),
        (
            render(&["--safe-area", "-1"]),
            "expected each margin from 0 to 45 percent",
        ),
        (
            render(&["--safe-area", "-1,2,3,4"]),
            "expected each margin from 0 to 45",
        ),
        (
            render(&["--outlines", "-1"]),
            "expected a number from 0 to 2",
        ),
        (render(&["--padding", "-5"]), "from 0 to 100"),
        (render(&["--hold", "-1"]), "expected seconds from 0 to 180"),
        (
            render(&["--orbit", "-400"]),
            "expected a number from -360 to 360",
        ),
        (
            vec!["cache", "clear", "--older-than", "soon"],
            "expected a whole number of days",
        ),
        (
            render(&["--config", "three.toml"]),
            "scale in three.toml: expected a whole number",
        ),
        (render(&["--config", "utf16.toml"]), "is saved as UTF-16"),
    ];
    for (args, message) in cases {
        let output = isotrail(temp.path(), &args);
        assert_error(&output, 2, message);
    }
}

#[test]
fn file_errors_are_one_plain_line() {
    let temp = with_hike();
    let output = isotrail(temp.path(), &["render", "nothere.gpx"]);
    assert_error(&output, 1, "can't open nothere.gpx: the file doesn't exist");
    assert_eq!(stderr(&output).lines().count(), 1, "{}", stderr(&output));

    let output = isotrail(
        temp.path(),
        &["render", "hike.itb", "--theme", "missing.toml"],
    );
    assert_error(
        &output,
        2,
        "can't open the theme missing.toml: the file doesn't exist",
    );
    assert_eq!(stderr(&output).matches("missing.toml").count(), 1);

    // An output folder that is really a file
    fs::write(temp.path().join("taken"), "").unwrap();
    let output = isotrail(temp.path(), &["render", "hike.itb", "-o", "taken/x.png"]);
    assert_error(&output, 1, "can't create the folder taken");
    assert_eq!(stderr(&output).lines().count(), 1, "{}", stderr(&output));
}

#[cfg(windows)]
#[test]
fn a_read_only_target_says_so_on_one_line() {
    let temp = with_hike();
    let target = temp.path().join("kept.png");
    fs::write(&target, "old").unwrap();
    let mut permissions = fs::metadata(&target).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&target, permissions).unwrap();
    let output = isotrail(
        temp.path(),
        &[
            "render",
            "hike.itb",
            "--scale",
            "10",
            "--overlay",
            "none",
            "-o",
            "kept.png",
        ],
    );
    assert_error(&output, 1, "can't write kept.png: permission denied");
    assert_eq!(stderr(&output).lines().count(), 1, "{}", stderr(&output));
    assert_eq!(fs::read(&target).unwrap(), b"old");
}

#[test]
fn palette_file_mistakes_are_usage_errors_found_before_any_download() {
    let temp = with_hike();
    fs::write(temp.path().join("bad.hex"), "ff0000\nnot a colour\n").unwrap();
    for (palette, message) in [
        (
            "missing.hex",
            "can't open the palette missing.hex: the file doesn't exist",
        ),
        ("bad.hex", "can't read the palette bad.hex: line 2"),
    ] {
        let output = isotrail(temp.path(), &["render", "hike.gpx", "--palette", palette]);
        assert_error(&output, 2, message);
        assert!(stderr(&output).contains("Usage: isotrail render"));
        assert!(!temp.path().join("cache").exists());
    }
}

#[test]
fn other_exit_codes_stay_as_they_were() {
    let temp = with_hike();
    let output = isotrail(
        temp.path(),
        &[
            "embed",
            "hike.itb",
            "-o",
            "site",
            "--with-runtime",
            "--runtime-dir",
            "no-runtime",
        ],
    );
    assert_error(&output, 1, "can't find the web runtime in no-runtime");
    let output = isotrail(temp.path(), &["render", "hike.itb", "--aspect", "storey"]);
    assert_eq!(output.status.code(), Some(2));
    let output = isotrail(temp.path(), &["render", "hike.itb", "--size", "1200x800"]);
    assert_error(
        &output,
        2,
        "--scale 3 doesn't divide 1200 x 800; use --scale 1, 2, 4, 5, 8, 10 or 16",
    );
}
