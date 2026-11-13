use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use isotrail_testkit::hike_bundle;
use serde_json::Value;

/// Nothing listens here, so a download fails at once
const CLOSED_TILES: &str = "http://127.0.0.1:1/{z}/{x}/{y}.png";
const CLOSED_OVERPASS: &str = "http://127.0.0.1:1/api/interpreter";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn command<S: AsRef<OsStr>>(dir: &Path, args: &[S]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_isotrail"));
    command
        .args(args)
        .current_dir(dir)
        .env("ISOTRAIL_CACHE_DIR", dir.join("cache"))
        .env("ISOTRAIL_CONFIG", "")
        .env("ISOTRAIL_TERRAIN_URL", CLOSED_TILES)
        .env("ISOTRAIL_OVERPASS_URL", CLOSED_OVERPASS)
        .env("ISOTRAIL_WEATHER_URL", "http://127.0.0.1:1")
        .env_remove("ISOTRAIL_OPEN_METEO_KEY")
        .env_remove("ISOTRAIL_FFMPEG")
        .stdin(Stdio::null());
    command
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

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The error and what follows it, without the progress lines that --json
/// leaves out
fn from_error(stderr: &str) -> &str {
    let at = stderr.find("error: ").expect("an error line");
    &stderr[at..]
}

/// Runs `args` with and without --json: the plain run prints nothing on
/// stdout, both print the same error on stderr and exit with `code`, and the
/// JSON run prints one error object with `reason` whose message is stderr's
/// error line. Returns the message
fn fails_with(
    command: impl Fn(&[&str]) -> Command,
    args: &[&str],
    code: i32,
    reason: &str,
) -> String {
    let plain = command(args).output().unwrap();
    let json_args = [args, &["--json"]].concat();
    let json = command(&json_args).output().unwrap();
    let stderr = text(&plain.stderr);
    assert_eq!(plain.status.code(), Some(code), "{args:?}: {stderr}");
    assert_eq!(
        json.status.code(),
        Some(code),
        "{args:?}: {}",
        text(&json.stderr)
    );
    assert!(plain.stdout.is_empty(), "{args:?}: {}", text(&plain.stdout));
    let stderr = from_error(&stderr);
    assert_eq!(from_error(&text(&json.stderr)), stderr, "{args:?}");

    let value: Value = serde_json::from_slice(&json.stdout)
        .unwrap_or_else(|e| panic!("{args:?}: {e}: {}", text(&json.stdout)));
    let object = value.as_object().unwrap();
    assert_eq!(object.len(), 1, "{value}");
    let error = value["error"].as_object().expect("an error object");
    assert_eq!(error.len(), 2, "{value}");
    assert_eq!(error["reason"], reason, "{args:?}: {stderr}");
    let message = error["message"].as_str().unwrap().to_owned();
    let line = stderr.lines().next().unwrap_or_default();
    let line = line.strip_prefix("error: ").unwrap();
    assert!(message.starts_with(line), "{message:?} for {line:?}");
    assert!(!message.starts_with("error"), "{message}");
    message
}

fn in_dir(dir: &Path) -> impl Fn(&[&str]) -> Command + '_ {
    move |args| command(dir, args)
}

#[test]
fn usage_mistakes() {
    let temp = with_hike();
    let dir = temp.path();
    fs::write(dir.join("three.toml"), "[render]\nscale = \"three\"\n").unwrap();
    let message = fails_with(
        in_dir(dir),
        &["render", "hike.gpx", "--scale", "three"],
        2,
        "usage",
    );
    assert!(
        message.contains("expected a whole number from 1 to 16"),
        "{message}"
    );
    assert!(!message.contains("Usage:"), "{message}");
    fails_with(
        in_dir(dir),
        &["render", "hike.gpx", "--colour", "red"],
        2,
        "usage",
    );
    let message = fails_with(
        in_dir(dir),
        &["render", "hike.gpx", "--config", "three.toml"],
        2,
        "usage",
    );
    assert!(message.contains("scale in three.toml"), "{message}");
    fails_with(
        in_dir(dir),
        &["render", "hike.itb", "--trim-start", "300"],
        2,
        "usage",
    );
    fails_with(
        in_dir(dir),
        &["stats", "hike.gpx", "--profile", "1"],
        2,
        "usage",
    );
    fails_with(in_dir(dir), &["bundle", "hike.gpx"], 2, "usage");
}

#[test]
fn a_missing_input_for_every_command() {
    let temp = with_hike();
    for args in [
        &["render", "nothere.gpx"][..],
        &["embed", "nothere.gpx", "-o", "site"],
        &["bundle", "nothere.gpx", "-o", "x.itb"],
        &["style-lab", "nothere.gpx", "-o", "lab"],
        &["stats", "nothere.gpx"],
    ] {
        let message = fails_with(in_dir(temp.path()), args, 1, "input");
        assert_eq!(message, "can't open nothere.gpx: the file doesn't exist");
    }
}

#[test]
fn inputs_that_are_not_hikes() {
    let temp = with_hike();
    fs::create_dir(temp.path().join("site")).unwrap();
    fs::write(temp.path().join("notes.txt"), "Walked up Snowdon\n").unwrap();
    fs::write(temp.path().join("hike.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    fs::write(temp.path().join("damaged.itb"), b"ITB1\x06\x00not deflate").unwrap();
    for name in ["site", "notes.txt", "hike.png", "damaged.itb"] {
        fails_with(in_dir(temp.path()), &["render", name], 1, "input");
    }
}

#[test]
fn scene_files_too_old_or_too_new() {
    let temp = with_hike();
    for (name, bytes) in [
        ("old.itb", b"ITB1\x04\x00rest"),
        ("new.itb", b"ITB1\x07\x00rest"),
    ] {
        fs::write(temp.path().join(name), bytes).unwrap();
        fails_with(in_dir(temp.path()), &["render", name], 1, "version");
        fails_with(in_dir(temp.path()), &["stats", name], 1, "version");
    }
}

#[test]
fn offline_with_an_empty_cache() {
    let temp = with_hike();
    let dir = temp.path();
    let command = |args: &[&str]| {
        let mut command = command(dir, args);
        command
            .env_remove("ISOTRAIL_TERRAIN_URL")
            .env_remove("ISOTRAIL_OVERPASS_URL");
        command
    };
    let message = fails_with(command, &["render", "hike.gpx", "--offline"], 1, "offline");
    assert!(message.starts_with("the elevation data for this area isn't in the cache yet"));
}

#[test]
fn a_server_out_of_reach() {
    let temp = with_hike();
    fails_with(
        in_dir(temp.path()),
        &["bundle", "hike.gpx", "-o", "x.itb", "--no-weather"],
        1,
        "network",
    );
}

#[test]
fn ffmpeg_that_isnt_there() {
    let temp = with_hike();
    let message = fails_with(
        in_dir(temp.path()),
        &[
            "render",
            "hike.itb",
            "-o",
            "x.mp4",
            "--ffmpeg",
            "no-ffmpeg/ffmpeg.exe",
        ],
        1,
        "ffmpeg",
    );
    assert!(message.contains("--ffmpeg"), "{message}");
}

#[test]
fn a_gpu_that_cant_be_used() {
    let temp = with_hike();
    let command = |args: &[&str]| {
        let mut command = command(temp.path(), args);
        command.env("ISOTRAIL_GPU", "abacus");
        command
    };
    let message = fails_with(command, &["render", "hike.itb", "-o", "x.png"], 1, "gpu");
    assert!(message.contains("ISOTRAIL_GPU"), "{message}");
}

#[test]
fn anything_else() {
    let temp = with_hike();
    fs::write(temp.path().join("taken"), "").unwrap();
    let message = fails_with(
        in_dir(temp.path()),
        &["render", "hike.itb", "-o", "taken/x.png"],
        1,
        "other",
    );
    assert!(
        message.starts_with("can't create the folder taken"),
        "{message}"
    );
}

#[test]
fn help_and_version_are_not_failures() {
    let temp = with_hike();
    for args in [&["render", "--help", "--json"][..], &["--version"]] {
        let output = command(temp.path(), args).output().unwrap();
        assert!(output.status.success());
        assert!(!text(&output.stdout).contains("\"error\""));
    }
}

#[test]
fn commands_without_json_never_print_it() {
    let temp = with_hike();
    let output = command(temp.path(), &["cache", "path", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!text(&output.stdout).contains("\"error\""));
}
